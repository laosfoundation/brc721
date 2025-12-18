use super::Brc721Error;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum Brc721Command {
    RegisterCollection = 0x00,
    RegisterOwnership = 0x01,
}

impl std::convert::TryFrom<u8> for Brc721Command {
    type Error = Brc721Error;

    fn try_from(value: u8) -> Result<Self, Self::Error> {
        match value {
            0x00 => Ok(Brc721Command::RegisterCollection),
            0x01 => Ok(Brc721Command::RegisterOwnership),
            x => Err(Brc721Error::UnknownCommand(x)),
        }
    }
}

impl From<Brc721Command> for u8 {
    fn from(cmd: Brc721Command) -> Self {
        cmd as u8
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_try_from_valid_command() {
        let value = 0x00u8;
        let cmd = Brc721Command::try_from(value);
        assert_eq!(cmd, Ok(Brc721Command::RegisterCollection));
    }

    #[test]
    fn test_try_from_invalid_command() {
        let value = 0xFFu8;
        let cmd = Brc721Command::try_from(value);
        match cmd {
            Err(Brc721Error::UnknownCommand(x)) => assert_eq!(x, 0xFF),
            _ => panic!("Expected UnknownCommand error."),
        }
    }

    #[test]
    fn test_into_u8() {
        let cmd = Brc721Command::RegisterCollection;
        let value: u8 = cmd.into();
        assert_eq!(value, 0x00);
    }

    #[test]
    fn test_register_ownership_command_value() {
        let cmd = Brc721Command::RegisterOwnership;
        let value: u8 = cmd.into();
        assert_eq!(value, 0x01);
    }
}
