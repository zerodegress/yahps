use std::fmt::Display;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum HandlePacketErrorKind {
    Ignorable,
    Disconnect,
    Fatal,
}

#[derive(Debug)]
pub struct HandlePacketError {
    kind: HandlePacketErrorKind,
    err: Box<dyn std::error::Error + Sync + Send>,
}

impl Display for HandlePacketError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{:?}", self)
    }
}

impl std::error::Error for HandlePacketError {}

impl HandlePacketError {
    pub fn new<E>(kind: HandlePacketErrorKind, err: E) -> Self
    where
        E: Into<Box<dyn std::error::Error + Sync + Send>>,
    {
        Self {
            kind,
            err: err.into(),
        }
    }

    pub fn kind(&self) -> &HandlePacketErrorKind {
        &self.kind
    }
}
