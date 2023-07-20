use anyhow::Context;
use log::{debug, error, info, trace, warn};
use std::{
    cmp::Ordering,
    fs,
    io::{Read, Write},
    time::Duration,
};
use stm32_an3155_rs::AN3155;

pub struct Builder<'a> {
    baud_rate: Option<u32>,
    timeout: Option<Duration>,
    path: &'a str,
}

impl<'a> Builder<'a> {
    pub fn with_path(path: &'a str) -> Self {
        Self {
            path,
            baud_rate: None,
            timeout: None,
        }
    }

    pub fn and_baud_rate(mut self, baud_rate: u32) -> Self {
        self.baud_rate.replace(baud_rate);
        self
    }

    pub fn and_timeout(mut self, timeout: Duration) -> Self {
        self.timeout.replace(timeout);
        self
    }

    fn build_serialport(self) -> anyhow::Result<Box<dyn serialport::SerialPort>> {
        let path = self.path;
        let baud_rate = self.baud_rate.unwrap_or(stm32_an3155_rs::DEFAULT_BAUDRATE);
        info!("opening serial port: {path} {baud_rate} 8E1");
        serialport::new(path, baud_rate)
            .parity(serialport::Parity::Even)
            .stop_bits(serialport::StopBits::One)
            .data_bits(serialport::DataBits::Eight)
            .timeout(self.timeout.unwrap_or(Duration::from_secs(1)))
            .open()
            .context("Failed to open serialport device")
    }

    /// Skip bootloader comms initialization
    ///
    /// This can be useful if you've already communicated with
    /// the bootloader and need to send new commands.  To be
    /// successful you must use the same baud rate as the
    /// original session
    pub fn skip_initialization(self) -> anyhow::Result<AN3155<Box<dyn serialport::SerialPort>>> {
        let serial = self.build_serialport()?;
        Ok(AN3155::with(serial))
    }

    /// Initialize comms with the bootloader
    pub fn initialize(self) -> anyhow::Result<AN3155<Box<dyn serialport::SerialPort>>> {
        let mut serial = self.build_serialport()?;

        info!("writing baudrate sync byte");
        serial
            .write(&[stm32_an3155_rs::SYNC_BYTE][..])
            .context("Failed to send baudrate sync byte")?;
        let mut buf = [0u8];
        info!("waiting for bootloader response");
        serial
            .read(&mut buf[..])
            .context("Failed to read response from bootloader")?;

        Ok(AN3155::with(serial))
    }
}

pub struct Info {
    pub version: stm32_an3155_rs::Version,
    pub commands: Vec<stm32_an3155_rs::BootloaderCommand>,
    pub product_id: u16,
}

pub fn get_info<T: Read + Write>(an3155: &mut stm32_an3155_rs::AN3155<T>) -> anyhow::Result<Info> {
    let version = an3155.get_version()?;
    let commands = an3155.get_commands()?;
    let product_id = an3155.get_id()?;
    Ok(Info {
        version,
        commands,
        product_id,
    })
}

pub fn erase<T: Read + Write>(
    an3155: &mut stm32_an3155_rs::AN3155<T>,
    address: u32,
    bytes: u32,
) -> anyhow::Result<()> {
    let pages_to_erase: Vec<u32> = {
        let start_offset = address - stm32_an3155_rs::DEFAULT_START_ADDRESS;
        let start_page = start_offset / (stm32_an3155_rs::DEFAULT_PAGE_SIZE as u32);
        let num_pages =
            ((bytes as f64) / (stm32_an3155_rs::DEFAULT_PAGE_SIZE as f64)).ceil() as u32;
        debug! {"starting page: {start_page}, num_pages: {num_pages}"};
        (start_page..start_page + num_pages).collect()
    };
    if pages_to_erase.len() == 0 {
        warn! {"no pages found to erase using address: {} and bytes: {}", address, bytes};
        return Ok(());
    }
    info! {"erasing {} pages starting at page #{}", pages_to_erase.first().unwrap(), pages_to_erase.last().unwrap()};

    match an3155.get_erase_command()? {
        stm32_an3155_rs::EraseCommand::Erase => {
            debug! {"using standard erase command"};
            if let Some(x) = pages_to_erase.iter().find(|&x| *x > u8::MAX.into()) {
                panic! {"Invalid page number: {}.  Max value is {}", x, u8::MAX};
            }
            // Convert pages into u8 values
            let pages_to_erase: Vec<u8> = pages_to_erase.into_iter().map(|x| x as u8).collect();

            debug! {"pages to erase: {:?}", pages_to_erase};

            // Erase pages
            for chunk in pages_to_erase.chunks(stm32_an3155_rs::MAX_ERASE_PAGE_COUNT) {
                an3155.standard_erase(chunk)?;
            }
        }
        stm32_an3155_rs::EraseCommand::ExtendedErase => {
            debug! {"using extended erase command"};
            if let Some(x) = pages_to_erase.iter().find(|&x| *x > u16::MAX.into()) {
                panic! {"Invalid page number: {}.  Max value is {}", x, u16::MAX};
            }
            let pages_to_erase: Vec<u16> = pages_to_erase.into_iter().map(|x| x as u16).collect();
            debug! {"pages to erase: {:?}", pages_to_erase};
            an3155.extended_erase(&pages_to_erase)?;
        }
    };
    Ok(())
}

pub fn flash<T: Read + Write>(
    an3155: &mut stm32_an3155_rs::AN3155<T>,
    address: u32,
    file: &str,
    skip_verification: bool,
) -> anyhow::Result<()> {
    let bytes = fs::read(&file)?;
    info! {"writing {} bytes to memory", bytes.len()};
    for (index, chunk) in bytes
        .chunks(stm32_an3155_rs::MAX_WRITE_BYTES_COUNT)
        .enumerate()
    {
        let addr = address + (index * stm32_an3155_rs::MAX_WRITE_BYTES_COUNT) as u32;
        debug! {"writing chunk #{} to address: 0x{addr:08X}", index + 1}
        an3155.write_memory(addr, chunk)?;
        if !skip_verification {
            info! {"reading back memory for verification"};
            let mut buf = vec![0u8; chunk.len()];
            debug! {"reading chunk #{} from address: 0x{addr:08X}", index + 1}
            an3155.read_memory(addr, &mut buf)?;
            debug! {"comparing bytes with original file"};
            for (byte, (original, written)) in chunk.iter().zip(buf.iter()).enumerate() {
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
