use rarmdb_data_model::Record;
use rarmdb_schema_def::ColumnDefinition;

pub struct RecordSerializer;

impl RecordSerializer {
    /// Serializes a Record into a byte array using the given column definitions.
    /// It is expected that the row column data matches the ordering and types of
    /// the table column definitions.
    pub fn serialize(columns: &[ColumnDefinition], record: &Record) -> Vec<u8> {
        todo!("Implement record serialization")
    }

    /// Deserializes a read-only slice of bytes into a Record.
    pub fn deserialize(columns: &[ColumnDefinition], bytes: &[u8]) -> Record {
        todo!("Implement record deserialization")
    }
}

// #[cfg(test)]
// mod tests {
//     use super::*;
//     use rarmdb_data_model::{DataValue, OrderedFloat};
//     use rarmdb_schema_def::PrimitiveDataType;

//     #[test]
//     fn test_serialize_deserialize_fixed_length_only() {
//         // Setup: A schema with exactly 3 fixed-length columns
//         let columns = vec![
//             ColumnDefinition::new("id".to_string(), PrimitiveDataType::Int, false, None).unwrap(),
//             ColumnDefinition::new(
//                 "is_active".to_string(),
//                 PrimitiveDataType::Boolean,
//                 false,
//                 None,
//             )
//             .unwrap(),
//             ColumnDefinition::new("score".to_string(), PrimitiveDataType::Float, false, None)
//                 .unwrap(),
//         ];

//         let original_record = Record::from(vec![
//             DataValue::Int(42),
//             DataValue::Boolean(true),
//             DataValue::Float(OrderedFloat(99.9)),
//         ]);

//         // Act 1: Serialize
//         let bytes = RecordSerializer::serialize(&columns, &original_record);

//         // Assert 1: Verify the tight packing size
//         // Expected size:
//         // 1 byte (Null Bitmap for up to 8 cols) + 4 bytes (Int) + 1 byte (Bool) + 8 bytes (Float) = 14 bytes
//         assert_eq!(
//             14,
//             bytes.len(),
//             "Serialized byte array size is incorrect. Did you calculate the null bitmap size and fixed offsets properly?"
//         );

//         // Act 2: Deserialize
//         let deserialized_record = RecordSerializer::deserialize(&columns, &bytes);

//         // Assert 2: Ensure data fidelity is maintained through the round-trip
//         assert_eq!(original_record, deserialized_record);
//     }
// }
