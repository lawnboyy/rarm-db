use crate::data_value::DataValue;
use serde::{Deserialize, Serialize};
use smallvec::SmallVec;

// Inline 1 element. If a key has 1 column, it stays on the stack.
// If it has 2+, it moves to the heap.
pub type KeyInner = SmallVec<[DataValue; 1]>;

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct Key(pub KeyInner);

#[cfg(test)]
mod tests {
    use super::*;
    use crate::data_value::DataValue;
    use smallvec::smallvec;
    use std::cmp::Ordering;

    #[test]
    fn test_key_equality() {
        let k1 = Key(smallvec![DataValue::Int(1)]);
        let k2 = Key(smallvec![DataValue::Int(1)]);
        let k3 = Key(smallvec![DataValue::Int(2)]);

        assert_eq!(k1, k2);
        assert_ne!(k1, k3);
    }

    #[test]
    fn test_single_column_ordering() {
        let k1 = Key(smallvec![DataValue::Int(10)]);
        let k2 = Key(smallvec![DataValue::Int(20)]);

        assert!(k1 < k2);
        assert_eq!(k1.cmp(&k2), Ordering::Less);
    }

    #[test]
    fn test_composite_key_ordering() {
        // (1, 1)
        let k1 = Key(smallvec![DataValue::Int(1), DataValue::Int(1)]);
        // (1, 2)
        let k2 = Key(smallvec![DataValue::Int(1), DataValue::Int(2)]);
        // (2, 1)
        let k3 = Key(smallvec![DataValue::Int(2), DataValue::Int(1)]);

        // (1, 1) < (1, 2)
        assert!(
            k1 < k2,
            "Second column should determine order when first is equal"
        );

        // (1, 2) < (2, 1)
        assert!(k2 < k3, "First column should determine order primarily");

        // (1, 1) < (2, 1)
        assert!(k1 < k3);
    }

    #[test]
    fn test_mixed_type_ordering() {
        // Typically keys should be of the same type schema, but Ord must handle mixed types safely.
        // DataValue derives Ord, which sorts based on Enum variant declaration order.
        // Assuming Int comes before Text in DataValue definition.

        let k_int = Key(smallvec![DataValue::Int(1)]);
        let k_text = Key(smallvec![DataValue::Text("1".to_string())]);

        // Just verifying it doesn't panic and is consistent
        assert!(k_int < k_text || k_text < k_int);
    }
}
