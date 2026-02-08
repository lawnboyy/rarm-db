#[derive(Debug, PartialEq, Eq)]
pub enum SchemaError {
    ArgumentMismatch { type_name: String, expected: String, found: usize },
    EmptyColumnName,
    EmptyDefaultValue,
    EmptyInput,
    InvalidColumnName,
    InvalidFormat(String),
    InvalidNumber(String),
    InvalidScale { precision: u8, scale: u8 },
    UnknownType(String),
}