use crate::{PrimitiveDataType, SchemaError};
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct ColumnDefinition {
    pub name: String,
    pub data_type: PrimitiveDataType,
    pub is_nullable: bool,
    pub default_value: Option<String>,
}

impl ColumnDefinition {
    pub fn new(
        name: String,
        data_type: PrimitiveDataType,
        is_nullable: bool,
        default_value: Option<String>,
    ) -> Result<Self, SchemaError> {
        if name.trim().is_empty() {
            return Err(SchemaError::InvalidColumnName);
        }

        if let Some(ref val) = default_value
            && val.trim().is_empty()
        {
            return Err(SchemaError::EmptyDefaultValue);
        }

        Ok(ColumnDefinition {
            name,
            data_type,
            is_nullable,
            default_value,
        })
    }

    pub fn is_fixed_type(&self) -> bool {
        match self.data_type {
            PrimitiveDataType::Blob(_u32) => false,
            PrimitiveDataType::Varchar(_u16) => false,
            _ => true,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::primitive_data_type::PrimitiveDataType;

    #[test]
    fn test_create_valid_column() {
        let col = ColumnDefinition::new(String::from("id"), PrimitiveDataType::Int, false, None);
        assert!(col.is_ok());
        let col = col.unwrap();
        assert_eq!(col.name, "id");
        assert_eq!(col.data_type, PrimitiveDataType::Int);
        assert!(!col.is_nullable);
        assert_eq!(col.default_value, None);
    }

    #[test]
    fn test_create_valid_column_with_default() {
        let col = ColumnDefinition::new(
            String::from("created_at"),
            PrimitiveDataType::DateTime,
            false,
            Some(String::from("NOW()")),
        );
        assert!(col.is_ok());
        let col = col.unwrap();
        assert_eq!(col.name, "created_at");
        assert_eq!(col.default_value, Some(String::from("NOW()")));
    }

    #[test]
    fn test_create_invalid_empty_name() {
        let col = ColumnDefinition::new(String::from(""), PrimitiveDataType::Int, false, None);
        assert!(col.is_err());
        assert!(matches!(col.unwrap_err(), SchemaError::InvalidColumnName));
    }

    #[test]
    fn test_create_invalid_whitespace_name() {
        let col = ColumnDefinition::new(String::from("   "), PrimitiveDataType::Int, false, None);
        assert!(col.is_err());
        assert!(matches!(col.unwrap_err(), SchemaError::InvalidColumnName));
    }

    #[test]
    fn test_create_invalid_empty_default() {
        let col = ColumnDefinition::new(
            String::from("id"),
            PrimitiveDataType::Int,
            false,
            Some(String::from("")),
        );
        assert!(col.is_err());
        assert!(matches!(col.unwrap_err(), SchemaError::EmptyDefaultValue));
    }

    #[test]
    fn test_create_invalid_whitespace_default() {
        let col = ColumnDefinition::new(
            String::from("id"),
            PrimitiveDataType::Int,
            false,
            Some(String::from("   ")),
        );
        assert!(col.is_err());
        assert!(matches!(col.unwrap_err(), SchemaError::EmptyDefaultValue));
    }
}
