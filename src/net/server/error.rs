use std::fmt::Display;

#[derive(Debug)]
pub struct WorkerError {
    err: Box<dyn std::error::Error + Send + Sync>,
}

impl Display for WorkerError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{:?}", self)
    }
}

impl std::error::Error for WorkerError {}
