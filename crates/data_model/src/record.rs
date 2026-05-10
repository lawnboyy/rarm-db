use crate::{DataValue, Key};
use rarmdb_schema_def::{TableDefinition, constraint::Constraint};
use serde::{Deserialize, Serialize};
use smallvec::{SmallVec, smallvec};
use std::{collections::HashMap, fmt, ops::Deref};

pub type RecordInner = SmallVec<[DataValue; 4]>;

/// In memory representation of a database record. A record is essentially a collection of
/// data values that corresponds to a schema. In the case of data rows, the schema associated
/// with a record is the table definition. In the case of an internal node record containing
/// separator key / child pointer pairs, the schema is composed of the key (could be primary
/// or a non-primary index key) and special column definitions that represent the child
/// pointer.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct Record(RecordInner);

impl Record {
    pub fn get_primary_key(&self, table_def: &TableDefinition) -> Key {
        // TODO: Should we handle the lack of a primary key more gracefully?
        let constraint = table_def.get_primary_key().unwrap();

        let pk_cols_option = match constraint {
            Constraint::PrimaryKey { column_names, .. } => Some(column_names),
            _ => None,
        };

        if let Some(pk_cols) = pk_cols_option {
            let mut key_values = Vec::with_capacity(pk_cols.len());
            // Create a reverse index lookup to key value order matches primary key definition column order...
            let pk_reverse_lookup: HashMap<&String, usize> =
                pk_cols.iter().enumerate().map(|(i, c)| (c, i)).collect();

            // Loop through all column definitions and extract any primary key column value and place it at the
            // appropriate index in the key_values vector.
            for i in 0..table_def.columns.len() {
                // If this is a primary key column...
                let current_col_name: &String = &table_def.columns[i].name;
                if pk_cols.contains(current_col_name) {
                    // Add the primary key column value to the proper index in our primary key values vector...
                    let key_col_index = pk_reverse_lookup[current_col_name];
                    key_values.insert(key_col_index, self[i].clone());
                }
            }

            return Key::from(key_values);
        }

        // TODO: Handle missing primary key...
        Key::from(Vec::new())
    }
}

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
