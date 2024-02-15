use std::fmt::Display;

#[derive(Debug)]
pub struct PacketTypeCastError(pub i32);

impl PacketTypeCastError {
    pub fn new(value: i32) -> Self {
        Self(value)
    }
}

impl Display for PacketTypeCastError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{:?}", self)
    }
}

impl std::error::Error for PacketTypeCastError {}
