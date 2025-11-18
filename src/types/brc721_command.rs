#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum Brc721Command {
    RegisterCollection = 0x00,
}

impl std::convert::TryFrom<u8> for Brc721Command {
    type Error = ();

    fn try_from(value: u8) -> Result<Self, Self::Error> {
        match value {
            0x00 => Ok(Brc721Command::RegisterCollection),
            _ => Err(()),
        }
    }
}
