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
        let total_record_size: usize =
            RecordSerializer::calculate_serialized_record_size(columns, record);
        let mut bytes = vec![0u8; total_record_size];

        // The starting offset of the record values will be immediately after null bitmap...
        let mut current_offset = null_bitmap_size;
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
                        bytes[current_offset..current_offset + fixed_size]
                            .copy_from_slice(&val.to_le_bytes());
                    }
                    DataValue::Boolean(val) => {
                        bytes[current_offset] = val as u8;
                    }
                    DataValue::DateTime(val) => {
                        bytes[current_offset..current_offset + fixed_size]
                            .copy_from_slice(&val.to_le_bytes());
                    }
                    DataValue::Decimal(val) => {
                        bytes[current_offset..current_offset + fixed_size]
                            .copy_from_slice(&val.serialize());
                    }
                    DataValue::Float(val) => {
                        bytes[current_offset..current_offset + fixed_size]
                            .copy_from_slice(&val.0.to_le_bytes());
                    }
                    DataValue::Int(val) => {
                        bytes[current_offset..current_offset + fixed_size]
                            .copy_from_slice(&val.to_le_bytes());
                    }
                    _ => continue,
                }
                current_offset += fixed_size;
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
            }
            // TODO: Calculate variable size
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
}
