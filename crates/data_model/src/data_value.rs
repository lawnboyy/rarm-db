use rust_decimal::Decimal;

use crate::OrderedFloat;

/// Represents a single data value within a Record, capable of holding different types
#[derive(Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum DataValue {
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
}
