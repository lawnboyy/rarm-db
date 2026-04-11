use std::collections::HashMap;

use rarmdb_data_model::{DataValue, Record};
use rarmdb_schema_def::ColumnDefinition;

pub struct RecordSerializer;

/// Serializer for data records for reading and writing to slotted page cells. The format of the data
/// record is | Header | Fixed Length Data | Variable Length Data |
///
/// Example:
///
/// [HEADER]---[FIXED-LENGTH DATA]------------[VARIABLE-LENGTH DATA]--------------------------------------------
/// +----------+-------------------+----------+------------------------+---------------------------------------+
/// | Null     | ID(4 bytes)       | IsActive | Name Length(4 bytes)   | Name Data('Alice' as UTF-8, 5 bytes)  |
/// | Bitmap   | (not null)        | (1 byte) | (not null)             |                                       |
/// | (1 byte) |                   |          |                        |                                       |
/// +----------+-------------------+----------+------------------------+---------------------------------------+
/// | 00001000 | 7B 00 00 00       | 01       | 05 00 00 00            | 41 6C 69 63 65                        |
/// +----------+-------------------+----------+------------------------+---------------------------------------+
/// ^          ^                   ^          ^                        ^
/// |          |                   |          |                        |
/// |          |                   |          +---- Length of 'Alice'  |
/// |          |                   +---- Value for IsActive(true)      |
/// |          +---- Value for ID(123)                                 |
/// +---- 4th column(Bio) is NULL, so 4th bit is 1
impl RecordSerializer {
    /// Serializes a Record into a byte array using the given column definitions.
    /// It is expected that the row column data matches the ordering and types of
    /// the table column definitions.
    pub fn serialize(columns: &[ColumnDefinition], record: &Record) -> Vec<u8> {
        let null_bitmap_size = RecordSerializer::get_null_bitmap_size(columns);
        let mut variable_length_lookup = HashMap::<String, usize>::new();
        let total_record_size: usize = RecordSerializer::calculate_serialized_record_size(
            columns,
            record,
            &mut variable_length_lookup,
        );
        let mut bytes = vec![0u8; total_record_size];

        // The starting offset of the record values will be immediately after null bitmap...
        let mut current_fixed_offset = null_bitmap_size;
        //let mut current_variable_offset =
        for i in 0..columns.len() {
            let col_def = &columns[i];
            if let Some(fixed_size) = col_def.data_type.get_fixed_size() {
                // If the value is null, then update the null bitmap
                if record[i] == DataValue::Null {
                    // Determine which byte the column bit resides in...
                    let null_bitmap_byte_index = i / 8;
                    let bit_in_byte = i % 8;
                    bytes[null_bitmap_byte_index] |= 1 << bit_in_byte;
                    continue;
                }

                // Otherwise, it is a non-null fixed value...
                let current_value = &record[i];
                match *current_value {
                    DataValue::BigInt(val) => {
                        bytes[current_fixed_offset..current_fixed_offset + fixed_size]
                            .copy_from_slice(&val.to_le_bytes());
                    }
                    DataValue::Boolean(val) => {
                        bytes[current_fixed_offset] = val as u8;
                    }
                    DataValue::DateTime(val) => {
                        bytes[current_fixed_offset..current_fixed_offset + fixed_size]
                            .copy_from_slice(&val.to_le_bytes());
                    }
                    DataValue::Decimal(val) => {
                        bytes[current_fixed_offset..current_fixed_offset + fixed_size]
                            .copy_from_slice(&val.serialize());
                    }
                    DataValue::Float(val) => {
                        bytes[current_fixed_offset..current_fixed_offset + fixed_size]
                            .copy_from_slice(&val.0.to_le_bytes());
                    }
                    DataValue::Int(val) => {
                        bytes[current_fixed_offset..current_fixed_offset + fixed_size]
                            .copy_from_slice(&val.to_le_bytes());
                    }
                    _ => continue,
                }
                current_fixed_offset += fixed_size;
            } else {
                // If the value is null, then update the null bitmap
                if record[i] == DataValue::Null {
                    // Determine which byte the column bit resides in...
                    let null_bitmap_byte_index = i / 8;
                    let bit_in_byte = i % 8;
                    bytes[null_bitmap_byte_index] |= 1 << bit_in_byte;
                    continue;
                }
                // Variable length value
                // Otherwise, it is a non-null variable value...
                let current_value = &record[i];
                let variable_length = variable_length_lookup[&col_def.name];
                match &current_value {
                    DataValue::Blob(val) => {
                        let length_offset = current_fixed_offset;
                        let blob_offset = length_offset + size_of::<i32>();
                        // Serialize the length of the data...
                        bytes[length_offset..length_offset + size_of::<i32>()]
                            .copy_from_slice(&variable_length.to_le_bytes());
                        // Serialize the data...
                        bytes[blob_offset..blob_offset + variable_length].copy_from_slice(&val);
                    }
                    DataValue::Text(val) => {
                        let length_offset = current_fixed_offset;
                        let str_offset = length_offset + size_of::<i32>();
                        // Serialize the length of the data...
                        bytes[length_offset..length_offset + size_of::<i32>()]
                            .copy_from_slice(&(variable_length as i32).to_le_bytes());
                        // Serialize the data...
                        bytes[str_offset..str_offset + variable_length]
                            .copy_from_slice(&val.as_bytes());
                    }
                    _ => continue,
                }
                current_fixed_offset += size_of::<i32>() + variable_length;
            }
        }

        bytes
    }

