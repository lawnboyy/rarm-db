use rarmdb_data_model::{Key, Record};
use rarmdb_schema_def::TableDefinition;

use crate::{
    PageId, SlottedPageView, StorageError,
    btree::ops,
    page::{
        INVALID_PAGE_INDEX, PAGE_HEADER_ITEM_COUNT_OFFSET,
        PAGE_HEADER_NEXT_SIBLING_LEAF_PAGE_INDEX_OFFSET, PAGE_HEADER_PARENT_INDEX_OFFSET,
        PAGE_HEADER_PREV_SIBLING_LEAF_PAGE_INDEX_OFFSET,
    },
    record_serializer, slot,
};

pub struct LeafNodeView<'a> {
    pub page_id: PageId,
    pub page_view: SlottedPageView<'a>,
}

impl<'a> LeafNodeView<'a> {
    pub fn new(page_id: PageId, page_view: SlottedPageView<'a>) -> Self {
        LeafNodeView { page_id, page_view }
    }

    /// This is a convenience method for performance that will append a record to the end of the slot array. It
    /// assumes that the caller knows that this record's key is the largest on the page. This is useful for splits
    /// and merges because the records will be ordered and appended to the page one at a time. There is no need
    /// to perform the binary search to find the insertion index.
    pub fn append(&mut self, record_data: &[u8]) -> Result<u16, StorageError> {
        // Retrieve the item count from the header... this will be the insertion index.
        let item_count = self
            .page_view
            .get_page_header_u16_value(PAGE_HEADER_ITEM_COUNT_OFFSET);
        self.page_view.try_insert_record(item_count, record_data)
    }

    pub fn get_item_count(&self) -> u16 {
        self.page_view
            .get_page_header_u16_value(PAGE_HEADER_ITEM_COUNT_OFFSET)
    }

    pub fn get_next_leaf_index(&self) -> Option<u32> {
        let val = self
            .page_view
            .get_page_header_u32_value(PAGE_HEADER_NEXT_SIBLING_LEAF_PAGE_INDEX_OFFSET);
        if val == INVALID_PAGE_INDEX {
            None
        } else {
            Some(val)
        }
    }

    pub fn get_prev_leaf_index(&self) -> Option<u32> {
        let val = self
            .page_view
            .get_page_header_u32_value(PAGE_HEADER_PREV_SIBLING_LEAF_PAGE_INDEX_OFFSET);
        if val == INVALID_PAGE_INDEX {
            None
        } else {
            Some(val)
        }
    }

    /// Performs a binary search of the records contained in the leaf node and returns the
    /// slot index of the record if found. Otherwise, it returns the slot index of the
    /// insertion point.
    pub fn find_key(&self, key: &Key, table_def: &TableDefinition) -> Result<usize, usize> {
        ops::find_key(&self.page_view, key, table_def)
    }

    pub fn insert_raw_record(
        &mut self,
        slot_index: u16,
        record_bytes: &[u8],
    ) -> Result<u16, StorageError> {
        self.page_view.try_insert_record(slot_index, record_bytes)
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
                .try_insert_record(insertion_slot as u16, record_bytes);
        }

