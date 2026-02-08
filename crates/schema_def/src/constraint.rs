use crate::{ReferentialAction, SchemaError};
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Deserialize, PartialEq, Eq, Serialize)]
pub enum Constraint {
    ForeignKey {
        name: String,
        on_update: ReferentialAction,
        on_delete: ReferentialAction,
        referencing_column_names: Vec<String>,
        referenced_table_name: String,
        referenced_column_names: Vec<String>,
    },
    PrimaryKey {
        name: String,
        column_names: Vec<String>,
    },
    UniqueKey {
        name: String,
        column_names: Vec<String>,
    },
}

impl Constraint {
    /// Creates a new Primary Key constraint.
    pub fn primary_key(name: String, column_names: Vec<String>) -> Result<Self, SchemaError> {
        Self::validate_key(&name, &column_names)?;
        // Return the new primary key constraint.
        Ok(Constraint::PrimaryKey { name, column_names })
    }

    /// Creates a new Unique Key constraint.
    pub fn unique_key(name: String, column_names: Vec<String>) -> Result<Self, SchemaError> {
        Self::validate_key(&name, &column_names)?;
        // Return the new unique key constraint.
        Ok(Constraint::UniqueKey { name, column_names })
    }

    /// Creates a new Foreign Key constraint.
    pub fn foreign_key(
        name: String,
        referencing_column_names: Vec<String>,
        referenced_table_name: String,
        referenced_column_names: Vec<String>,
        on_update: ReferentialAction,
        on_delete: ReferentialAction,
    ) -> Result<Self, SchemaError> {
        // 1. Name not empty
        if name.trim().is_empty() {
            return Err(SchemaError::InvalidForeignKey(
                "Foreign key name cannot be empty!".into(),
            ));
        }

        // 2. Referenced table not empty
        if referenced_table_name.trim().is_empty() {
            return Err(SchemaError::InvalidForeignKey(
                "Foreign key referenced table name cannot be empty!".into(),
            ));
        }

        // 3. Column lists not empty
        if referenced_column_names.is_empty() {
            return Err(SchemaError::InvalidForeignKey(
                "Foreign key referenced column name list cannot be empty!".into(),
            ));
        }

        if referencing_column_names.is_empty() {
            return Err(SchemaError::InvalidForeignKey(
                "Foreign key referencing column name list cannot be empty!".into(),
            ));
        }

        // 4. Column names cannot be empty or whitespace-only.
        Self::validate_columns(&referenced_column_names)?;
        Self::validate_columns(&referencing_column_names)?;

        // 5. Column lists length mismatch
        if referenced_column_names.len() != referencing_column_names.len() {
            return Err(SchemaError::InvalidForeignKey(
                "Foreign key referenced column names count must match referencing column names count!".into(),
            ));
        }

        Ok(Constraint::ForeignKey {
            name,
            on_update,
            on_delete,
            referencing_column_names,
            referenced_table_name,
            referenced_column_names,
        })
    }

    fn validate_key(name: &str, column_names: &[String]) -> Result<(), SchemaError> {
        if name.trim().is_empty() {
            return Err(SchemaError::InvalidKeyName);
        }

        // Validate the column name collection; return an error if any invalid names found (empty strings or all whitespace)
        Self::validate_columns(&column_names)?;
        Ok(())
    }

