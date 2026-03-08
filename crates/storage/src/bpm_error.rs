#[derive(Debug, PartialEq, Eq)]
pub enum BufferPoolError {
    BufferFull,
    DiskRead(String),
    Generic(String),
}