    /// Deserializes a read-only slice of bytes into a Record.
    // pub fn deserialize(columns: &[ColumnDefinition], bytes: &[u8]) -> Record {
    //     todo!("Implement record deserialization")
    // }

    pub fn calculate_serialized_record_size(
        columns: &[ColumnDefinition],
        record: &Record,
        variable_length_lookup: &mut HashMap<String, usize>,
    ) -> usize {
        let null_bitmap_size = RecordSerializer::get_null_bitmap_size(columns);
        let mut total_record_size = null_bitmap_size as usize;

        // Loop through each column and determine the size...
        let col_len = columns.len();
        for i in 0..col_len {
            let row_value = &record[i];
            // If the value is null, no memory will be occupied
            if *row_value == DataValue::Null {
                continue;
            }

            let col_def = &columns[i];
            if let Some(fixed_size) = col_def.data_type.get_fixed_size() {
                total_record_size += fixed_size;
            } else {
                // This is a variable length column, so determine the exact length of the value
                // We'll capture the length of the value to store along with the value...
                total_record_size += size_of::<i32>();

                // Now determine the length of the data.
                match &row_value {
                    DataValue::Blob(val) => {
                        let blob_size = val.len();
                        total_record_size += blob_size;
                        variable_length_lookup.insert(col_def.name.clone(), blob_size);
                    }
                    DataValue::Text(val) => {
                        let string_size = val.as_bytes().len();
                        total_record_size += string_size;
                        variable_length_lookup.insert(col_def.name.clone(), string_size);
                    }
                    _ => {
                        // TODO: We should probably throw an error here...
                        continue;
                    }
                }
            }
        }

        total_record_size
    }

