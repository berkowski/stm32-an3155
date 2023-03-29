use anyhow::Context;
use thiserror::Error as ThisError;

use std::{
    convert::{AsRef, TryFrom},
    ffi::OsStr,
    io::{Error as IoError, ErrorKind as IoErrorKind},
    marker::PhantomData,
    path::Path,
};

/// Baudrate sync byte used during initialization
const SYNC_BYTE: u8 = 0x7F;

/// Default baud rate
pub const DEFAULT_BAUDRATE: u32 = 57_600;

#[repr(u8)]
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum BootloaderCommand {
    /// Gets the version and the allowed commands supported by the current version of the protocol.
    Get = 0x00,
    /// Gets the protocol version.
    GetVersion = 0x01,
    /// Gets the chip ID.
    GetId = 0x02,
    /// Reads up to 256 bytes of memory starting from an address specified by the application.
    ReadMemory = 0x11,
    /// Jumps to user application code located in the internal flash memory or in the SRAM.
    Go = 0x21,
    /// Writes up to 256 bytes to the RAM or flash memory starting from an address specified by the application
    WriteMemory = 0x31,
    /// Erases from one to all the flash memory pages.
    Erase = 0x43,
    /// Erases from one to all the flash memory pages using two-byte addressing mode (available only for USART bootloader v3.0 and higher).
    ExtendedErase = 0x44,
    /// Generic command that allows to add new features depending on the product constraints, without adding a new command for every feature.
    Special = 0x50,
    /// Generic command that allows the user to send more data compared to the Special command
    ExtendedSpecial = 0x51,
    /// Enables the write protection for some sectors.
    WriteProtect = 0x63,
    /// Disables the write protection for all flash memory sectors.
    WriteUnprotect = 0x73,
    /// Enables the read protection
    ReadoutProtect = 0x82,
    /// Disables the read protection
    ReadoutUnprotect = 0x92,
    /// Computes a CRC value on a given memory area with a size multiple of 4 bytes.
    GetChecksum = 0xA1,
}

impl TryFrom<u8> for BootloaderCommand {
    type Error = Error;
    fn try_from(v: u8) -> Result<Self, Self::Error> {
        match v {
            0x00 => Ok(Self::Get),
            0x01 => Ok(Self::GetVersion),
            0x02 => Ok(Self::GetId),
            0x11 => Ok(Self::ReadMemory),
            0x21 => Ok(Self::Go),
            0x31 => Ok(Self::WriteMemory),
            0x43 => Ok(Self::Erase),
            0x44 => Ok(Self::ExtendedErase),
            0x50 => Ok(Self::Special),
            0x51 => Ok(Self::ExtendedSpecial),
            0x63 => Ok(Self::WriteProtect),
            0x73 => Ok(Self::WriteUnprotect),
            0x82 => Ok(Self::ReadoutProtect),
            0x92 => Ok(Self::ReadoutUnprotect),
            0xA1 => Ok(Self::GetChecksum),
            _ => Err(Error::InvalidBootloaderCommand(v)),
        }
    }
}

/// Packet response from bootloader
#[repr(u8)]
pub enum Response {
    /// Accepted
    Ack = 0x79,
    /// Not accepted
    Nack = 0x1F,
}

/// Type of erase command used on chip
///
/// Each chip's bootloader will support either the Erase command or
/// the ExtendedErase command.  The commands are mutually exclusive
pub enum EraseCommand {
    /// Normal erase command
    Erase,
    /// Erase command using two-byte addressing mode
    ExtendedErase,
}

impl TryFrom<u8> for Response {
    type Error = Error;

    fn try_from(v: u8) -> Result<Self, Self::Error> {
        match v {
            0x79 => Ok(Self::Ack),
            0x1F => Ok(Self::Nack),
            _ => Err(Error::InvalidResponse(v)),
        }
    }
}

#[derive(ThisError, Debug)]
pub enum Error {
    #[error("invalid response from bootloader: 0x{0:02X}")]
    InvalidResponse(u8),

    #[error("received a NACK from bootloader")]
    Nack,

    #[error("invalid bootloader command: 0x{0:02X}")]
    InvalidBootloaderCommand(u8),

    #[error("unsupported operation")]
    Unsupported,
}

/// Bootloader version
///
/// # Example
/// ```
/// # use stm32_an3155::Version;
/// let ver = Version::from(0x10);
///
/// assert_eq!(1, ver.major());
/// assert_eq!(0, ver.minor());
/// assert_eq!((1, 0), ver.value());
/// ```
pub struct Version(u8);

impl Version {
    pub fn value(&self) -> (u8, u8) {
        (self.major(), self.minor())
    }

    pub fn major(&self) -> u8 {
        self.0 >> 4
    }

