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
    let cli = Opt::parse();

    let mut an3155 = Builder::with_port(&cli.port)
        .and_baud_rate(cli.baud_rate)
        .initialize()?;

    match cli.command.unwrap_or(Command::Info) {
        Command::Info => {
            let version = an3155.get_version()?;
            let (major, minor) = version.value();
            println! {"Bootloader version: {major}.{minor}"}
        }
        Command::Flash { file: _file } => unimplemented! {},
    }

    Ok(())
}
