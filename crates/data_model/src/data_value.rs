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
                let mut iterations = 0;
                let mut remainder = integer_part;
                while remainder >= 10 {
                    remainder = remainder / 10;
                    iterations += 1;
                }

                let integer_digit_count = iterations + 1;

                return scale <= (s as u32) && integer_digit_count <= p - s;
            }
            (_, _) => false,
        }
    }

    //fn log(integer: &)
}

#[cfg(test)]
mod tests {
    use std::{collections::HashSet, str::FromStr};

    use rarmdb_schema_def::PrimitiveDataType;

    use super::*;

    #[test]
    fn test_float_nan_equality() {
        // Critical: Verify that the Enum wraps the OrderedFloat logic correctly.
        // Standard floats are not equal (NaN != NaN), but our DataValue must be.
        let v1 = DataValue::Float(OrderedFloat(f64::NAN));
        let v2 = DataValue::Float(OrderedFloat(f64::NAN));
        let v3 = DataValue::Float(OrderedFloat(1.0));

        assert_eq!(
            v1, v2,
            "DataValue::Float(NaN) should equal DataValue::Float(NaN)"
        );
        assert_ne!(v1, v3);
    }

    #[test]
    fn test_blob_equality() {
        // Verify that Blobs compare by value (content), not reference.
        let v1 = DataValue::Blob(vec![1, 2, 3]);
        let v2 = DataValue::Blob(vec![1, 2, 3]);
        let v3 = DataValue::Blob(vec![1, 2, 4]);

        assert_eq!(v1, v2);
        assert_ne!(v1, v3);
    }

    #[test]
    fn test_structural_ordering() {
        // Verify that Null sorts before other values (based on Enum definition order)
        let null_val = DataValue::Null;
        let int_val = DataValue::Int(0);

        assert!(
            null_val < int_val,
            "Null should be less than Int based on enum variant order"
        );

        // Verify Int < BigInt (structural)
        let big_int_val = DataValue::BigInt(0);
        assert!(int_val < big_int_val);
    }

    #[test]
    fn test_decimal_equality_normalization() {
        // Verify that 1.0 == 1.00
        let d1 = DataValue::Decimal(Decimal::from_str("1.0").unwrap());
        let d2 = DataValue::Decimal(Decimal::from_str("1.00").unwrap());

        assert_eq!(d1, d2);
    }

    #[test]
    fn test_hashing_capabilities() {
        // This test ensures DataValue implements Hash correctly.
        // Note: This relies on OrderedFloat implementing Hash manually, as f64 does not support it.
        let mut set = HashSet::new();

        let v1 = DataValue::Int(42);
        let v2 = DataValue::Text("hello".to_string());
        let v3 = DataValue::Float(OrderedFloat(1.5));
        let v4 = DataValue::Float(OrderedFloat(f64::NAN)); // Hash(NaN) check

        set.insert(v1.clone());
        set.insert(v2.clone());
        set.insert(v3.clone());
        set.insert(v4.clone());

        assert!(set.contains(&v1));
        assert!(set.contains(&v2));
        assert!(set.contains(&v3));
        assert!(set.contains(&DataValue::Float(OrderedFloat(f64::NAN))));
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
