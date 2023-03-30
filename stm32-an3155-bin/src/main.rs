use anyhow::Context;
use clap::Parser;
use log::{debug, info, trace, warn};
use std::{cmp::Ordering, fs};
use stm32_an3155::{Builder, DEFAULT_BAUDRATE};

#[derive(clap::Parser)]
#[command(author, version, about, long_about = None)]
struct Opt {
    /// Serial port
    #[arg(short, long, default_value_t = String::from("/dev/ttyUSB0"))]
    port: String,

    /// Baud rate
    #[arg(short, long, default_value_t = DEFAULT_BAUDRATE)]
    baud_rate: u32,

    /// Skip baud rate initialization
    #[arg(short, long)]
    skip_initialization: bool,

    #[command(subcommand)]
    command: Option<Command>,
}

#[derive(clap::Subcommand)]
enum Command {
    /// Print bootloader information to stdout
    Info,
    /// Flash new firmware from given file
    Flash {
        /// Filename of raw firmware binary
        file: String,

        /// Starting address to write firmware to
        #[arg(short, long, default_value_t = String::from("0x08000000"))]
        address: String,
    },
}

fn main() -> anyhow::Result<()> {
    env_logger::init();
    let cli = Opt::parse();

    let builder = Builder::with_port(&cli.port).and_baud_rate(cli.baud_rate);

    let mut an3155 = match cli.skip_initialization {
        true => builder.skip_initialization(),
        false => builder.initialize(),
    }
    .context("Failed to create bootloader comms object")?;

    match cli.command.unwrap_or(Command::Info) {
        Command::Info => {
            let version = an3155.get_version()?;
            let (major, minor) = version.value();
            let commands = an3155.get_commands()?;
            let product_id = an3155.get_id()?;
            println! {"Product ID: 0x{:04X?}", product_id}
            println! {"Bootloader version: {major}.{minor}"}
            print! {"Available commands: " }
            for command in &commands[..commands.len() - 1] {
                print! {"{:?}, ", command};
            }
            println! {"{:?}", commands.last().unwrap()};
        }
        Command::Flash {
            address: address_str,
            file,
        } => {
            let size = fs::metadata(&file)?.len();
            let address = u32::from_str_radix(&address_str.trim_start_matches("0x"), 16)
                .with_context(|| format! {"Unable to parse address from string: {address_str}"})?;
            if address < stm32_an3155::DEFAULT_START_ADDRESS {
                panic! {"Invalid starting address: {address_str}"};
            }
            info! {"Flashing {size} bytes using file: {file} to address: {address_str}"};

            let pages_to_erase: Vec<u32> = {
                let start_offset = address - stm32_an3155::DEFAULT_START_ADDRESS;
                let start_page = start_offset / (stm32_an3155::DEFAULT_PAGE_SIZE as u32);
                let num_pages =
                    ((size as f64) / (stm32_an3155::DEFAULT_PAGE_SIZE as f64)).ceil() as u32;
                debug! {"starting page: {start_page}, num_pages: {num_pages}"};
                (start_page..start_page + num_pages).collect()
            };

            match an3155.get_erase_command()? {
                stm32_an3155::EraseCommand::Erase => {
                    debug! {"using standard erase command"};
                    if let Some(x) = pages_to_erase.iter().find(|&x| *x > u8::MAX.into()) {
                        panic! {"Invalid page number: {}.  Max value is {}", x, u8::MAX};
                    }
                    // Convert pages into u8 values
                    let pages_to_erase: Vec<u8> =
                        pages_to_erase.into_iter().map(|x| x as u8).collect();

                    debug! {"pages to erase: {:?}", pages_to_erase};

                    // Erase pages
                    for chunk in pages_to_erase.chunks(stm32_an3155::MAX_ERASE_PAGE_COUNT) {
                        an3155.standard_erase(chunk)?;
                    }
                }
                stm32_an3155::EraseCommand::ExtendedErase => {
                    debug! {"using extended erase command"};
                    if let Some(x) = pages_to_erase.iter().find(|&x| *x > u16::MAX.into()) {
                        panic! {"Invalid page number: {}.  Max value is {}", x, u16::MAX};
                    }
                    let pages_to_erase: Vec<u16> =
                        pages_to_erase.into_iter().map(|x| x as u16).collect();
                    debug! {"pages to erase: {:?}", pages_to_erase};
                    an3155.extended_erase(&pages_to_erase)?;
                }
            }

            info! {"writing {size} bytes to memory"};
            let bytes = fs::read(&file)?;
            for (index, chunk) in bytes
                .chunks(stm32_an3155::MAX_WRITE_BYTES_COUNT)
                .enumerate()
            {
                let addr = address + (index * stm32_an3155::MAX_WRITE_BYTES_COUNT) as u32;
                debug! {"writing chunk #{} to address: 0x{addr:08X}", index + 1}
                an3155.write_memory(addr, chunk)?;
            }

            info! {"reading back memory for verification"};
            let mut buf: Vec<u8> = Vec::with_capacity(size as usize);
            buf.resize(size as usize, 0);
            for (index, chunk) in buf
                .chunks_mut(stm32_an3155::MAX_READ_BYTES_COUNT)
                .enumerate()
            {
                let addr = address + (index * stm32_an3155::MAX_WRITE_BYTES_COUNT) as u32;
                debug! {"reading chunk #{} from address: 0x{addr:08X}", index + 1}
                an3155.read_memory(addr, chunk)?;
            }

            debug! {"comparing bytes with original file"};
            for (byte, (original, written)) in bytes.iter().zip(buf.iter()).enumerate() {
                match original.cmp(&written) {
                    Ordering::Equal => continue,
                    _ => {
                        panic! {"Verification failed for byte #{}", byte}
                    }
                }
            }
        }
    }

    Ok(())
}