    pub fn get_null_bitmap_size(columns: &[ColumnDefinition]) -> usize {
        (columns.len() + 7) / 8
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rarmdb_data_model::{DataValue, OrderedFloat};
    use rarmdb_schema_def::PrimitiveDataType;

    #[test]
    fn test_serialize_fixed_length_only() {
        // Setup: A schema with exactly 3 fixed-length columns
        let columns = vec![
            ColumnDefinition::new("id".to_string(), PrimitiveDataType::Int, false, None).unwrap(),
            ColumnDefinition::new(
                "is_active".to_string(),
                PrimitiveDataType::Boolean,
                false,
                None,
            )
            .unwrap(),
            ColumnDefinition::new("score".to_string(), PrimitiveDataType::Float, false, None)
                .unwrap(),
        ];

        let record = Record::from(vec![
            DataValue::Int(42),
            DataValue::Boolean(true),
            DataValue::Float(OrderedFloat(99.9)),
        ]);

        // Act
        let bytes = RecordSerializer::serialize(&columns, &record);

        // Assert: Build the exact expected byte array manually
        let mut expected_bytes = vec![0u8]; // 1-byte Null Bitmap (0 = no nulls)
        expected_bytes.extend_from_slice(&42i32.to_le_bytes()); // Int: 4 bytes (Little Endian)
        expected_bytes.extend_from_slice(&[1u8]); // Boolean: 1 byte
        expected_bytes.extend_from_slice(&99.9f64.to_le_bytes()); // Float: 8 bytes (Little Endian)

        assert_eq!(
            expected_bytes, bytes,
            "Serialized byte array does not match expected fixed-length layout!"
        );
    }

    #[test]
    fn test_serialize_fixed_length_with_null() {
        // Setup: A schema with 3 fixed-length columns. The middle column is nullable.
        let columns = vec![
            ColumnDefinition::new("id".to_string(), PrimitiveDataType::Int, false, None).unwrap(),
            ColumnDefinition::new(
                "is_active".to_string(),
                PrimitiveDataType::Boolean,
                true,
                None,
            )
            .unwrap(),
            ColumnDefinition::new("score".to_string(), PrimitiveDataType::Float, false, None)
                .unwrap(),
        ];

        let record = Record::from(vec![
            DataValue::Int(42),
            DataValue::Null, // The middle column is NULL
            DataValue::Float(OrderedFloat(99.9)),
        ]);

        // Act
        let bytes = RecordSerializer::serialize(&columns, &record);

        // Assert: Build the exact expected byte array manually
        // Null Bitmap Logic:
        // Col 0 (Int): Not Null -> Bit 0 = 0
        // Col 1 (Bool): Null -> Bit 1 = 1
        // Col 2 (Float): Not Null -> Bit 2 = 0
        // Binary: 00000010 = 2u8
        let mut expected_bytes = vec![2u8];

        // Data Section:
        expected_bytes.extend_from_slice(&42i32.to_le_bytes()); // Col 0: Int (4 bytes)
        // Col 1 is NULL, so we DO NOT write the 1 byte for the boolean!
        expected_bytes.extend_from_slice(&99.9f64.to_le_bytes()); // Col 2: Float (8 bytes)

        assert_eq!(
            expected_bytes.len(),
            13,
            "Serialized byte array size is incorrect. Did you skip the null column's data space?"
        );

        assert_eq!(
            expected_bytes, bytes,
            "Serialized byte array does not match expected layout for null values!"
        );
    }

    #[test]
    fn test_serialize_variable_length() {
        // Setup: A schema with 1 fixed-length column and 1 variable-length column
        let columns = vec![
            ColumnDefinition::new("id".to_string(), PrimitiveDataType::Int, false, None).unwrap(),
            ColumnDefinition::new(
                "name".to_string(),
                PrimitiveDataType::Varchar(255),
                false,
                None,
            )
            .unwrap(),
        ];

        let record = Record::from(vec![
            DataValue::Int(10),
            DataValue::Text("Alice".to_string()),
        ]);

        // Act
        let bytes = RecordSerializer::serialize(&columns, &record);

        // Assert: Build expected byte array manually based on the two-pass architecture
        let mut expected_bytes = vec![0u8]; // Null Bitmap (no nulls)

        // Pass 1: Fixed-Length Data
        expected_bytes.extend_from_slice(&10i32.to_le_bytes()); // Col 0: Int (4 bytes)

        // Pass 2: Variable-Length Data
        // The C# logic writes the length as an Int32 first...
        expected_bytes.extend_from_slice(&5i32.to_le_bytes());
        // ...followed immediately by the raw UTF-8 bytes
        expected_bytes.extend_from_slice("Alice".as_bytes());

        assert_eq!(
            14,
            expected_bytes.len(),
            "Expected size is 1 (bitmap) + 4 (id) + 4 (string length) + 5 (string data) = 14 bytes"
        );

        assert_eq!(
            expected_bytes, bytes,
            "Serialized byte array does not match expected layout for variable-length data!"
        );
    }
}
