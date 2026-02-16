use crate::DataValue;
use serde::{Deserialize, Serialize};
use smallvec::{SmallVec, smallvec};
use std::{fmt, ops::Deref};

pub type RecordInner = SmallVec<[DataValue; 4]>;

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct Record(RecordInner);

impl Deref for Record {
    type Target = [DataValue];

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl fmt::Display for Record {
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

impl From<DataValue> for Record {
    fn from(value: DataValue) -> Self {
        Record(smallvec![value])
    }
}

impl From<Vec<DataValue>> for Record {
    fn from(values: Vec<DataValue>) -> Self {
        Record(SmallVec::from(values))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::data_value::DataValue;

    #[test]
    fn test_record_creation_and_access() {
        let values = vec![
            DataValue::Int(1),
            DataValue::Text("Alice".to_string()),
            DataValue::Boolean(true),
        ];

        let record = Record::from(values.clone());

        // These methods now come from the Deref trait
        assert_eq!(record.len(), 3);
        assert!(!record.is_empty());

        // Test get() via Deref
        assert_eq!(record.get(0), Some(&values[0]));
        assert_eq!(record.get(1), Some(&values[1]));
        assert_eq!(record.get(2), Some(&values[2]));
        assert_eq!(record.get(3), None);

        // Test direct indexing (also via Deref)
        assert_eq!(record[0], values[0]);
    }

    #[test]
    fn test_record_equality() {
        let r1 = Record::from(vec![DataValue::Int(1), DataValue::Int(2)]);
        let r2 = Record::from(vec![DataValue::Int(1), DataValue::Int(2)]);
        let r3 = Record::from(vec![DataValue::Int(1), DataValue::Int(3)]);

        assert_eq!(r1, r2);
        assert_ne!(r1, r3);
    }

    #[test]
    fn test_small_record_optimization() {
        let small = Record::from(vec![
            DataValue::Int(1),
            DataValue::Int(2),
            DataValue::Int(3),
            DataValue::Int(4),
        ]);
        assert_eq!(small.len(), 4);
    }

    #[test]
    fn test_large_record_spill() {
        let large = Record::from(vec![
            DataValue::Int(1),
            DataValue::Int(2),
            DataValue::Int(3),
            DataValue::Int(4),
            DataValue::Int(5),
        ]);
        assert_eq!(large.len(), 5);
        assert_eq!(large.get(4), Some(&DataValue::Int(5)));
    }

    #[test]
    fn test_record_display() {
        // Single Int
        let r1 = Record::from(DataValue::Int(42));
        assert_eq!(format!("{}", r1), "(42)");

        // Composite
        let r2 = Record::from(vec![
            DataValue::Int(1),
            DataValue::Text("Alice".to_string()),
        ]);
        assert_eq!(format!("{}", r2), "(1, Alice)"); // Text displays without quotes based on DataValue impl

        // Empty
        let r3 = Record::from(Vec::new());
        assert_eq!(format!("{}", r3), "()");
    }
}
