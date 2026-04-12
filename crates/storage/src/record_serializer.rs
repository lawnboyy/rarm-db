use rarmdb_data_model::{DataValue, Key, Record};
use rarmdb_schema_def::{ColumnDefinition, PrimitiveDataType, TableDefinition};

use crate::SerializationError;

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
    pub fn serialize(
        columns: &[ColumnDefinition],
        record: &Record,
    ) -> Result<Vec<u8>, SerializationError> {
        let null_bitmap_size = RecordSerializer::get_null_bitmap_size(columns);
        let mut variable_length_sizes = Vec::<usize>::new();
        let mut fixed_length_size: usize = 0;
        let total_record_size: usize = RecordSerializer::calculate_serialized_record_size(
            columns,
            record,
            &mut variable_length_sizes,
            &mut fixed_length_size,
        );
        let mut variable_length_col_index = 0;
        let mut bytes = vec![0u8; total_record_size];

        // The starting offset of the record values will be immediately after null bitmap...
        let mut current_fixed_offset = null_bitmap_size;
        // Track a second offset for variable length values...
        let mut current_variable_offset = null_bitmap_size + fixed_length_size;
        for (i, (col_def, current_value)) in columns.iter().zip(record.iter()).enumerate() {
            // If the value is null, then update the null bitmap
            if *current_value == DataValue::Null {
                // Determine which byte the column bit resides in...
                let null_bitmap_byte_index = i / 8;
                let bit_in_byte = i % 8;
                bytes[null_bitmap_byte_index] |= 1 << bit_in_byte;
                continue;
            }

            if let Some(fixed_size) = col_def.data_type.get_fixed_size() {
                // Otherwise, it is a non-null fixed value...
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
                    _ => return Err(SerializationError::DataTypeMismatch),
                }
                current_fixed_offset += fixed_size;
            } else {
                // Variable length value
                // Otherwise, it is a non-null variable value...
                let variable_length = variable_length_sizes[variable_length_col_index];
                variable_length_col_index += 1;
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
                    _ => return Err(SerializationError::DataTypeMismatch),
                }
                current_variable_offset += size_of::<i32>() + variable_length;
            }
        }

        Ok(bytes)
    }

    /// Deserializes a read-only slice of bytes into a Record.
    pub fn deserialize(
        columns: &[ColumnDefinition],
        bytes: &[u8],
    ) -> Result<Record, SerializationError> {
        let col_count = columns.len();
        let mut data_values = vec![DataValue::Null; col_count];
        let null_bitmap_size = RecordSerializer::get_null_bitmap_size(columns);
        let null_bitmap = bytes[0..null_bitmap_size].iter().as_slice();

        // Track the current fixed sized data offset
        let mut current_fixed_size_offset = null_bitmap_size;
        // Track the current variable sized data offset
        let mut current_variable_size_offset = RecordSerializer::get_variable_length_data_offset(
            columns,
            null_bitmap,
            null_bitmap_size,
        );

        for (i, col_def) in columns.iter().enumerate() {
            if RecordSerializer::is_col_value_null(null_bitmap, i) {
                // Reject a null value for a non-nullable column.
                if !col_def.is_nullable {
                    return Err(SerializationError::NullValueForNonNullColumnFound);
                }

                data_values[i] = DataValue::Null;
                continue;
            }

            data_values[i] = RecordSerializer::deserialize_col_value(
                col_def,
                bytes,
                &mut current_fixed_size_offset,
                &mut current_variable_size_offset,
            );
        }

        Ok(Record::from(data_values))
    }

    pub fn deserialize_primary_key(
        table_def: &TableDefinition,
        record_bytes: &[u8],
    ) -> Result<Key, SerializationError> {
        let result: Option<Vec<&ColumnDefinition>> = table_def.get_primary_key_columns();
        if let Some(key_cols) = result {
            let key_array = key_cols.as_slice();

            return Ok(RecordSerializer::deserialize_key(
                &table_def.columns,
                key_array,
                record_bytes,
            )
            .unwrap());
        }

        Err(SerializationError::PrimaryKeyNotFound)
    }

    pub fn deserialize_key(
        record_cols: &[ColumnDefinition],
        key_cols: &[&ColumnDefinition],
        record_bytes: &[u8],
    ) -> Result<Key, SerializationError> {
        let key_col_count = key_cols.len();
        let col_count = record_cols.len();
        let mut row_values = vec![DataValue::Null; key_col_count];

        let null_bitmap_size = (col_count + 7) / 8;
        let null_bitmap = &record_bytes[..null_bitmap_size];

        let mut current_fixed_offset = null_bitmap_size;
        let mut current_variable_offset = RecordSerializer::get_variable_length_data_offset(
            record_cols,
            null_bitmap,
            null_bitmap_size,
        );
        let mut key_cols_found = 0;
        for (i, col_def) in record_cols.iter().enumerate() {
            // Early out if we've found all the key columns...
            if key_cols_found >= key_col_count {
                break;
            }

            let is_key_col = key_cols
                .iter()
                .map(|col| &col.name)
                .any(|n| *n == col_def.name);

            let mut key_col_index = 0;
            if is_key_col {
                key_col_index = key_cols
                    .iter()
                    .position(|c| c.name == col_def.name)
                    .unwrap();
                key_cols_found += 1;
            }

            if RecordSerializer::is_col_value_null(null_bitmap, i) {
                if is_key_col {
                    return Err(SerializationError::NullPrimaryKeyColumnValue);
                }
                continue;
            }

            if col_def.is_fixed_type() {
                let data_size = col_def.data_type.get_fixed_size().unwrap();
                if is_key_col {
                    row_values[key_col_index] =
                        RecordSerializer::deserialize_fixed_sized_col_value(
                            col_def,
                            record_bytes,
                            &current_fixed_offset,
                            data_size,
                        )
                        .expect("msg");
                }

                current_fixed_offset += data_size;
            } else {
                let variable_data_bytes = &record_bytes[current_variable_offset..];
                let (data_size_bytes, rest) = variable_data_bytes.split_at(size_of::<i32>());
                let data_size = i32::from_le_bytes(data_size_bytes.try_into().unwrap());
                current_variable_offset += size_of::<i32>();
                if is_key_col {
                    row_values[key_col_index] =
                        RecordSerializer::deserialize_variable_sized_col_value(
                            col_def, rest, data_size,
                        )
                        .unwrap();
                }
                current_variable_offset += data_size as usize;
            }
        }

        Ok(Key::from(row_values))
    }

    fn deserialize_col_value(
        col_def: &ColumnDefinition,
        bytes: &[u8],
        current_fixed_sized_data_offset: &mut usize,
        current_variable_sized_data_offset: &mut usize,
    ) -> DataValue {
        if col_def.data_type.is_fixed_size() {
            let data_size = col_def.data_type.get_fixed_size().unwrap();
            let row_value = RecordSerializer::deserialize_fixed_sized_col_value(
                col_def,
                bytes,
                current_fixed_sized_data_offset,
                data_size,
            );

            *current_fixed_sized_data_offset += data_size;

            // TODO: Gracefully handle errors...
            // if let Err(e) = row_value {
            //     return e;
            // }

            return row_value.unwrap();
        } else {
            // Calculate the ending offset of the variable column data as 4 bytes for the length of the data plus the length of the data.
            let variable_data_bytes = &bytes[*current_fixed_sized_data_offset..];
            let (data_size_bytes, rest) = variable_data_bytes.split_at(size_of::<i32>());
            let data_size = i32::from_le_bytes(data_size_bytes.try_into().unwrap());
            *current_variable_sized_data_offset += size_of::<i32>();
            let row_value =
                RecordSerializer::deserialize_variable_sized_col_value(col_def, rest, data_size);
            *current_variable_sized_data_offset += data_size as usize;

            return row_value.unwrap();
        }
    }

    fn deserialize_fixed_sized_col_value(
        col_def: &ColumnDefinition,
        bytes: &[u8],
        current_fixed_sized_data_offset: &usize,
        data_size: usize,
    ) -> Result<DataValue, SerializationError> {
        let data_value =
            &bytes[*current_fixed_sized_data_offset..*current_fixed_sized_data_offset + data_size];
        match col_def.data_type {
            PrimitiveDataType::BigInt => {
                let val = i64::from_le_bytes(data_value.try_into().unwrap());
                Ok(DataValue::BigInt(val))
            }
            PrimitiveDataType::Boolean => {
                let val = data_value[0];
                Ok(DataValue::Boolean(val != 0))
            }
            PrimitiveDataType::DateTime => {
                let val = i64::from_le_bytes(data_value.try_into().unwrap());
                Ok(DataValue::DateTime(val))
            }
            PrimitiveDataType::Decimal(_, _) => {
                let arr: [u8; 16] = data_value
                    .try_into()
                    .map_err(|_| SerializationError::DataTypeMismatch)?;
                let val = rust_decimal::Decimal::deserialize(arr);
                Ok(DataValue::Decimal(val))
            }
            PrimitiveDataType::Float => {
                let val = f64::from_le_bytes(data_value.try_into().unwrap());
                Ok(DataValue::Float(rarmdb_data_model::OrderedFloat(val)))
            }
            PrimitiveDataType::Int => {
                let val = i32::from_le_bytes(data_value.try_into().unwrap());
                Ok(DataValue::Int(val))
            }
            _ => Err(SerializationError::DataTypeMismatch),
        }
    }

    fn deserialize_variable_sized_col_value(
        col_def: &ColumnDefinition,
        bytes: &[u8],
        data_size: i32,
    ) -> Result<DataValue, SerializationError> {
        // Calculate the ending offset of the variable column data as 4 bytes for the length of the data plus the length of the data.
        let end_offset = data_size as usize;

        // With the start and end offsets of the data, get the data slice to deserialize.
        let data_bytes = &bytes[..end_offset];

        match col_def.data_type {
            PrimitiveDataType::Blob(_) => Ok(DataValue::Blob(data_bytes.to_vec())),
            PrimitiveDataType::Varchar(_) => Ok(DataValue::Text(
                String::from_utf8(data_bytes.to_vec()).unwrap(),
            )),
            _ => Err(SerializationError::DataTypeMismatch),
        }
    }

    fn calculate_serialized_record_size(
        columns: &[ColumnDefinition],
        record: &Record,
        variable_length_sizes: &mut Vec<usize>,
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
                        variable_length_sizes.push(blob_size);
                    }
                    DataValue::Text(val) => {
                        let string_size = val.as_bytes().len();
                        total_record_size += string_size;
                        variable_length_sizes.push(string_size);
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

    fn get_variable_length_data_offset(
        columns: &[ColumnDefinition],
        null_bitmap: &[u8],
        null_bitmap_size: usize,
    ) -> usize {
        // let col_count = columns.len();
        let mut offset = null_bitmap_size;

        for (i, col_def) in columns.iter().enumerate() {
            if RecordSerializer::is_col_value_null(null_bitmap, i) {
                continue;
            }

            if col_def.is_fixed_type() {
                offset += col_def.data_type.get_fixed_size().unwrap();
            }
        }

        offset
    }

    fn is_col_value_null(null_bitmap: &[u8], index: usize) -> bool {
        let null_bitmap_byte_index = index / 8;
        let null_bitmap_byte = null_bitmap[null_bitmap_byte_index];
        let bit_in_byte = index % 8;
        return (null_bitmap_byte & (1 << bit_in_byte)) != 0;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rarmdb_data_model::{DataValue, OrderedFloat};
    use rarmdb_schema_def::{PrimitiveDataType, TableDefinition, constraint::Constraint};

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
        let bytes = RecordSerializer::serialize(&columns, &record).unwrap();

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
        let bytes = RecordSerializer::serialize(&columns, &record).unwrap();

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
        let bytes = RecordSerializer::serialize(&columns, &record).unwrap();

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
        let bytes = RecordSerializer::serialize(&columns, &record).unwrap();

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
        let bytes = RecordSerializer::serialize(&columns, &record).unwrap();

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
        let bytes = RecordSerializer::serialize(&columns, &record).unwrap();

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

        let bytes = RecordSerializer::serialize(&columns, &record).unwrap();

        // 1 (bitmap) + 4 (length) + 4 (data) = 9 bytes
        assert_eq!(bytes.len(), 9);
        assert_eq!(i32::from_le_bytes(bytes[1..5].try_into().unwrap()), 4);
        assert_eq!(&bytes[5..9], "🦀".as_bytes());
    }

    #[test]
    fn test_serialize_all_null_record() {
        let mut columns = Vec::new();
        for i in 0..8 {
            columns.push(
                ColumnDefinition::new(format!("col_{}", i), PrimitiveDataType::Int, true, None)
                    .unwrap(),
            );
        }
        let record = Record::from(vec![DataValue::Null; 8]);
        let bytes = RecordSerializer::serialize(&columns, &record).unwrap();

        // Assert: 1-byte bitmap with every bit set (255) and no data
        assert_eq!(bytes.len(), 1);
        assert_eq!(bytes[0], 255);
    }

    #[test]
    fn test_serialize_large_blob_boundary() {
        let columns = vec![
            ColumnDefinition::new(
                "large_data".to_string(),
                PrimitiveDataType::Blob(1024 * 1024),
                false,
                None,
            )
            .unwrap(),
        ];
        // 64 KB of data
        let size = 65536;
        let data: Vec<u8> = (0..size).map(|i| (i % 256) as u8).collect();
        let record = Record::from(vec![DataValue::Blob(data.clone())]);

        let bytes = RecordSerializer::serialize(&columns, &record).unwrap();

        // Assert: 1 (bitmap) + 4 (length prefix) + 65536 (data)
        assert_eq!(bytes.len(), 1 + 4 + size);
        let length_bytes: [u8; 4] = bytes[1..5].try_into().unwrap();
        assert_eq!(i32::from_le_bytes(length_bytes), size as i32);
        assert_eq!(&bytes[5..], &data[..]);
    }

    #[test]
    fn test_deserialize_fixed_length_only() {
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

        // Manually build the bytes as they should appear on disk
        let mut bytes = vec![0u8]; // 1-byte Null Bitmap (0 = no nulls)
        bytes.extend_from_slice(&42i32.to_le_bytes()); // Col 0: 42
        bytes.extend_from_slice(&[1u8]); // Col 1: true
        bytes.extend_from_slice(&99.9f64.to_le_bytes()); // Col 2: 99.9

        // Act
        let record = RecordSerializer::deserialize(&columns, &bytes)
            .expect("Deserialization should succeed");

        // Assert
        assert_eq!(3, record.len());
        assert_eq!(DataValue::Int(42), record[0]);
        assert_eq!(DataValue::Boolean(true), record[1]);
        assert_eq!(DataValue::Float(OrderedFloat(99.9)), record[2]);
    }

    #[test]
    fn test_deserialize_fixed_length_with_null() {
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

        // Manually build the bytes as they should appear on disk
        // Null Bitmap: Col 0 (not null), Col 1 (null), Col 2 (not null) -> Binary 00000010 = 2u8
        let mut bytes = vec![2u8];
        bytes.extend_from_slice(&42i32.to_le_bytes()); // Col 0: 42
        // Col 1 is NULL, so skip data bytes
        bytes.extend_from_slice(&99.9f64.to_le_bytes()); // Col 2: 99.9

        // Act
        let record = RecordSerializer::deserialize(&columns, &bytes)
            .expect("Deserialization should succeed");

        // Assert
        assert_eq!(3, record.len());
        assert_eq!(DataValue::Int(42), record[0]);
        assert_eq!(DataValue::Null, record[1]);
        assert_eq!(DataValue::Float(OrderedFloat(99.9)), record[2]);
    }

    #[test]
    fn test_deserialize_variable_length() {
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

        // Manually build the bytes based on the two-pass architecture
        let mut bytes = vec![0u8]; // Null Bitmap (no nulls)
        bytes.extend_from_slice(&10i32.to_le_bytes()); // Pass 1: Fixed-Length (ID=10)
        bytes.extend_from_slice(&5i32.to_le_bytes()); // Pass 2: Variable-Length (Length=5)
        bytes.extend_from_slice("Alice".as_bytes()); // Pass 2: Variable-Length (Data)

        // Act
        let record = RecordSerializer::deserialize(&columns, &bytes)
            .expect("Deserialization should succeed");

        // Assert
        assert_eq!(2, record.len());
        assert_eq!(DataValue::Int(10), record[0]);
        assert_eq!(DataValue::Text("Alice".to_string()), record[1]);
    }

    #[test]
    fn test_deserialize_interleaved_and_nulls() {
        // Setup: A complex schema with interleaved fixed/variable and nulls
        // Col 0: id (Fixed, Not Null)
        // Col 1: name (Var, Nullable)
        // Col 2: is_active (Fixed, Not Null)
        // Col 3: bio (Var, Nullable)
        let columns = vec![
            ColumnDefinition::new("id".to_string(), PrimitiveDataType::Int, false, None).unwrap(),
            ColumnDefinition::new(
                "name".to_string(),
                PrimitiveDataType::Varchar(255),
                true,
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
            ColumnDefinition::new("bio".to_string(), PrimitiveDataType::Blob(1024), true, None)
                .unwrap(),
        ];

        // Data:
        // Col 0: 100
        // Col 1: NULL
        // Col 2: true
        // Col 3: [0xDE, 0xAD] (length 2)

        // Null Bitmap:
        // Col 0: 0, Col 1: 1 (NULL), Col 2: 0, Col 3: 0 -> Binary 00000010 = 2u8
        let mut bytes = vec![2u8];

        // Pass 1: Fixed Section (Only Col 0 and Col 2 have data)
        bytes.extend_from_slice(&100i32.to_le_bytes()); // Col 0
        bytes.extend_from_slice(&[1u8]); // Col 2

        // Pass 2: Variable Section (Only Col 3 has data because Col 1 is NULL)
        bytes.extend_from_slice(&2i32.to_le_bytes()); // Col 3 Length
        bytes.extend_from_slice(&[0xDE, 0xAD]); // Col 3 Data

        // Act
        let record = RecordSerializer::deserialize(&columns, &bytes)
            .expect("Complex deserialization should succeed");

        // Assert
        assert_eq!(4, record.len());
        assert_eq!(DataValue::Int(100), record[0]);
        assert_eq!(DataValue::Null, record[1]);
        assert_eq!(DataValue::Boolean(true), record[2]);
        assert_eq!(DataValue::Blob(vec![0xDE, 0xAD]), record[3]);
    }

    #[test]
    fn test_deserialize_bitmap_overflow() {
        // Setup: 10 columns to force a 2-byte null bitmap
        let mut columns = Vec::new();
        for i in 0..10 {
            columns.push(
                ColumnDefinition::new(format!("col_{}", i), PrimitiveDataType::Int, true, None)
                    .unwrap(),
            );
        }

        // Bitmap logic:
        // Byte 0: Col 0 is NULL (bit 0 = 1), Byte 1: Col 9 is NULL (bit 1 = 1)
        // Byte 0 binary: 00000001 = 1
        // Byte 1 binary: 00000010 = 2
        let mut bytes = vec![1u8, 2u8];

        // 8 non-null Ints (Col 1 through Col 8)
        for i in 1..9 {
            bytes.extend_from_slice(&(i as i32).to_le_bytes());
        }

        // Act
        let record = RecordSerializer::deserialize(&columns, &bytes)
            .expect("Should handle multi-byte bitmap");

        // Assert
        assert_eq!(10, record.len());
        assert_eq!(DataValue::Null, record[0]);
        assert_eq!(DataValue::Int(1), record[1]);
        assert_eq!(DataValue::Int(8), record[8]);
        assert_eq!(DataValue::Null, record[9]);
    }

    #[test]
    fn test_deserialize_primary_key() {
        // Setup: A table where the primary key is composite (id, region_code)
        let mut schema = TableDefinition::new("multi_pk_table".to_string()).unwrap();
        schema.add_column(
            ColumnDefinition::new("id".to_string(), PrimitiveDataType::Int, false, None).unwrap(),
        );
        schema.add_column(
            ColumnDefinition::new(
                "name".to_string(),
                PrimitiveDataType::Varchar(255),
                false,
                None,
            )
            .unwrap(),
        );
        schema.add_column(
            ColumnDefinition::new(
                "region_code".to_string(),
                PrimitiveDataType::Int,
                false,
                None,
            )
            .unwrap(),
        );

        // PK on columns 0 and 2
        schema.add_constraint(
            Constraint::primary_key(
                "pk".to_string(),
                vec!["id".to_string(), "region_code".to_string()],
            )
            .unwrap(),
        );

        // Construct a full record buffer
        // Null Bitmap (1 byte): 0
        let mut bytes = vec![0u8];
        // Fixed: id=500, region_code=99
        bytes.extend_from_slice(&500i32.to_le_bytes());
        bytes.extend_from_slice(&99i32.to_le_bytes());
        // Variable: name="Worker"
        bytes.extend_from_slice(&6i32.to_le_bytes());
        bytes.extend_from_slice("Worker".as_bytes());

        // Act: Only extract the primary key
        let key =
            RecordSerializer::deserialize_primary_key(&schema, &bytes).expect("Should extract PK");

        // Assert: The key should contain (500, 99)
        assert_eq!(2, key.len());
        assert_eq!(DataValue::Int(500), key[0]);
        assert_eq!(DataValue::Int(99), key[1]);
    }
}
