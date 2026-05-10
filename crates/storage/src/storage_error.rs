#[derive(Debug, PartialEq, Eq)]
pub enum StorageError {
    DuplicateKey,
    InvalidSlotIndex,
    PageFull,
}
