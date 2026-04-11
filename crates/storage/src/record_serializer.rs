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
        let mut fixed_length_size: usize = 0;
        let total_record_size: usize = RecordSerializer::calculate_serialized_record_size(
            columns,
            record,
            &mut variable_length_lookup,
            &mut fixed_length_size,
        );
        let mut bytes = vec![0u8; total_record_size];

        // The starting offset of the record values will be immediately after null bitmap...
        let mut current_fixed_offset = null_bitmap_size;
        // Track a second offset for variable length values...
        let mut current_variable_offset = null_bitmap_size + fixed_length_size;
        for i in 0..columns.len() {
            let col_def = &columns[i];
            // If the value is null, then update the null bitmap
            if record[i] == DataValue::Null {
                // Determine which byte the column bit resides in...
                let null_bitmap_byte_index = i / 8;
                let bit_in_byte = i % 8;
                bytes[null_bitmap_byte_index] |= 1 << bit_in_byte;
                continue;
            }

            if let Some(fixed_size) = col_def.data_type.get_fixed_size() {
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
                // Variable length value
                // Otherwise, it is a non-null variable value...
                let current_value = &record[i];
                let variable_length = variable_length_lookup[&col_def.name];
                match &current_value {
                    DataValue::Blob(val) => {
                        let length_offset = current_variable_offset;
                        let blob_offset = length_offset + size_of::<i32>();
                        // Serialize the length of the data...
                        bytes[length_offset..length_offset + size_of::<i32>()]
                            .copy_from_slice(&(variable_length as i32).to_le_bytes());
                        // Serialize the data...
                        bytes[blob_offset..blob_offset + variable_length].copy_from_slice(&val);
                    }
                    DataValue::Text(val) => {
                        let length_offset = current_variable_offset;
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
                current_variable_offset += size_of::<i32>() + variable_length;
            }
        }

        bytes
    }

    /// Deserializes a read-only slice of bytes into a Record.
    // pub fn deserialize(columns: &[ColumnDefinition], bytes: &[u8]) -> Record {
    //     todo!("Implement record deserialization")
    // }

    fn calculate_serialized_record_size(
        columns: &[ColumnDefinition],
        record: &Record,
        variable_length_lookup: &mut HashMap<String, usize>,
        fixed_length_size: &mut usize,
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
                *fixed_length_size += fixed_size;
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

    #[test]
    fn test_serialize_interleaved_fixed_and_variable() {
        // Setup: A schema where fixed and variable length columns are interleaved
        let columns = vec![
            ColumnDefinition::new("id".to_string(), PrimitiveDataType::Int, false, None).unwrap(),
            ColumnDefinition::new(
                "name".to_string(),
                PrimitiveDataType::Varchar(255),
                false,
                None,
            )
            .unwrap(),
            ColumnDefinition::new(
                "is_active".to_string(),
                PrimitiveDataType::Boolean,
                false,
                None,
            )
            .unwrap(),
        ];

        let record = Record::from(vec![
            DataValue::Int(10),
            DataValue::Text("Alice".to_string()),
            DataValue::Boolean(true),
        ]);

        // Act
        let bytes = RecordSerializer::serialize(&columns, &record);

        // Assert: Build expected byte array manually based on the two-pass architecture
        let mut expected_bytes = vec![0u8]; // Null Bitmap (no nulls)

        // Pass 1: Fixed-Length Data MUST be grouped together first!
        expected_bytes.extend_from_slice(&10i32.to_le_bytes()); // Col 0: Int (4 bytes)
        expected_bytes.extend_from_slice(&[1u8]); // Col 2: Boolean (1 byte)

        // Pass 2: Variable-Length Data MUST come after all fixed-length data!
        expected_bytes.extend_from_slice(&5i32.to_le_bytes()); // Col 1: Varchar length (4 bytes)
        expected_bytes.extend_from_slice("Alice".as_bytes()); // Col 1: Varchar data (5 bytes)

        assert_eq!(
            15,
            expected_bytes.len(),
            "Expected size is 1 (bitmap) + 4 (id) + 1 (is_active) + 4 (string length) + 5 (string data) = 15 bytes"
        );

        assert_eq!(
            expected_bytes, bytes,
            "Serialized byte array did not group fixed and variable length data correctly!"
        );
    }

    #[test]
    fn test_serialize_all_data_types() {
        use rust_decimal::Decimal;

        // Setup: A schema with every supported PrimitiveDataType
        let columns = vec![
            ColumnDefinition::new("col_int".to_string(), PrimitiveDataType::Int, false, None)
                .unwrap(),
            ColumnDefinition::new(
                "col_bigint".to_string(),
                PrimitiveDataType::BigInt,
                false,
                None,
            )
            .unwrap(),
            ColumnDefinition::new(
                "col_bool".to_string(),
                PrimitiveDataType::Boolean,
                false,
                None,
            )
            .unwrap(),
            ColumnDefinition::new(
                "col_float".to_string(),
                PrimitiveDataType::Float,
                false,
                None,
            )
            .unwrap(),
            ColumnDefinition::new(
                "col_decimal".to_string(),
                PrimitiveDataType::Decimal(10, 2),
                false,
                None,
            )
            .unwrap(),
            ColumnDefinition::new(
                "col_datetime".to_string(),
                PrimitiveDataType::DateTime,
                false,
                None,
            )
            .unwrap(),
            ColumnDefinition::new(
                "col_varchar".to_string(),
                PrimitiveDataType::Varchar(255),
                false,
                None,
            )
            .unwrap(),
            ColumnDefinition::new(
                "col_blob".to_string(),
                PrimitiveDataType::Blob(255),
                false,
                None,
            )
            .unwrap(),
        ];

        let decimal_val = Decimal::new(1234, 2); // 12.34
        let datetime_ticks = 1672531200000i64; // Arbitrary timestamp/ticks
        let blob_data = vec![0xDE, 0xAD, 0xBE, 0xEF];

        let record = Record::from(vec![
            DataValue::Int(42),
            DataValue::BigInt(1234567890),
            DataValue::Boolean(true),
            DataValue::Float(OrderedFloat(3.14159)),
            DataValue::Decimal(decimal_val),
            DataValue::DateTime(datetime_ticks), // Assuming DateTime stores an i64 internally
            DataValue::Text("Alice".to_string()),
            DataValue::Blob(blob_data.clone()),
        ]);

        // Act
        let bytes = RecordSerializer::serialize(&columns, &record);

        // Assert: Build expected byte array manually
        let mut expected_bytes = vec![0u8]; // Null Bitmap (1 byte covers up to 8 columns)

        // --- Pass 1: Fixed-Length Data ---
        expected_bytes.extend_from_slice(&42i32.to_le_bytes()); // Int (4 bytes)
        expected_bytes.extend_from_slice(&1234567890i64.to_le_bytes()); // BigInt (8 bytes)
        expected_bytes.extend_from_slice(&[1u8]); // Boolean (1 byte)
        expected_bytes.extend_from_slice(&3.14159f64.to_le_bytes()); // Float (8 bytes)
        expected_bytes.extend_from_slice(&decimal_val.serialize()); // Decimal (16 bytes)
        expected_bytes.extend_from_slice(&datetime_ticks.to_le_bytes()); // DateTime (8 bytes)

        // --- Pass 2: Variable-Length Data ---
        // Varchar: Length + Data
        expected_bytes.extend_from_slice(&5i32.to_le_bytes()); // String Length (4 bytes)
        expected_bytes.extend_from_slice("Alice".as_bytes()); // String Data (5 bytes)

        // Blob: Length + Data
        expected_bytes.extend_from_slice(&(blob_data.len() as i32).to_le_bytes()); // Blob Length (4 bytes)
        expected_bytes.extend_from_slice(&blob_data); // Blob Data (4 bytes)

        assert_eq!(
            63,
            expected_bytes.len(),
            "Expected size mismatch. 1 (bitmap) + 45 (fixed) + 17 (variable) = 63 bytes"
        );

        assert_eq!(
            expected_bytes, bytes,
            "Serialized byte array failed the 'All Types' gauntlet!"
        );
    }

    #[test]
    fn test_serialize_bitmap_overflow_and_empty_strings() {
        // Setup: 9 columns to force a 2-byte null bitmap
        let mut columns = Vec::new();
        for i in 0..8 {
            columns.push(
                ColumnDefinition::new(format!("col_{}", i), PrimitiveDataType::Int, true, None)
                    .unwrap(),
            );
        }
        // The 9th column
        columns.push(
            ColumnDefinition::new(
                "last_col".to_string(),
                PrimitiveDataType::Varchar(255),
                true,
                None,
            )
            .unwrap(),
        );

        // Record: 1st col is Null, 9th col is an EMPTY string (not null)
        let mut values = vec![DataValue::Null];
        for _ in 1..8 {
            values.push(DataValue::Int(1));
        }
        values.push(DataValue::Text("".to_string()));

        let record = Record::from(values);

        // Act
        let bytes = RecordSerializer::serialize(&columns, &record);

        // Assert
        // Bitmap:
        // Byte 0: 00000001 (Col 0 is null) -> 1u8
        // Byte 1: 00000000 (Col 8 is NOT null, it's an empty string) -> 0u8
        assert_eq!(bytes[0], 1);
        assert_eq!(bytes[1], 0);

        // Pass 2 (Variable): The empty string should still write its 4-byte length (0)
        // Offset: 2 (bitmap) + 28 (7 ints) = 30.
        let length_bytes: [u8; 4] = bytes[30..34].try_into().unwrap();
        assert_eq!(i32::from_le_bytes(length_bytes), 0);
        assert_eq!(bytes.len(), 34);
    }

    #[test]
    fn test_serialize_complex_utf8() {
        let columns = vec![
            ColumnDefinition::new(
                "emoji".to_string(),
                PrimitiveDataType::Varchar(255),
                false,
                None,
            )
            .unwrap(),
        ];

        // The Crab emoji is 4 bytes in UTF-8: [240, 159, 166, 128]
        let record = Record::from(vec![DataValue::Text("🦀".to_string())]);

        let bytes = RecordSerializer::serialize(&columns, &record);

        // 1 (bitmap) + 4 (length) + 4 (data) = 9 bytes
        assert_eq!(bytes.len(), 9);
        assert_eq!(i32::from_le_bytes(bytes[1..5].try_into().unwrap()), 4);
        assert_eq!(&bytes[5..9], "🦀".as_bytes());
    }
}
