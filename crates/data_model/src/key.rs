use crate::data_value::DataValue;
use serde::{Deserialize, Serialize};
use smallvec::{SmallVec, smallvec};
use std::{
    fmt::{self},
    ops::Deref,
};

// If the key is a single column it will be inlined (stored in the struct instead of on the heap).
// For a composite key with 2 or more columns, we'll store the vector on the heap.
pub type KeyInner = SmallVec<[DataValue; 1]>;

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct Key(KeyInner);

impl Deref for Key {
    type Target = [DataValue];

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl fmt::Display for Key {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "(")?;

        for (i, val) in self.0.iter().enumerate() {
            if i > 0 {
                write!(f, ", ")?;
            }
            write!(f, "{}", val)?;
        }

        write!(f, ")")
    }
}

impl From<DataValue> for Key {
    fn from(value: DataValue) -> Self {
        Key(smallvec![value])
    }
}

impl From<Vec<DataValue>> for Key {
    fn from(values: Vec<DataValue>) -> Self {
        Key(SmallVec::from(values))
    }
}

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
        let k1 = Key(smallvec![DataValue::Int(1), DataValue::Int(1)]);
        let k2 = Key(smallvec![DataValue::Int(1), DataValue::Int(2)]);
        let k3 = Key(smallvec![DataValue::Int(2), DataValue::Int(1)]);

        assert!(k1 < k2);
        assert!(k2 < k3);
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

    #[test]
    fn test_from_conversions() {
        // From single value
        let k1 = Key::from(DataValue::Int(100));
        assert_eq!(k1.len(), 1); // .len() comes from Deref
        assert_eq!(k1[0], DataValue::Int(100)); // Indexing comes from Deref

        // From Vec
        let k2 = Key::from(vec![DataValue::Int(1), DataValue::Int(2)]);
        assert_eq!(k2.len(), 2);
    }

    #[test]
    fn test_len() {
        let k1 = Key::from(vec![DataValue::Int(1)]);
        assert_eq!(k1.len(), 1);

        let k0 = Key::from(Vec::new());
        assert_eq!(k0.len(), 0);
    }

    #[test]
    fn test_is_empty() {
        let k_empty = Key::from(Vec::new());
        assert!(k_empty.is_empty());

        let k_full = Key::from(DataValue::Int(1));
        assert!(!k_full.is_empty());
    }

    #[test]
    fn test_slice_behavior() {
        let v1 = DataValue::Int(10);
        let v2 = DataValue::Text("test".to_string());

        let key = Key::from(vec![v1.clone(), v2.clone()]);

        // Use iter().as_slice() to access the slice via Deref -> Iterator
        let slice = key.iter().as_slice();

        assert_eq!(slice.len(), 2);
        assert_eq!(slice[0], v1);
        assert_eq!(slice[1], v2);
    }

    #[test]
    fn test_key_display() {
        // Single Int
        let k1 = Key::from(DataValue::Int(42));
        assert_eq!(format!("{}", k1), "(42)");

        // Composite Ints
        let k2 = Key::from(vec![DataValue::Int(10), DataValue::Int(20)]);
        assert_eq!(format!("{}", k2), "(10, 20)");

        // Mixed Types with Null and String
        // Assuming DataValue::Text prints without quotes based on current impl
        let k3 = Key::from(vec![
            DataValue::Int(1),
            DataValue::Text("A".to_string()),
            DataValue::Null,
        ]);
        assert_eq!(format!("{}", k3), "(1, A, NULL)");

        // Empty Key
        let k4 = Key::from(Vec::new());
        assert_eq!(format!("{}", k4), "()");
    }

    #[test]
    fn test_complex_composite_key_ordering_mixed() {
        use crate::ordered_float::OrderedFloat;

        // (5, "Abe", 3.533)
        let k1 = Key::from(vec![
            DataValue::Int(5),
            DataValue::Text("Abe".to_string()),
            DataValue::Float(OrderedFloat(3.533)),
        ]);

        // (5, "Bob", 3.533)
        let k2 = Key::from(vec![
            DataValue::Int(5),
            DataValue::Text("Bob".to_string()),
            DataValue::Float(OrderedFloat(3.533)),
        ]);

        // "Abe" < "Bob", so k1 < k2 regardless of the float value
        assert!(k1 < k2, "(5, Abe, ...) should be less than (5, Bob, ...)");
        assert_ne!(k1, k2);
    }
}
