#[derive(Debug, PartialEq, Eq)]
pub enum SerializationError {
    DataTypeMismatch,
    NullValueForNonNullColumnFound,
}
