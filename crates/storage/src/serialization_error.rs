#[derive(Debug, PartialEq, Eq)]
pub enum SerializationError {
    DataTypeMismatch,
    PrimaryKeyNotFound,
    NullPrimaryKeyColumnValue,
    NullValueForNonNullColumnFound,
}
