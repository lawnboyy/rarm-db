#[derive(Debug, PartialEq, Eq)]
pub enum SchemaError {
    ArgumentMismatch {
        type_name: String,
        expected: String,
        found: usize,
    },
    EmptyDefaultValue,
    EmptyInput,
    ForeignKeyColumnMismatch,
    InvalidColumnName,
    InvalidFormat(String),
    InvalidKeyName,
    InvalidNumber(String),
    InvalidScale {
        precision: u8,
        scale: u8,
    },
    UnknownType(String),
}
