use std::fmt;

use rarmdb_schema_def::PrimitiveDataType;
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};

use crate::OrderedFloat;

/// Represents a single data value within a Record, capable of holding different types
#[derive(Clone, Debug, Deserialize, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize)]
pub enum DataValue {
    Null,
    Int(i32),
    BigInt(i64),
    Boolean(bool),
    Float(OrderedFloat),
    Text(String),
    Blob(Vec<u8>),
    DateTime(i64),
    Decimal(Decimal),
}

impl DataValue {
    pub fn is_compatible(&self, schema_type: &PrimitiveDataType) -> bool {
        match (self, *schema_type) {
            // Null is compatible with any type, but we should check if the type is nullable
            (DataValue::Null, _) => true,
            (DataValue::Int(_), PrimitiveDataType::Int) => true,
            (DataValue::Int(_), PrimitiveDataType::BigInt) => true,
            (DataValue::BigInt(_), PrimitiveDataType::BigInt) => true,
            (DataValue::Boolean(_), PrimitiveDataType::Boolean) => true,
            (DataValue::Float(_), PrimitiveDataType::Float) => true,
            (DataValue::Text(val), PrimitiveDataType::Varchar(max_len)) => {
                val.len() <= (max_len as usize)
            }
            (DataValue::Blob(val), PrimitiveDataType::Blob(max_len)) => {
                val.len() <= (max_len as usize)
            }
            (DataValue::DateTime(_), PrimitiveDataType::DateTime) => true,
            (DataValue::Decimal(val), PrimitiveDataType::Decimal(p, s)) => {
                // Check the integer part...
                let scale = val.scale();

                // If the scale is larger than the schema scale allowed, then the value is not compatible.
                if scale > (s as u32) {
                    return false;
                }

                // Get the integer part to the left of the decimal place.
                let integer_part = val.trunc().abs().mantissa();

                // We've determined the scale is compatible, so if the integer part is 0, then it is compatible
                // with any max precision from the schema type.
                if integer_part == 0 {
                    return true;
                }

                // Use log(base 10) + 1 to determine the number of digits in the integer part.
                let integer_digit_count = integer_part.checked_ilog10().unwrap_or(0) + 1;

                scale <= (s as u32) && integer_digit_count <= ((p - s) as u32)
            }
            _ => false,
        }
    }
}

impl fmt::Display for DataValue {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            DataValue::BigInt(val) => write!(f, "{}", val),
            DataValue::Int(val) => write!(f, "{}", val),
            DataValue::Blob(val) => write!(f, "<BLOB length={}>", val.len()),
            DataValue::Boolean(val) => write!(f, "{}", val),
            DataValue::DateTime(val) => write!(f, "{}", val),
            DataValue::Decimal(val) => write!(f, "{}", val),
            DataValue::Float(val) => write!(f, "{}", val.0),
            DataValue::Text(val) => write!(f, "{}", val),
            DataValue::Null => write!(f, "NULL"),
        }
    }
}

#[cfg(test)]
mod tests {
    use std::{collections::HashSet, str::FromStr};

    use rarmdb_schema_def::PrimitiveDataType;

    use super::*;

    #[test]
    fn test_float_nan_equality() {
        let v1 = DataValue::Float(OrderedFloat(f64::NAN));
        let v2 = DataValue::Float(OrderedFloat(f64::NAN));
        assert_eq!(v1, v2);
    }

    #[test]
    fn test_blob_equality() {
        let v1 = DataValue::Blob(vec![1, 2, 3]);
        let v2 = DataValue::Blob(vec![1, 2, 3]);
        assert_eq!(v1, v2);
    }

    #[test]
    fn test_structural_ordering() {
        let null_val = DataValue::Null;
        let int_val = DataValue::Int(0);
        assert!(null_val < int_val);
    }

    #[test]
    fn test_decimal_equality_normalization() {
        let d1 = DataValue::Decimal(Decimal::from_str("1.0").unwrap());
        let d2 = DataValue::Decimal(Decimal::from_str("1.00").unwrap());
        assert_eq!(d1, d2);
    }

