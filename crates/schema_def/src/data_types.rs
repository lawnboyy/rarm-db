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

#[derive(Debug)]
pub enum SchemaError {
    InvalidScale { precision: u8, scale: u8 },
}

impl PrimitiveDataType {
    // A "Smart Constructor" for Decimal
    pub fn decimal(precision: u8, scale: u8) -> Result<Self, SchemaError> {
        if scale > precision {
            // Return the Failure variant
            return Err(SchemaError::InvalidScale { precision, scale });
        }
        
        Ok(PrimitiveDataType::Decimal(precision, scale))
    }

    pub fn get_fixed_size(&self) -> Option<usize> {
        match self {
            PrimitiveDataType::Int => Some(4),
            PrimitiveDataType::BigInt => Some(8),
            PrimitiveDataType::Boolean => Some(1),
            PrimitiveDataType::Decimal { .. } => Some(16),
            _ => None,
        }
    }
}