use anyhow::Context;
use clap::Parser;
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
        Command::Flash { file: _file } => unimplemented! {},
    }

    Ok(())
}