    pub fn minor(&self) -> u8 {
        self.0 & 0x0F
    }
}

impl From<u8> for Version {
    fn from(v: u8) -> Self {
        Self(v)
    }
}

pub struct Builder<'a> {
    baud_rate: Option<u32>,
    path: &'a str,
}

impl<'a> Builder<'a> {
    pub fn with_port(path: &'a str) -> Self {
        Self {
            path,
            baud_rate: None,
        }
    }

    pub fn and_baud_rate(self, baud_rate: u32) -> Self {
        Self {
            path: self.path,
            baud_rate: Some(baud_rate),
        }
    }

    pub fn initialize(self) -> anyhow::Result<AN3155> {
        let Builder { path, baud_rate } = self;
        let mut serial = serialport::new(path, baud_rate.unwrap_or(DEFAULT_BAUDRATE))
            .parity(serialport::Parity::Even)
            .stop_bits(serialport::StopBits::One)
            .data_bits(serialport::DataBits::Eight)
            .open()
            .context("Failed to open serialport device")?;

        serial
            .write(&[SYNC_BYTE][..])
            .context("Failed to send baudrate sync byte")?;
        let mut buf = [0u8];
        serial
            .read(&mut buf[..])
            .context("Failed to read response from bootloader")?;

        Ok(AN3155 { serial })
    }
}

pub struct AN3155 {
    serial: Box<dyn serialport::SerialPort>,
}

impl AN3155 {
    fn write(&mut self, bytes: &[u8]) -> anyhow::Result<usize> {
        self.serial
            .write(bytes)
            .context("Failed to write data to serial port")
    }

    fn write_with_checksum(&mut self, bytes: &[u8]) -> anyhow::Result<usize> {
        let chksum = bytes.iter().fold(0u8, |acc, b| acc ^ *b);
        let n = self.write(bytes)?;
        let _ = self
            .serial
            .write(&[chksum][..])
            .context("Failed to write checksum")?;
        Ok(n + 1)
    }

    fn read(&mut self, buf: &mut [u8]) -> anyhow::Result<usize> {
        self.serial
            .read(buf)
            .context("Failed to read from serialport")
    }

    fn read_exact(&mut self, buf: &mut [u8]) -> anyhow::Result<()> {
        let n = self.read(buf)?;
        if n != buf.len() {
            Err(IoError::from(IoErrorKind::UnexpectedEof).into())
        } else {
            Ok(())
        }
    }

    fn read_byte(&mut self) -> anyhow::Result<u8> {
        let mut byte = [0u8];
        let _ = self.read_exact(&mut byte[..])?;
        Ok(byte[0])
    }

    fn read_ack(&mut self) -> anyhow::Result<()> {
        let byte = self.read_byte()?;
        match Response::try_from(byte).context("Failed to read valid response from bootloader")? {
            Response::Ack => Ok(()),
            Response::Nack => Err(Error::Nack.into()),
        }
    }

    /// Get the bootloader version
    pub fn get_version(&mut self) -> anyhow::Result<Version> {
        self.write_with_checksum(&[BootloaderCommand::GetVersion as u8][..])
            .context("Failed to send GetVersion command")?;
        self.read_ack()?;
        let byte = self
            .read_byte()
            .context("Failed to read protocol version byte")?;

        let mut buf = [0u8, 0u8];
        self.read_exact(&mut buf)
            .context("Failed to read compatability bytes")?;
        self.read_ack()?;
        Ok(Version::from(byte))
    }

    /// Get the bootloader commands
    pub fn get_commands(&mut self) -> anyhow::Result<Vec<BootloaderCommand>> {
        self.write_with_checksum(&[BootloaderCommand::Get as u8][..])
            .context("Failed to send Get command")?;
        self.read_ack()?;

        let n = self
            .read_byte()
            .context("Failed to read protocol version byte")?;

        let mut buf = Vec::with_capacity(n as usize);
        buf.resize(n as usize, 0);
        self.read_exact(&mut buf)
            .context("Failed to read bootloader command list")?;
        self.read_ack()?;
        let mut commands: Vec<BootloaderCommand> = Vec::with_capacity(n as usize);
        for b in buf {
            commands.push(
                BootloaderCommand::try_from(b)
                    .context("Bootloader returned an unknown command value")?,
            );
        }
        Ok(commands)
    }

    fn get_erase_command(&mut self) -> anyhow::Result<EraseCommand> {
        let commands = self
            .get_commands()
            .context("Failed to get bootloader command list")?;

        if commands.contains(&BootloaderCommand::Erase) {
            Ok(EraseCommand::Erase)
        } else if commands.contains(&BootloaderCommand::ExtendedErase) {
            Ok(EraseCommand::ExtendedErase)
        } else {
            Err(Error::Unsupported.into())
        }
    }
}
