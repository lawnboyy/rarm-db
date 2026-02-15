use serde::{Deserialize, Serialize};
use std::str::FromStr;

use crate::SchemaError;

/// Defines the fundamental data types supported by the database system.
#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Eq, Serialize)]
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

    /// Represents a fixed-point number with precision (total number of significant digits)
    /// and scale (number of significant digits to the right of the decimal place).
    Decimal(u8, u8),

    /// Represents a date and time value.
    DateTime,

    /// Represents a double-precision floating-point number (maps to f64).
    Float,

    /// Represents a variable-length binary data blob.
    Blob(u32),
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

    pub fn is_fixed_size(&self) -> bool {
        !matches!(
            self,
            PrimitiveDataType::Blob(_) | PrimitiveDataType::Varchar(_) | PrimitiveDataType::Unknown
        )
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

impl FromStr for PrimitiveDataType {
    type Err = SchemaError;

    fn from_str(input: &str) -> Result<Self, Self::Err> {
        let input = input.trim();
        if input.is_empty() {
            return Err(SchemaError::EmptyInput);
        }

        let normalized = input.to_ascii_uppercase();

        // Split name and args
        // Example: "DECIMAL(10, 2)" -> name="DECIMAL", args_str="10, 2"
        let (type_name, args_str) = match normalized.find('(') {
            Some(open_idx) => {
                if !normalized.ends_with(')') {
                    return Err(SchemaError::InvalidFormat(format!(
                        "Missing closing parenthesis in '{}'",
                        input
                    )));
                }
                // Slice before '('
                let name = &normalized[..open_idx];
                // Slice between '(' and ')'
                let args = &normalized[open_idx + 1..normalized.len() - 1];
                (name.trim(), Some(args))
            }
            None => (normalized.as_str(), None),
        };

        // Parse args into vector of strings
        let args: Vec<&str> = match args_str {
            Some(s) if !s.trim().is_empty() => s.split(',').map(|x| x.trim()).collect(),
            _ => Vec::new(),
        };

        match type_name {
            "INT" | "INTEGER" => {
                if !args.is_empty() {
                    return Err(SchemaError::ArgumentMismatch {
                        type_name: "INT".into(),
                        expected: "0".into(),
                        found: args.len(),
                    });
                }
                Ok(PrimitiveDataType::Int)
            }
            "BIGINT" => {
                if !args.is_empty() {
                    return Err(SchemaError::ArgumentMismatch {
                        type_name: "BIGINT".into(),
                        expected: "0".into(),
                        found: args.len(),
                    });
                }
                Ok(PrimitiveDataType::BigInt)
            }
            "VARCHAR" => {
                if args.len() != 1 {
                    return Err(SchemaError::ArgumentMismatch {
                        type_name: "VARCHAR".into(),
                        expected: "1".into(),
                        found: args.len(),
                    });
                }
                let len = args[0]
                    .parse::<u16>()
                    .map_err(|_| SchemaError::InvalidNumber(args[0].into()))?;
                Ok(PrimitiveDataType::Varchar(len))
            }
            "BOOLEAN" | "BOOL" => {
                if !args.is_empty() {
                    return Err(SchemaError::ArgumentMismatch {
                        type_name: "BOOLEAN".into(),
                        expected: "0".into(),
                        found: args.len(),
                    });
                }
                Ok(PrimitiveDataType::Boolean)
            }
            "DECIMAL" => {
                if args.is_empty() {
                    // Default precision/scale if not provided (e.g. 18, 0)
                    Ok(PrimitiveDataType::Decimal(18, 0))
                } else if args.len() == 2 {
                    let p = args[0]
                        .parse::<u8>()
                        .map_err(|_| SchemaError::InvalidNumber(args[0].into()))?;
                    let s = args[1]
                        .parse::<u8>()
                        .map_err(|_| SchemaError::InvalidNumber(args[1].into()))?;
                    PrimitiveDataType::decimal(p, s)
                } else {
                    Err(SchemaError::ArgumentMismatch {
                        type_name: "DECIMAL".into(),
                        expected: "0 or 2".into(),
                        found: args.len(),
                    })
                }
            }
            "DATETIME" => {
                if !args.is_empty() {
                    return Err(SchemaError::ArgumentMismatch {
                        type_name: "DATETIME".into(),
                        expected: "0".into(),
                        found: args.len(),
                    });
                }
                Ok(PrimitiveDataType::DateTime)
            }
            "FLOAT" | "DOUBLE" => {
                if !args.is_empty() {
                    return Err(SchemaError::ArgumentMismatch {
                        type_name: "FLOAT".into(),
                        expected: "0".into(),
                        found: args.len(),
                    });
                }
                Ok(PrimitiveDataType::Float)
            }
            "BLOB" => {
                if args.len() != 1 {
                    return Err(SchemaError::ArgumentMismatch {
                        type_name: "BLOB".into(),
                        expected: "1".into(),
                        found: args.len(),
                    });
                }
                let len = args[0]
                    .parse::<u32>()
                    .map_err(|_| SchemaError::InvalidNumber(args[0].into()))?;
                Ok(PrimitiveDataType::Blob(len))
            }
            _ => Err(SchemaError::UnknownType(type_name.into())),
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

    #[test]
    fn test_is_fixed_size() {
        assert!(PrimitiveDataType::Int.is_fixed_size());
        assert!(PrimitiveDataType::BigInt.is_fixed_size());
        assert!(PrimitiveDataType::Float.is_fixed_size());
        assert!(PrimitiveDataType::Boolean.is_fixed_size());
        assert!(PrimitiveDataType::DateTime.is_fixed_size());
        assert!(PrimitiveDataType::Decimal(10, 2).is_fixed_size());

        assert!(!PrimitiveDataType::Varchar(255).is_fixed_size());
        assert!(!PrimitiveDataType::Blob(1024).is_fixed_size());
        assert!(!PrimitiveDataType::Unknown.is_fixed_size());
    }

    #[test]
    fn test_from_str() {
        // Basic types
        assert_eq!(
            "INT".parse::<PrimitiveDataType>(),
            Ok(PrimitiveDataType::Int)
        );
        assert_eq!(
            "integer".parse::<PrimitiveDataType>(),
            Ok(PrimitiveDataType::Int)
        );
        assert_eq!(
            "BIGINT".parse::<PrimitiveDataType>(),
            Ok(PrimitiveDataType::BigInt)
        );
        assert_eq!(
            "BOOLEAN".parse::<PrimitiveDataType>(),
            Ok(PrimitiveDataType::Boolean)
        );
        assert_eq!(
            "BOOL".parse::<PrimitiveDataType>(),
            Ok(PrimitiveDataType::Boolean)
        );
        assert_eq!(
            "DATETIME".parse::<PrimitiveDataType>(),
            Ok(PrimitiveDataType::DateTime)
        );
        assert_eq!(
            "FLOAT".parse::<PrimitiveDataType>(),
            Ok(PrimitiveDataType::Float)
        );

        // Parameterized types
        assert_eq!(
            "VARCHAR(100)".parse::<PrimitiveDataType>(),
            Ok(PrimitiveDataType::Varchar(100))
        );
        assert_eq!(
            "varchar( 255 )".parse::<PrimitiveDataType>(),
            Ok(PrimitiveDataType::Varchar(255))
        ); // whitespace check
        assert_eq!(
            "BLOB(1024)".parse::<PrimitiveDataType>(),
            Ok(PrimitiveDataType::Blob(1024))
        );

        // Decimal variants
        assert_eq!(
            "DECIMAL".parse::<PrimitiveDataType>(),
            Ok(PrimitiveDataType::Decimal(18, 0))
        ); // Default
        assert_eq!(
            "DECIMAL(10, 2)".parse::<PrimitiveDataType>(),
            Ok(PrimitiveDataType::Decimal(10, 2))
        );
        assert_eq!(
            "DECIMAL( 5 , 5 )".parse::<PrimitiveDataType>(),
            Ok(PrimitiveDataType::Decimal(5, 5))
        );

        // Errors
        assert!(matches!(
            "FOOBAR".parse::<PrimitiveDataType>(),
            Err(SchemaError::UnknownType(_))
        ));
        assert!(matches!(
            "INT(10)".parse::<PrimitiveDataType>(),
            Err(SchemaError::ArgumentMismatch { .. })
        ));
        assert!(matches!(
            "VARCHAR".parse::<PrimitiveDataType>(),
            Err(SchemaError::ArgumentMismatch { .. })
        )); // Missing args
        assert!(matches!(
            "VARCHAR(ABC)".parse::<PrimitiveDataType>(),
            Err(SchemaError::InvalidNumber(_))
        ));
        assert!(matches!(
            "DECIMAL(2, 10)".parse::<PrimitiveDataType>(),
            Err(SchemaError::InvalidScale { .. })
        ));
    }
}
