use anyhow::Context;
use clap::Parser;
#[allow(unused_imports)]
use log::{debug, info, trace, warn};
use std::{fs, time::Duration};

#[derive(clap::Parser)]
#[command(author, version, about, long_about = None)]
struct Opt {
    /// Serial port
    #[arg(short, long, default_value_t = String::from("/dev/ttyUSB0"))]
    port: String,

    /// Baud rate
    #[arg(short, long, default_value_t = stm32_an3155_rs::DEFAULT_BAUDRATE)]
    baud_rate: u32,

    /// Skip baud rate initialization
    #[arg(short, long)]
    skip_initialization: bool,

    /// Serialport communication timeout, in milliseconds
    #[arg(short, long, default_value_t = 1_000u64)]
    timeout_ms: u64,

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

        /// Don't verify bytes written after flashing.
        #[arg(short, long)]
        skip_verification: bool,
    },
}

fn main() -> anyhow::Result<()> {
    env_logger::init();
    let cli = Opt::parse();

    let builder = stm32_an3155::Builder::with_path(&cli.port)
        .and_baud_rate(cli.baud_rate)
        .and_timeout(Duration::from_millis(cli.timeout_ms));

    let mut an3155 = match cli.skip_initialization {
        true => builder.skip_initialization(),
        false => builder.initialize(),
    }
    .context("Failed to create bootloader comms object")?;

    match cli.command.unwrap_or(Command::Info) {
        Command::Info => {
            let info = stm32_an3155::get_info(&mut an3155)?;
            println! {"Product ID: 0x{:04X?}", info.product_id}
            let (major, minor) = info.version.value();
            println! {"Bootloader version: {major}.{minor}"}
            print! {"Available commands: " }
            for command in &info.commands[..info.commands.len() - 1] {
                print! {"{:?}, ", command};
            }
            println! {"{:?}", info.commands.last().unwrap()};
        }
        Command::Flash {
            address: address_str,
            file,
            skip_verification,
        } => {
            let size = fs::metadata(&file)?.len() as u32;
            let address = u32::from_str_radix(&address_str.trim_start_matches("0x"), 16)
                .with_context(|| format! {"Unable to parse address from string: {address_str}"})?;
            if address < stm32_an3155_rs::DEFAULT_START_ADDRESS {
                panic! {"Invalid starting address: {address_str}"};
            }
            info! {"Flashing {file} ({size} bytes) to address: {address_str}"};

            stm32_an3155::flash(&mut an3155, address, &file, skip_verification)?;
        }
    }

    Ok(())
}
