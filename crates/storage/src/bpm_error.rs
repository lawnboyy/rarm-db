#[derive(Debug, PartialEq, Eq)]
pub enum BufferPoolError {
    BufferFull,
    DiskRead(String),
    DiskWrite(String),
    Generic(String),
    InFlightBroadcast(String),
}