        // A record with this primary key already exists, so it's a duplicate key...
        Err(StorageError::DuplicateKey)
    }

    /// Appends the records of the right sibling to the end of this leaf node to reclaim a page of storage and keep
    /// the nodes data dense.
    // pub fn merge(
    //     &mut self,
    //     right_sibling: &mut LeafNodeView<'_>,
    //     right_sibling_next: Option<&mut LeafNodeView<'_>>,
    //     table_def: &TableDefinition,
    // ) -> Result<(), StorageError> {
    // }

    pub fn set_next_leaf_index(&mut self, page_index: Option<u32>) {
        let index = if let Some(i) = page_index {
            i
        } else {
            INVALID_PAGE_INDEX
        };
        self.page_view
            .set_page_header_u32_value(PAGE_HEADER_NEXT_SIBLING_LEAF_PAGE_INDEX_OFFSET, index);
    }

    pub fn set_prev_leaf_index(&mut self, page_index: Option<u32>) {
        let index = if let Some(i) = page_index {
            i
        } else {
            INVALID_PAGE_INDEX
        };
        self.page_view
            .set_page_header_u32_value(PAGE_HEADER_PREV_SIBLING_LEAF_PAGE_INDEX_OFFSET, index);
    }

    /// Finds the ordered position for the new record, calculates a split index based on
    /// actual data size in bytes, formats this page and writes half the sorted data to
    /// this page and half to the new right sibling node. If the b-tree orchestrator
    /// attempts an insert and it fails because the page is full, then it can allocate
    /// a new page and call this method to distribute the data across 2 nodes.
    pub fn split_and_insert(
        &mut self,
        record: &Record,
        right_sibling: &mut LeafNodeView,
        orig_right_sibling: Option<&mut LeafNodeView>,
        table_def: &TableDefinition,
    ) -> Result<Key, StorageError> {
        // Check the item count of the new right sibling to ensure that it's empty, otherwise it
        // is an error condition.
        if right_sibling.get_item_count() != 0 {
            return Err(StorageError::NewRightSiblingNotEmpty);
        }

        let item_count = self.get_item_count();

        // Serialize the new record to insert into the sorted list to split between nodes.
        let serialized_new_record =
            record_serializer::serialize(&table_def.columns, record).unwrap();

        // Get the new record's primary key...
        let new_record_key = record.get_primary_key(table_def);
        // Create a vector to hold the sorted list of primary keys for this leaf with the new primary key inserted.
        let mut sorted_keys: Vec<Key> = Vec::new();
        let mut sorted_key_index: usize = 0;
        // Has the record been inserted yet?
        let mut new_record_inserted = false;
        // Create a vector to hold the sorted list of raw record references.
        let mut sorted_records: Vec<Box<[u8]>> = Vec::new();
        // Capture the total size in bytes of all records to determine where to split the node.
        let mut total_size: usize = 0;

        // Loop through this leaf's records to build a sorted list of existing records, plus the new record.
        for index in 0..item_count {
            let record_option = self.page_view.get_record(index);
            // Guard against an invalid slot index...
            if let Some(record_bytes) = record_option {
                let owned_record_bytes: Box<[u8]> = Box::from(record_bytes);
                // Add to our byte total...
                total_size += record_bytes.len();
                // Deserialize the record's primary key
                let current_key =
                    record_serializer::deserialize_primary_key(table_def, record_bytes)
                        .expect("Could not deserialize the record's key!");
                // Compare with the new record's primary key...
                if !new_record_inserted && new_record_key < current_key {
                    // ...and insert a reference to the new record key if less than current record primary key.
                    sorted_keys.insert(sorted_key_index, new_record_key.clone());
                    sorted_records
                        .insert(sorted_key_index, Box::from(serialized_new_record.clone()));
                    sorted_key_index += 1;
                    new_record_inserted = true;
                    total_size += serialized_new_record.len();
                }
                // Add the current record to the lists
                sorted_keys.insert(sorted_key_index, current_key);
                sorted_records.insert(sorted_key_index, owned_record_bytes);
                sorted_key_index += 1;
            } else {
                return Err(StorageError::InvalidSlotIndex);
            }
        }

        // If the new record was never inserted because it's the largest key, insert it now.
        if !new_record_inserted {
            sorted_keys.insert(sorted_key_index, new_record_key);
            sorted_records.insert(sorted_key_index, Box::from(serialized_new_record));
        }

        // Determine the midpoint based on data size to ensure data is evenly distributed across the nodes. This will be the separator key.
        // TODO: Can optimize this by just doing total / 2 if there are no variable length columns in the schema.
        let split_index = LeafNodeView::find_split_index_by_size(&sorted_records, total_size);
        let mid_point_key = sorted_keys[split_index].clone();

        // Capture header information prior to rewriting the page
        // Capture parent page index
        let raw_parent_index = self
            .page_view
            .get_page_header_u64_value(PAGE_HEADER_PARENT_INDEX_OFFSET);

        let parent_index = if raw_parent_index == u64::MAX {
            None
        } else {
            Some(raw_parent_index)
        };

        // Capture next leaf page index (new right sibling will point to this page)
        let orig_next_page_index = self.get_next_leaf_index();

        // Re-initialize the page
        self.page_view
            .initialize(crate::PageType::LeafNode, parent_index);

        // Copy the first half of the data to this leaf page...
        for index in 0..split_index {
            // Append the raw record to the page (we can just call insert here, but it's essentially an append operation because
            // the records are sorted)
            let record = &sorted_records[index];
            let insert_result = self.append(record);
            if let Err(err) = insert_result {
                return Err(err);
            }
        }

        // For midpoint to data row length
        let final_count: usize = item_count as usize + 1;
        for index in split_index..final_count {
            // Append the raw record to the page (we can just call insert here, but it's essentially an append operation because
            // the records are sorted)
            let record = &sorted_records[index];
            // Append the raw record to the new right sibling leaf
            let insert_result = right_sibling.append(record);
            if let Err(err) = insert_result {
                return Err(err);
            }
        }

        // Adjust the sibling pointers
        // Set this leaf's next page to the new right sibling page index
        self.set_next_leaf_index(Some(right_sibling.page_id.page_index));
        // Set the new right sibling's next page index to this page's original next page index
        right_sibling.set_next_leaf_index(orig_next_page_index);
        // Set the new right sibling's previous page index to this page index
        right_sibling.set_prev_leaf_index(Some(self.page_id.page_index));

        // If this node had right sibling before the split, we need to update it's previous sibling page index.
        if let Some(orig_right) = orig_right_sibling {
            orig_right.set_prev_leaf_index(Some(right_sibling.page_id.page_index));
        }

        // Return the new separator key
        Ok(mid_point_key)
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

    fn find_split_index_by_size(sorted_records: &Vec<Box<[u8]>>, total_size: usize) -> usize {
        let half_of_total = total_size / 2;
        let mut current_data_size = 0;
        let mut index = 0;
        for record in sorted_records {
            current_data_size += record.len();

            if current_data_size > half_of_total {
                return index;
            }

            index += 1;
        }

        index
    }
}
