/// Defines the fundamental data types supported by the database system.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PrimitiveDataType {
    /// Represents an unknown or unsupported type.
    Unknown,

    /// Represents a 32-bit signed integer (maps to i32).
    Int,

    /// Represents a 64-bit signed integer (maps to i64).
    BigInt,

    /// Represents a variable-length string of characters.
    Varchar(u16),

    /// Represents a boolean value (true/false).
    Boolean,

    /// Represents a fixed-point number.
    Decimal(u8, u8),

    /// Represents a date and time value.
    DateTime,

    /// Represents a double-precision floating-point number (maps to f64).
    Float,

    /// Represents a variable-length binary data blob.
    Blob(u32),
}