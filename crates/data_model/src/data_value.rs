use rust_decimal::Decimal;

use crate::OrderedFloat;

/// Represents a single data value within a Record, capable of holding different types
#[derive(PartialEq, Eq, PartialOrd, Ord)]
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