    /// Private helper to validate a list of column names.
    /// Checks that the list is not empty and does not contain empty/whitespace strings.
    fn validate_columns(column_names: &[String]) -> Result<(), SchemaError> {
        if column_names.is_empty() {
            return Err(SchemaError::InvalidColumnName);
        }

        for col in column_names.iter() {
            if col.trim().is_empty() {
                return Err(SchemaError::InvalidColumnName);
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_primary_key_valid() {
        let pk = Constraint::primary_key("pk_users".to_string(), vec!["id".to_string()]);
        assert!(pk.is_ok());

        if let Constraint::PrimaryKey { name, column_names } = pk.unwrap() {
            assert_eq!(name, "pk_users");
            assert_eq!(column_names, vec!["id"]);
        } else {
            panic!("Expected PrimaryKey variant");
        }
    }

    #[test]
    fn test_primary_key_invalid_empty_name() {
        let pk = Constraint::primary_key("".to_string(), vec!["id".to_string()]);
        assert!(pk.is_err());
        // Verify specific error type if defined, e.g. SchemaError::InvalidFormat
        match pk {
            Err(SchemaError::InvalidKeyName) => {}
            _ => panic!("Expected InvalidScale error"),
        }
    }

    #[test]
    fn test_primary_key_invalid_empty_columns() {
        let pk = Constraint::primary_key("pk_users".to_string(), vec![]);
        assert!(pk.is_err());
    }

    #[test]
    fn test_unique_key_valid() {
        let uq = Constraint::unique_key("uq_email".to_string(), vec!["email".to_string()]);
        assert!(uq.is_ok());

        if let Constraint::UniqueKey { name, column_names } = uq.unwrap() {
            assert_eq!(name, "uq_email");
            assert_eq!(column_names, vec!["email"]);
        } else {
            panic!("Expected UniqueKey variant");
        }
    }

    #[test]
    fn test_foreign_key_valid() {
        let fk = Constraint::foreign_key(
            "fk_user_role".to_string(),
            vec!["role_id".to_string()],
            "roles".to_string(),
            vec!["id".to_string()],
            ReferentialAction::Cascade,
            ReferentialAction::NoAction,
        );
        assert!(fk.is_ok());

        if let Constraint::ForeignKey {
            name,
            referencing_column_names,
            referenced_table_name,
            referenced_column_names,
            on_update,
            on_delete,
        } = fk.unwrap()
        {
            assert_eq!(name, "fk_user_role");
            assert_eq!(referencing_column_names, vec!["role_id"]);
            assert_eq!(referenced_table_name, "roles");
            assert_eq!(referenced_column_names, vec!["id"]);
            assert_eq!(on_update, ReferentialAction::Cascade);
            assert_eq!(on_delete, ReferentialAction::NoAction);
        } else {
            panic!("Expected ForeignKey variant");
        }
    }

    #[test]
    fn test_foreign_key_invalid_mismatched_columns() {
        // 2 referencing columns, 1 referenced column
        let fk = Constraint::foreign_key(
            "fk_bad".to_string(),
            vec!["a".to_string(), "b".to_string()],
            "other".to_string(),
            vec!["id".to_string()],
            ReferentialAction::NoAction,
            ReferentialAction::NoAction,
        );
        assert!(fk.is_err());
    }

    #[test]
    fn test_foreign_key_invalid_empty_referenced_table() {
        let fk = Constraint::foreign_key(
            "fk_bad".to_string(),
            vec!["id".to_string()],
            "".to_string(), // Empty table name
            vec!["id".to_string()],
            ReferentialAction::NoAction,
            ReferentialAction::NoAction,
        );
        assert!(fk.is_err());
    }

    #[test]
    fn test_foreign_key_invalid_empty_columns() {
        let fk = Constraint::foreign_key(
            "fk_bad".to_string(),
            vec![], // Empty columns
            "other".to_string(),
            vec![],
            ReferentialAction::NoAction,
            ReferentialAction::NoAction,
        );
        assert!(fk.is_err());
    }

    #[test]
    fn test_foreign_key_invalid_referencing_columns_content() {
        // Valid list length, but contains an empty string
        let fk = Constraint::foreign_key(
            "fk_bad".to_string(),
            vec!["col1".to_string(), "".to_string()],
            "other".to_string(),
            vec!["colA".to_string(), "colB".to_string()],
            ReferentialAction::NoAction,
            ReferentialAction::NoAction,
        );
        assert!(fk.is_err());
    }

    #[test]
    fn test_foreign_key_invalid_referenced_columns_content() {
        // Valid list length, but contains a whitespace string
        let fk = Constraint::foreign_key(
            "fk_bad".to_string(),
            vec!["col1".to_string(), "col2".to_string()],
            "other".to_string(),
            vec!["colA".to_string(), "   ".to_string()],
            ReferentialAction::NoAction,
            ReferentialAction::NoAction,
        );
        assert!(fk.is_err());
    }
}
