#[derive(Debug, PartialEq, Eq)]
pub enum StorageError {
    DuplicateKey,
    KeyNotFound,
    InvalidSlotIndex,
    NewRightSiblingNotEmpty,
    PageFull,
}
