#[derive(Debug, PartialEq, Eq)]
pub enum StorageError {
    InvalidSlotIndex,
    PageFull,
}
