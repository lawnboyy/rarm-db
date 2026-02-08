use serde::{Serialize, Deserialize};

use crate::ReferentialAction;

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
    column_names: Vec<String>,
    name: String
  },
  UniqueKey {
    column_names: Vec<String>,
    name: String
  }
}