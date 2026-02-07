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
            PrimitiveDataType::Decimal(..) => Some(16),
            PrimitiveDataType::Float => Some(8),
            PrimitiveDataType::DateTime => Some(8),
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_decimal_validation() {
        // Valid case: Precision > Scale
        let valid = PrimitiveDataType::decimal(10, 2);
        assert!(valid.is_ok());
        assert_eq!(valid.unwrap(), PrimitiveDataType::Decimal(10, 2));

        // Valid case: Precision == Scale
        let equal = PrimitiveDataType::decimal(5, 5);
        assert!(equal.is_ok());

        // Invalid case: Scale > Precision
        let invalid = PrimitiveDataType::decimal(2, 10);
        assert!(invalid.is_err());
        
        // verify error type pattern matching if needed
        match invalid {
            Err(SchemaError::InvalidScale { precision, scale }) => {
                assert_eq!(precision, 2);
                assert_eq!(scale, 10);
            }
            _ => panic!("Expected InvalidScale error"),
        }
    }

    #[test]
    fn test_fixed_sizes() {
        assert_eq!(PrimitiveDataType::Int.get_fixed_size(), Some(4));
        assert_eq!(PrimitiveDataType::BigInt.get_fixed_size(), Some(8));
        assert_eq!(PrimitiveDataType::Float.get_fixed_size(), Some(8));
        assert_eq!(PrimitiveDataType::Boolean.get_fixed_size(), Some(1));
        assert_eq!(PrimitiveDataType::DateTime.get_fixed_size(), Some(8));
        
        // Decimals are fixed size (16 bytes)
        assert_eq!(PrimitiveDataType::Decimal(10, 2).get_fixed_size(), Some(16));

        // Varchar and Blob are variable size -> None
        assert_eq!(PrimitiveDataType::Varchar(255).get_fixed_size(), None);
        assert_eq!(PrimitiveDataType::Blob(1024).get_fixed_size(), None);
    }
}