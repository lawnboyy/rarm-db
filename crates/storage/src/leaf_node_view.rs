use rarmdb_data_model::{Key, Record};
use rarmdb_schema_def::TableDefinition;

use crate::{SlottedPageView, StorageError, record_serializer, slot};

pub struct LeafNodeView<'a> {
    pub page_view: SlottedPageView<'a>,
}

impl<'a> LeafNodeView<'a> {
    pub fn new(page_view: SlottedPageView<'a>) -> Self {
        LeafNodeView { page_view }
    }

    /// Performs a binary search of the records contained in the leaf node and returns the
    /// slot index of the record if found. Otherwise, it returns the slot index of the
    /// insertion point.
    pub fn find_key(&self, key: &Key, table_def: &TableDefinition) -> Result<usize, usize> {
        let item_count = self.page_view.get_item_count();

        // Binary search algorithm...
        let mut low = 0;
        let mut high = item_count;
        while low < high {
            let mid = low + (high - low) / 2;
            let mid_point_data = self.page_view.get_record(mid).unwrap();
            let mid_point_key =
                record_serializer::deserialize_primary_key(table_def, mid_point_data).unwrap();

            if *key == mid_point_key {
                return Ok(mid as usize);
            } else if *key > mid_point_key {
                low = mid + 1;
            } else if *key < mid_point_key {
                high = mid;
            }
        }

        Err(low as usize)
    }

    pub fn insert_record(
        &mut self,
        record: &Record,
        table_def: &TableDefinition,
    ) -> Result<u16, StorageError> {
        // TODO: First check if there is enough available space...
        // Extract the primary key from this record...
        let primary_key = &record.get_primary_key(table_def);
        // Find the slot index where this record will be inserted...
        let slot_index = self.find_key(primary_key, table_def);

        // The key does not exist, so the slot index is the insertion point...
        if let Err(insertion_slot) = slot_index {
            // Serialize the record...
            let record_bytes = &record_serializer::serialize(&table_def.columns, record).unwrap();
            return self
                .page_view
                .try_add_record(insertion_slot as u16, record_bytes);
        }

        // A record with this primary key already exists, so it's a duplicate key...
        Err(StorageError::DuplicateKey)
    }
}
