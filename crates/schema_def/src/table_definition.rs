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
        if name.trim().is_empty() {
            return Err(SchemaError::InvalidTableName(String::from(
                "Table name cannot be empty or whitespace!",
            )));
        }

        Ok(TableDefinition {
            table_id: 0,
            name,
            columns: [].to_vec(),
            constraints: [].to_vec(),
        })
    }

    pub fn add_column(&mut self, column_def: ColumnDefinition) {
        self.columns.push(column_def);
    }

    pub fn add_constraint(&mut self, constraint: Constraint) {
        self.constraints.push(constraint);
    }

    pub fn get_column(&self, name: &str) -> Option<&ColumnDefinition> {
        self.columns.iter().find(|c| c.name == name)
    }

    pub fn get_constraint(&self, name: &str) -> Option<&Constraint> {
        self.constraints.iter().find(|c| c.name() == name)
    }
}

#[cfg(test)]
mod tests {
    use crate::PrimitiveDataType;

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

    #[test]
    fn test_create_invalid_empty_name() {
        let table_def = TableDefinition::new(String::from(""));
        assert!(table_def.is_err());
    }

    #[test]
    fn test_create_invalid_whitespace_name() {
        let table_def = TableDefinition::new(String::from("   "));
        assert!(table_def.is_err());
    }

    #[test]
    fn test_add_column() {
        let mut table_def = TableDefinition::new(String::from("users")).unwrap();

        let col =
            ColumnDefinition::new(String::from("id"), PrimitiveDataType::Int, false, None).unwrap();
        table_def.add_column(col.clone());

        assert_eq!(table_def.columns.len(), 1);
        assert_eq!(table_def.columns[0], col);

        // Helper method existence check (as implied by C# usage)
        assert!(table_def.get_column("id").is_some());
        assert!(table_def.get_column("missing").is_none());
    }

    #[test]
    fn test_add_constraint() {
        let mut table_def = TableDefinition::new(String::from("users")).unwrap();

        let pk =
            Constraint::primary_key(String::from("pk_users"), vec![String::from("id")]).unwrap();
        table_def.add_constraint(pk.clone());

        assert_eq!(table_def.constraints.len(), 1);
        assert_eq!(table_def.constraints[0], pk);

        // Helper method existence check
        assert!(table_def.get_constraint("pk_users").is_some());
        assert!(table_def.get_constraint("missing").is_none());
    }
}
