use serde::{Deserialize, Serialize};

use crate::{ColumnDefinition, SchemaError, constraint::Constraint};

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct TableDefinition {
    pub table_id: u32,
    pub name: String,
    pub columns: Vec<ColumnDefinition>,
    pub constraints: Vec<Constraint>,
}

impl TableDefinition {
    pub fn new(name: String) -> Result<Self, SchemaError> {
        Ok(TableDefinition {
            table_id: 0,
            name,
            columns: [].to_vec(),
            constraints: [].to_vec(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_create_valid_table_definition() {
        let table_def = TableDefinition::new(String::from("users"));
        assert!(table_def.is_ok());

        let table_def = table_def.unwrap();
        assert_eq!(table_def.name, "users");
        // Verify defaults (empty lists)
        assert!(table_def.columns.is_empty());
        assert!(table_def.constraints.is_empty());
    }
}