    #[test]
    fn test_hashing_capabilities() {
        let mut set = HashSet::new();
        let v1 = DataValue::Int(42);
        set.insert(v1.clone());
        assert!(set.contains(&v1));
    }

    #[test]
    fn test_type_compatibility_varchar() {
        // Schema: VARCHAR(5)
        let schema_type = PrimitiveDataType::Varchar(5);

        // Valid: "Hello" (5 chars)
        let val_valid = DataValue::Text("Hello".to_string());
        assert!(
            val_valid.is_compatible(&schema_type),
            "'Hello' should fit in VARCHAR(5)"
        );

        // Invalid: "Hello World" (11 chars)
        let val_too_long = DataValue::Text("Hello World".to_string());
        assert!(
            !val_too_long.is_compatible(&schema_type),
            "'Hello World' should NOT fit in VARCHAR(5)"
        );
    }

    #[test]
    fn test_type_compatibility_blob() {
        // Schema: BLOB(5)
        let schema_type = PrimitiveDataType::Blob(5);

        // Valid: 5 bytes
        let val_valid = DataValue::Blob(vec![1, 2, 3, 4, 5]);
        assert!(
            val_valid.is_compatible(&schema_type),
            "5 bytes should fit in BLOB(5)"
        );

        // Valid: Empty
        let val_empty = DataValue::Blob(vec![]);
        assert!(
            val_empty.is_compatible(&schema_type),
            "Empty bytes should fit in BLOB(5)"
        );

        // Invalid: 6 bytes
        let val_too_long = DataValue::Blob(vec![1, 2, 3, 4, 5, 6]);
        assert!(
            !val_too_long.is_compatible(&schema_type),
            "6 bytes should NOT fit in BLOB(5)"
        );
    }

    #[test]
    fn test_type_compatibility_mismatch() {
        let schema_blob = PrimitiveDataType::Blob(10);
        let val_text = DataValue::Text("hello".to_string());
        assert!(
            !val_text.is_compatible(&schema_blob),
            "Text should not be compatible with Blob schema"
        );

        let schema_varchar = PrimitiveDataType::Varchar(10);
        let val_blob = DataValue::Blob(vec![1]);
        assert!(
            !val_blob.is_compatible(&schema_varchar),
            "Blob should not be compatible with Varchar schema"
        );
    }

    #[test]
    fn test_type_compatibility_int() {
        let schema_int = PrimitiveDataType::Int;
        let schema_bigint = PrimitiveDataType::BigInt;

        let val_int = DataValue::Int(100);
        let val_bigint = DataValue::BigInt(100);

        // Int fits in Int
        assert!(val_int.is_compatible(&schema_int));

        // Int fits in BigInt (Upcasting)
        assert!(val_int.is_compatible(&schema_bigint));

        // BigInt does NOT fit in Int (Strict type checking)
        assert!(!val_bigint.is_compatible(&schema_int));
    }

    #[test]
    fn test_type_compatibility_decimal() {
        // Schema: DECIMAL(4, 2) -> Max 99.99
        let schema_decimal = PrimitiveDataType::Decimal(4, 2);

        // Valid: 10.50 (Precision 4, Scale 2)
        let val_valid = DataValue::Decimal(Decimal::from_str("10.50").unwrap());
        assert!(val_valid.is_compatible(&schema_decimal));

        // Valid: 1.1 (Effective precision 2, scale 1 -> fits in 4, 2)
        let val_small = DataValue::Decimal(Decimal::from_str("1.1").unwrap());
        assert!(val_small.is_compatible(&schema_decimal));

        // Valid: 0.99 (Integer part 0 fits in 4-2=2 digits)
        let val_zero_int = DataValue::Decimal(Decimal::from_str("0.99").unwrap());
        assert!(val_zero_int.is_compatible(&schema_decimal));

        // Invalid: 100.50 (Requires Precision 5: 1-0-0-5-0)
        let val_overflow = DataValue::Decimal(Decimal::from_str("100.50").unwrap());
        assert!(!val_overflow.is_compatible(&schema_decimal));

        // Invalid: Scale too high
        let val_scale = DataValue::Decimal(Decimal::from_str("1.123").unwrap());
        assert!(!val_scale.is_compatible(&schema_decimal));
    }
}
