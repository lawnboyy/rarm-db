#[derive(Debug, PartialEq, Eq)]
pub enum BufferPoolError {
    BufferFull,
    Generic(String),
}
