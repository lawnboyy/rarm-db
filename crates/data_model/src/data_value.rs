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

impl DataValue {}

#[cfg(test)]
mod tests {
    use std::{collections::HashSet, str::FromStr};

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
}
