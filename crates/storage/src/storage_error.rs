#[derive(Debug, PartialEq, Eq)]
pub enum StorageError {
    DuplicateKey,
    KeyNotFound,
    InsertRecordFailed,
    InsufficientSpaceForMerge,
    InvalidSlotIndex,
    NewRightSiblingNotEmpty,
    PageFull,
}
