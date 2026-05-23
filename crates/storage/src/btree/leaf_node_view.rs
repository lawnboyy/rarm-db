use rarmdb_data_model::{Key, Record};
use rarmdb_schema_def::TableDefinition;

use crate::{
    PageId, SlottedPageView, StorageError,
    btree::ops,
    page::{TYPE_SPECIFIC_POINTER_1_OFFSET, TYPE_SPECIFIC_POINTER_2_OFFSET},
    record_serializer,
};

pub struct LeafNodeView<'a> {
    pub page_id: PageId,
    pub page_view: SlottedPageView<'a>,
}

impl<'a> LeafNodeView<'a> {
    pub fn new(page_id: PageId, page_view: SlottedPageView<'a>) -> Self {
        LeafNodeView { page_id, page_view }
    }

    pub fn get_next_leaf_index(&self) -> Option<u32> {
        let val = self
            .page_view
            .get_page_header_u32_value(TYPE_SPECIFIC_POINTER_1_OFFSET);
        if val == 0 { None } else { Some(val) }
    }

    pub fn get_prev_leaf_index(&self) -> Option<u32> {
        let val = self
            .page_view
            .get_page_header_u32_value(TYPE_SPECIFIC_POINTER_2_OFFSET);
        if val == 0 { None } else { Some(val) }
    }

    /// Performs a binary search of the records contained in the leaf node and returns the
    /// slot index of the record if found. Otherwise, it returns the slot index of the
    /// insertion point.
    pub fn find_key(&self, key: &Key, table_def: &TableDefinition) -> Result<usize, usize> {
        ops::find_key(&self.page_view, key, table_def)
    }

    pub fn insert_record(
        &mut self,
        record: &Record,
        table_def: &TableDefinition,
    ) -> Result<u16, StorageError> {
        // Extract the primary key from this record...
        let primary_key = &record.get_primary_key(table_def);
        // Find the slot index where this record will be inserted...
        let slot_index = self.find_key(primary_key, table_def);

        // The key does not exist, so the slot index is the insertion point...
        if let Err(insertion_slot) = slot_index {
            // Serialize the record...
            let record_bytes = &record_serializer::serialize(&table_def.columns, record).unwrap();

            // The slotted page will return an error if there is insufficient space to insert this record.
            return self
                .page_view
                .try_add_record(insertion_slot as u16, record_bytes);
        }

        // A record with this primary key already exists, so it's a duplicate key...
        Err(StorageError::DuplicateKey)
    }

    pub(crate) fn set_next_leaf_index(&self, page_index: Option<u32>) {}

    pub(crate) fn set_prev_leaf_index(&self, page_index: Option<u32>) {}

    pub(crate) fn split_and_insert(
        &self,
        record: &Record,
        right_sibling: &mut LeafNodeView,
        orig_right_sibling: Option<&mut LeafNodeView>,
        schema: &TableDefinition,
    ) -> Result<(), StorageError> {
        Err(StorageError::PageFull)
    }

    pub fn update_record(
        &mut self,
        record: &Record,
        table_def: &TableDefinition,
    ) -> Result<u16, StorageError> {
        // Extract the primary key of the record...
        let key = record.get_primary_key(table_def);
        let slot_result = ops::find_key(&self.page_view, &key, table_def);

        if let Ok(slot_index) = slot_result {
            let record_data = record_serializer::serialize(&table_def.columns, record)
                .expect("There was an error serializing the record!");
            self.page_view
                .try_update_record(slot_index as u16, Vec::as_slice(&record_data))
        } else {
            Err(StorageError::KeyNotFound)
        }
    }
}
