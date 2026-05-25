use crate::{
    PageType,
    page::{
        PAGE_HEADER_DATA_HEAP_END_OFFSET_OFFSET, PAGE_HEADER_ITEM_COUNT_OFFSET,
        PAGE_HEADER_PAGE_TYPE_OFFSET, PAGE_HEADER_PARENT_INDEX_OFFSET, PAGE_HEADER_SIZE,
    },
    page_id::PAGE_SIZE,
    slot::SLOT_SIZE,
    storage_error::StorageError,
};

/// A struct that represents a structured view of the page frame buffer. It provides support
/// for slotted pages which use an ordered array of offsets to provide lookups of the records
/// stored on the page in the data heap. The data heap is unordered but starts at the end of
/// the page and grows towards the slot array. The slot array starts at the beginning of the
/// page after the header and grows towards the data heap. In between is free space. This
/// allows for performant deletes and inserts to the data heap.
///
/// Slotted Page Structure:
/// [ Header | Slots -> | Free Space | <- Data Record Cells ]
pub struct SlottedPageView<'a> {
    buffer: &'a mut [u8; PAGE_SIZE],
}

impl<'a> SlottedPageView<'a> {
    pub fn new(buffer: &'a mut [u8; PAGE_SIZE]) -> Self {
        SlottedPageView { buffer }
    }

    pub fn initialize(&mut self, page_type: PageType, parent_page_index: Option<u64>) {
        // Reset the data buffer...
        self.buffer.fill(0);

        // Set the byte at the page type offset...
        self.buffer[PAGE_HEADER_PAGE_TYPE_OFFSET] = page_type as u8;

        // Reset the item count...
        self.set_page_header_u16_value(PAGE_HEADER_ITEM_COUNT_OFFSET, 0);

        // Set the parent
        let parent_index = if let Some(index) = parent_page_index {
            index
        } else {
            u64::MAX
        };
        self.set_page_header_u64_value(PAGE_HEADER_PARENT_INDEX_OFFSET, parent_index);

        // Data heap grows backwards, so the data heap starting offset is the end of the page.
        self.buffer
            [PAGE_HEADER_DATA_HEAP_END_OFFSET_OFFSET..PAGE_HEADER_DATA_HEAP_END_OFFSET_OFFSET + 2]
            .copy_from_slice(&u16::to_le_bytes(PAGE_SIZE as u16))
    }

    /// Rewrites the existing record data heap as a contiguous block to eliminate fragmentation. If
    /// the current free space is equivalent to the contiguous free space block, then no action is
    /// performed.
    pub fn compact(&mut self) {
        // If the contiguous free space is the same as the total free space, then there is no
        // fragmentation and no compaction is needed.
        // TODO: This is slightly inefficient because the 2 free space methods are making some of the
        // same calculations and header value lookups.
        let free_space = self.get_free_space();
        if free_space == self.get_free_space_contiguous() {
            return;
        }

        // Pull the records into a vector to re-write to the data heap.
        let item_count = self.get_item_count();
        // Calculate the size of the compacted data block by taking the total page space minus the page
        // header and slot array and subtract the total free space.
        let total_free_and_data_space =
            PAGE_SIZE - PAGE_HEADER_SIZE - item_count as usize * SLOT_SIZE;
        let total_data_heap_size = total_free_and_data_space - free_space;
        let mut compacted_data_block: Vec<u8> = Vec::with_capacity(total_data_heap_size);
        // Loop through the slot array and fetch each record, writing it to the data heap vector.
        for i in 0..item_count {
            let record_data = self.get_record(i).unwrap();
            compacted_data_block.extend_from_slice(record_data);
        }

        // Now write the updated data block, one record at a time and update the slot offsets.
        // Initialize our current data heap offset to the end of the page.
        let mut current_data_heap_offset = PAGE_SIZE;
        let mut current_data_buffer_offset = 0;
        for i in 0..item_count {
            let (_, record_size) = self.get_slot(i);
            let record_data = &compacted_data_block
                [current_data_buffer_offset..current_data_buffer_offset + record_size];

            current_data_buffer_offset += record_size;

            // Write the record to the data heap at the new offset...
            current_data_heap_offset -= record_size;
            self.buffer[current_data_heap_offset as usize
                ..current_data_heap_offset as usize + record_size]
                .copy_from_slice(record_data);

            self.set_slot(i, current_data_heap_offset as u16, record_size as u16);
        }

        // Zero out the contiguous block for debugging purposes...
        let free_space_offset = PAGE_HEADER_SIZE + item_count as usize * SLOT_SIZE;
        self.buffer[free_space_offset..free_space_offset + free_space].fill(0);

        // Update the data heap offset in the header.
        self.set_page_header_u16_value(
            PAGE_HEADER_DATA_HEAP_END_OFFSET_OFFSET,
            current_data_heap_offset as u16,
        );
    }

    pub fn delete_record(&mut self, index: u16) {
        let item_count = self.get_item_count();

        // TODO: Should the method return an error in this case?
        // Ignore invalid index...
        if index >= item_count {
            return;
        }

        let slot_offset = PAGE_HEADER_SIZE + index as usize * SLOT_SIZE;
        let num_bytes_to_shift = item_count as usize * SLOT_SIZE - (index as usize + 1) * SLOT_SIZE;

        if num_bytes_to_shift > 0 {
            let source_offset = slot_offset + SLOT_SIZE;
            self.buffer.copy_within(
                source_offset..source_offset + num_bytes_to_shift,
                slot_offset,
            );
        }

        self.set_page_header_u16_value(PAGE_HEADER_ITEM_COUNT_OFFSET, item_count - 1);
    }

    pub fn get_data_start_offset(&self) -> u16 {
        self.get_page_header_u16_value(PAGE_HEADER_DATA_HEAP_END_OFFSET_OFFSET)
    }

    /// Calculates the total logical free space available on the page, including any
    /// fragmented space between records (e.g. as the result of deleting a record
    /// or updated a record with a smaller total size).
    pub fn get_free_space(&self) -> usize {
        // Get the slot array size...
        let item_count = self.get_item_count();
        let slot_array_size = item_count as usize * SLOT_SIZE;

        // Get the data heap size...
        // Data Heap Size = Page Size - Data Heap Offset
        let data_heap_offset =
            self.get_page_header_u16_value(PAGE_HEADER_DATA_HEAP_END_OFFSET_OFFSET);
        let data_heap_size = PAGE_SIZE - data_heap_offset as usize;

        let free_space_contiguous = PAGE_SIZE - PAGE_HEADER_SIZE - slot_array_size - data_heap_size;

        // Determine the total space used by the records, which may differ from the data heap size due to
        // soft deletes and record updates.
        let mut total_used_data_space = 0;
        for i in 0..item_count {
            total_used_data_space += self.get_slot(i).1;
        }

        free_space_contiguous + (data_heap_size - total_used_data_space)
    }

    /// Calculates the free space available in the contiguous free block between
    /// the slot array and the data block on the page using the following:
    /// Free Space = Page Size - Page Header - Slot Array - Data Heap
    pub fn get_free_space_contiguous(&self) -> usize {
        // Get the slot array size...
        let item_count = self.get_item_count();
        let slot_array_size = item_count as usize * SLOT_SIZE;

        // Get the data heap size...
        // Data Heap Size = Page Size - Data Heap Offset
        let data_heap_offset =
            self.get_page_header_u16_value(PAGE_HEADER_DATA_HEAP_END_OFFSET_OFFSET);
        let data_heap_size = PAGE_SIZE - data_heap_offset as usize;

        let free_space = PAGE_SIZE - PAGE_HEADER_SIZE - slot_array_size - data_heap_size;

        free_space
    }

    /// Returns the total number of records contained in this page.
    pub fn get_item_count(&self) -> u16 {
        let bytes: [u8; 2] = self.buffer
            [PAGE_HEADER_ITEM_COUNT_OFFSET..PAGE_HEADER_ITEM_COUNT_OFFSET + 2]
            .try_into()
            .expect(
                "The index of one or more bytes at the item header offset exceed the page size!",
            );

        u16::from_le_bytes(bytes)
    }

    pub fn get_page_type(&self) -> PageType {
        PageType::from(self.buffer[PAGE_HEADER_PAGE_TYPE_OFFSET])
    }

    pub fn get_record(&self, index: u16) -> Option<&[u8]> {
        // Check the item count against the requested index...
        let item_count = self.get_item_count();
        if index >= item_count {
            return None;
        }

        // Look up the slot at the provided index...
        let (record_offset, record_size) = self.get_slot(index);

        Some(&self.buffer[record_offset..record_offset + record_size])
    }

    /// Attempts to insert a new record to the page. If there is insufficient space on the page for
    /// the given record, a PageFull error is returned. Otherwise, the insertion index is returned.
    pub fn try_insert_record(
        &mut self,
        index: u16,
        record_data: &[u8],
    ) -> Result<u16, StorageError> {
        // Check if there is enough space available on the page for this record...
        let free_space = self.get_free_space_contiguous();
        if free_space < record_data.len() + SLOT_SIZE {
            return Err(StorageError::PageFull);
        }

        let initial_item_count = self.get_item_count();
        // The insertion index cannot be greater than the item count. Appending a record to the end of
        // the slot array would mean insertion index == item count.
        if index > initial_item_count {
            return Err(StorageError::InvalidSlotIndex);
        }

        // Determine the new data heap offset where the record data will be written to...
        let record_size = record_data.len();
        let data_heap_offset =
            self.get_page_header_u16_value(PAGE_HEADER_DATA_HEAP_END_OFFSET_OFFSET);
        let new_data_heap_offset = data_heap_offset - record_size as u16;

        // Write the record to the data heap at the new offset...
        self.buffer[new_data_heap_offset as usize..new_data_heap_offset as usize + record_size]
            .copy_from_slice(record_data);

        // Update the data heap offset in the page header...
        self.set_page_header_u16_value(
            PAGE_HEADER_DATA_HEAP_END_OFFSET_OFFSET,
            new_data_heap_offset,
        );

        // Update the slot array
        // First, determine the location to write the inserted slot...
        let slot_insertion_offset = PAGE_HEADER_SIZE + SLOT_SIZE * index as usize;

        // Check if slots need to be shifted to the right after the insertion index...
        let slots_to_shift = initial_item_count - index;
        // Shift the slots 1 to the right...
        if slots_to_shift > 0 {
            let source_offset = slot_insertion_offset;
            let destination_offset = source_offset + SLOT_SIZE;
            let slot_block_length = slots_to_shift as usize * SLOT_SIZE;

            self.buffer.copy_within(
                source_offset..source_offset + slot_block_length,
                destination_offset,
            );
        }

        // Write the new record offset...
        self.buffer[slot_insertion_offset..slot_insertion_offset + 2]
            .copy_from_slice(&new_data_heap_offset.to_le_bytes());
        // Write the record size...
        self.buffer[slot_insertion_offset + 2..slot_insertion_offset + 4]
            .copy_from_slice(&(record_size as u16).to_le_bytes());

        //  Update the item count
        let item_count = initial_item_count + 1;
        self.set_page_header_u16_value(PAGE_HEADER_ITEM_COUNT_OFFSET, item_count);

        Ok(index)
    }

    pub fn try_update_record(
        &mut self,
        index: u16,
        record_data: &[u8],
    ) -> Result<u16, StorageError> {
        let item_count = self.get_item_count();
        if index >= item_count {
            return Err(StorageError::InvalidSlotIndex);
        }

        // Use the given index to look up the slot...
        let (record_offset, orig_record_size) = self.get_slot(index);

        // Where the updated record gets written depends on whether the size changed...
        let updated_record_size = record_data.len();
        if updated_record_size <= orig_record_size {
            // If the size is the same or smaller, write the updated value to the current offset...
            self.buffer[record_offset..record_offset + updated_record_size]
                .copy_from_slice(&record_data);
            // Update the record size if necessary...
            if updated_record_size < orig_record_size {
                self.set_slot(index, record_offset as u16, updated_record_size as u16);
            }
        } else {
            // Otherwise, the updated record cannot be written to the current offset as it will overwrite
            // the adjacent record.
            // Return a page full error if there is insufficient space...
            let free_space = self.get_free_space_contiguous();
            if updated_record_size > free_space {
                return Err(StorageError::PageFull);
            }

            // If there is sufficient free space, calculate the new offset as the data heap
            // offset minus the updated record size.
            let data_heap_offset =
                self.get_page_header_u16_value(PAGE_HEADER_DATA_HEAP_END_OFFSET_OFFSET);
            let new_data_heap_offset = data_heap_offset - updated_record_size as u16;

            // Write the updated record to the new offset.
            self.buffer[new_data_heap_offset as usize
                ..new_data_heap_offset as usize + updated_record_size]
                .copy_from_slice(record_data);

            // Update the slot in the slot array to reflect the new offset and updated record size.
            self.set_slot(
                index,
                new_data_heap_offset as u16,
                updated_record_size as u16,
            );

            // Update the data heap offset in the page header.
            self.set_page_header_u16_value(
                PAGE_HEADER_DATA_HEAP_END_OFFSET_OFFSET,
                new_data_heap_offset,
            );
        }

        Ok(index)
    }

    pub fn get_page_header_u8_value(&self, offset: usize) -> u8 {
        let page_header_offset_bytes: [u8; 1] = self.buffer[offset..offset + 1]
            .try_into()
            .expect("Page header value offset exceeded the page size!");
        let page_header_value = u8::from_le_bytes(page_header_offset_bytes);
        page_header_value
    }

    pub fn get_page_header_u16_value(&self, offset: usize) -> u16 {
        let page_header_offset_bytes: [u8; 2] = self.buffer[offset..offset + 2]
            .try_into()
            .expect("Page header value offset exceeded the page size!");
        let page_header_value = u16::from_le_bytes(page_header_offset_bytes);
        page_header_value
    }

    pub(crate) fn get_page_header_u32_value(&self, offset: usize) -> u32 {
        let page_header_offset_bytes: [u8; 4] = self.buffer[offset..offset + 4]
            .try_into()
            .expect("Page header value offset exceeded the page size!");
        let page_header_value = u32::from_le_bytes(page_header_offset_bytes);
        page_header_value
    }

    pub(crate) fn get_page_header_u64_value(&self, offset: usize) -> u64 {
        let page_header_offset_bytes: [u8; 8] = self.buffer[offset..offset + 8]
            .try_into()
            .expect("Page header value offset exceeded the page size!");
        let page_header_value = u64::from_le_bytes(page_header_offset_bytes);
        page_header_value
    }

    pub(crate) fn set_page_header_u16_value(&mut self, offset: usize, value: u16) {
        let value_le_bytes = u16::to_le_bytes(value);
        self.buffer[offset..offset + 2].copy_from_slice(&value_le_bytes);
    }

    pub(crate) fn set_page_header_u32_value(&mut self, offset: usize, value: u32) {
        let value_le_bytes = u32::to_le_bytes(value);
        self.buffer[offset..offset + 4].copy_from_slice(&value_le_bytes);
    }

    pub(crate) fn set_page_header_u64_value(&mut self, offset: usize, value: u64) {
        let value_le_bytes = u64::to_le_bytes(value);
        self.buffer[offset..offset + 8].copy_from_slice(&value_le_bytes);
    }

    fn get_slot(&self, index: u16) -> (usize, usize) {
        // Look up the slot at the provided index...
        let slot_offset = PAGE_HEADER_SIZE + index as usize * SLOT_SIZE;
        let record_offset_bytes = self.buffer[slot_offset..slot_offset + 2]
            .try_into()
            .expect("Could not retrieve record offset from buffer!");
        let record_offset = u16::from_le_bytes(record_offset_bytes) as usize;
        let record_size_bytes = self.buffer[slot_offset + 2..slot_offset + 4]
            .try_into()
            .expect("Could not retrieve record size from buffer!");
        let record_size = u16::from_le_bytes(record_size_bytes) as usize;

        (record_offset, record_size)
    }

    fn set_slot(&mut self, index: u16, record_offset: u16, record_size: u16) {
        let slot_offset = PAGE_HEADER_SIZE + index as usize * SLOT_SIZE;
        // Write the record offset...
        self.buffer[slot_offset..slot_offset + 2].copy_from_slice(&record_offset.to_le_bytes());
        // Write the record size...
        self.buffer[slot_offset + 2..slot_offset + 4].copy_from_slice(&record_size.to_le_bytes());
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{PageType, page::PAGE_HEADER_SIZE, page_id::PAGE_SIZE, slot::SLOT_SIZE};

    #[test]
    fn test_initialize_sets_correct_defaults() {
        let mut buffer = [0u8; PAGE_SIZE];
        let mut page = SlottedPageView::new(&mut buffer);

        // We use LeafNode as a sample type for initialization
        page.initialize(PageType::LeafNode, None);

        assert_eq!(PageType::LeafNode, page.get_page_type());
        assert_eq!(0, page.get_item_count());

        // In a slotted page, DataStartOffset should initialize to the end of the page.
        // Assuming your get_data_start_offset() returns the raw u16/u32 value.
        assert_eq!(PAGE_SIZE as u16, page.get_data_start_offset());

        // Free space should be PAGE_SIZE (8192) - HEADER_SIZE (32) = 8160
        // because there are 0 slots and 0 records.
        assert_eq!(
            PAGE_SIZE - PAGE_HEADER_SIZE,
            page.get_free_space_contiguous()
        );

        let mut buffer2 = [0u8; PAGE_SIZE];
        let mut page2 = SlottedPageView::new(&mut buffer2);

        // We use LeafNode as a sample type for initialization
        page2.initialize(PageType::InternalNode, None);

        assert_eq!(PageType::InternalNode, page2.get_page_type());
    }

    #[test]
    fn test_add_record_verify_buffer_directly() {
        let mut buffer = [0u8; PAGE_SIZE];
        let mut page = SlottedPageView::new(&mut buffer);
        page.initialize(PageType::LeafNode, None);

        let data = b"Hello Slotted Page";
        let data_len = data.len() as u16;

        // Act: Add the first record at logical index 0
        page.try_insert_record(0, data)
            .expect("Should have room for record");

        // Assert: Metadata updated
        assert_eq!(1, page.get_item_count());

        // 1. Verify Slot 0 in the buffer (Header is 32 bytes, Slot is 4 bytes)
        // Slot format: [Offset (u16 LE), Length (u16 LE)]
        let slot0_start = PAGE_HEADER_SIZE;
        let actual_offset =
            u16::from_le_bytes(buffer[slot0_start..slot0_start + 2].try_into().unwrap());
        let actual_len =
            u16::from_le_bytes(buffer[slot0_start + 2..slot0_start + 4].try_into().unwrap());

        // The record should be placed at the very end of the page
        let expected_offset = (PAGE_SIZE - data.len()) as u16;
        assert_eq!(
            expected_offset, actual_offset,
            "Slot offset should point to the end of the buffer"
        );
        assert_eq!(data_len, actual_len, "Slot length should match data size");

        // 2. Verify Data in the buffer heap
        let heap_data = &buffer[actual_offset as usize..(actual_offset + actual_len) as usize];
        assert_eq!(
            data, heap_data,
            "Buffer data at calculated offset must match original input"
        );
    }

    #[test]
    fn test_insert_record_at_index_with_shifting() {
        let mut buffer = [0u8; PAGE_SIZE];

        let (item_count, free_space) = {
            let mut page = SlottedPageView::new(&mut buffer);
            page.initialize(PageType::LeafNode, None);

            // 1. Add records to the start and end of the eventual array
            page.try_insert_record(0, b"Record 0").expect("Insert at 0");
            page.try_insert_record(1, b"Record 2").expect("Insert at 1");

            // 2. Insert into the middle (logical index 1)
            // This forces "Record 2" to shift right in the slot array.
            page.try_insert_record(1, b"Record 1")
                .expect("Insert in middle");

            (page.get_item_count(), page.get_free_space_contiguous())
        };

        assert_eq!(3, item_count);

        // Verify logical order by inspecting the slot array directly
        let expected_values = vec![
            b"Record 0".as_slice(),
            b"Record 1".as_slice(),
            b"Record 2".as_slice(),
        ];

        for (i, expected) in expected_values.iter().enumerate() {
            let slot_start = PAGE_HEADER_SIZE + (i * SLOT_SIZE);
            let offset =
                u16::from_le_bytes(buffer[slot_start..slot_start + 2].try_into().unwrap()) as usize;
            let len = u16::from_le_bytes(buffer[slot_start + 2..slot_start + 4].try_into().unwrap())
                as usize;

            let actual = &buffer[offset..offset + len];
            assert_eq!(
                *expected, actual,
                "Data mismatch at logical slot index {}",
                i
            );
        }

        // Verify free space accounts for 3 records and 3 slots
        let expected_used =
            b"Record 0".len() + b"Record 1".len() + b"Record 2".len() + (3 * SLOT_SIZE);
        assert_eq!(PAGE_SIZE - PAGE_HEADER_SIZE - expected_used, free_space);
    }

    #[test]
    fn test_insert_at_invalid_index_fails() {
        let mut buffer = [0u8; PAGE_SIZE];
        let mut page = SlottedPageView::new(&mut buffer);
        page.initialize(PageType::LeafNode, None);

        // Cannot insert at index 1 if index 0 is empty
        let result = page.try_insert_record(1, b"data");
        assert!(
            result.is_err(),
            "Should not allow out-of-bounds insertion index"
        );
    }

    #[test]
    fn test_page_full_returns_error() {
        let mut buffer = [0u8; PAGE_SIZE];
        let mut page = SlottedPageView::new(&mut buffer);
        page.initialize(PageType::LeafNode, None);

        // Fill the page until it's almost full
        let max_data_size = PAGE_SIZE - PAGE_HEADER_SIZE - SLOT_SIZE;
        let giant_record = vec![0u8; max_data_size];

        page.try_insert_record(0, &giant_record)
            .expect("Should fit giant record");

        // CONTIGUOUS Free space should be 0
        assert_eq!(0, page.get_free_space_contiguous());

        // Attempting to add anything else should fail
        let result = page.try_insert_record(1, b"extra");
        assert!(
            result.is_err(),
            "Should fail when no space for slot and data"
        );
    }

    #[test]
    fn test_get_record_retrieves_correct_data() {
        let mut buffer = [0u8; PAGE_SIZE];
        let mut page = SlottedPageView::new(&mut buffer);
        page.initialize(PageType::LeafNode, None);

        let rec0 = b"Alpha";
        let rec1 = b"Beta";

        page.try_insert_record(0, rec0).unwrap();
        page.try_insert_record(1, rec1).unwrap();

        // Act & Assert
        assert_eq!(Some(rec0.as_slice()), page.get_record(0));
        assert_eq!(Some(rec1.as_slice()), page.get_record(1));
        assert_eq!(None, page.get_record(2));
    }

    #[test]
    fn test_update_record_in_place() {
        let mut buffer = [0u8; PAGE_SIZE];
        let mut page = SlottedPageView::new(&mut buffer);
        page.initialize(PageType::LeafNode, None);

        // 1. Setup: Add a record
        page.try_insert_record(0, b"Old Data")
            .expect("Initial add failed");
        let free_space_after_add = page.get_free_space_contiguous();

        // 2. Act: Update with new data of exact same length
        let result = page.try_update_record(0, b"New Data");

        // 3. Assert
        assert!(result.is_ok(), "Update of same-size record should succeed");
        assert_eq!(Some(b"New Data".as_slice()), page.get_record(0));
        assert_eq!(
            free_space_after_add,
            page.get_free_space_contiguous(),
            "Free space should remain identical for in-place update"
        );
    }

    #[test]
    fn test_update_record_relocates_when_larger() {
        let mut buffer = [0u8; PAGE_SIZE];
        let mut page = SlottedPageView::new(&mut buffer);
        page.initialize(PageType::LeafNode, None);

        // 1. Setup: Add a small record
        page.try_insert_record(0, b"Small")
            .expect("Initial add failed");
        let free_space_after_add = page.get_free_space_contiguous();

        // 2. Act: Update with much larger data
        let new_data = b"Much Larger Record Data";
        let result = page.try_update_record(0, new_data);

        // 3. Assert
        assert!(
            result.is_ok(),
            "Update with larger data should succeed when space is available"
        );
        assert_eq!(Some(new_data.as_slice()), page.get_record(0));

        // 4. Assert: Free space should decrease by the EXACT length of the new record.
        // The old record "Small" (5 bytes) becomes garbage in the heap, and a fresh 23 bytes
        // are consumed from the end of the heap.
        assert_eq!(
            free_space_after_add - new_data.len(),
            page.get_free_space_contiguous(),
            "Free space should decrease by the length of the relocated record"
        );
    }

    #[test]
    fn test_update_record_fails_when_relocation_exceeds_space() {
        let mut buffer = [0u8; PAGE_SIZE];
        let mut page = SlottedPageView::new(&mut buffer);
        page.initialize(PageType::LeafNode, None);

        // 1. Add an initial record (Small)
        page.try_insert_record(0, b"Small").unwrap();

        // 2. Fill almost the entire page with another record
        let remaining_usable = page.get_free_space_contiguous() - SLOT_SIZE;
        let filler = vec![0u8; remaining_usable - 10]; // Leave exactly 10 bytes of free space
        page.try_insert_record(1, &filler)
            .expect("Should fit filler");

        assert_eq!(10, page.get_free_space_contiguous());

        // 3. Attempt to update Record 0 with 20 bytes (Relocation required)
        let result = page.try_update_record(0, &[0u8; 20]);

        // 4. Assert: Should return PageFull because 20 > 10
        assert!(matches!(result, Err(StorageError::PageFull)));

        // Verify original data is still intact
        assert_eq!(Some(b"Small".as_slice()), page.get_record(0));
    }

    #[test]
    fn test_update_record_at_exact_remaining_space_limit() {
        let mut buffer = [0u8; PAGE_SIZE];
        let mut page = SlottedPageView::new(&mut buffer);
        page.initialize(PageType::LeafNode, None);

        // 1. Add an initial record
        page.try_insert_record(0, b"A").unwrap();

        // 2. Fill the page such that there are exactly 20 bytes of contiguous free space left
        // Note: we subtract SLOT_SIZE here because try_add_record consumes both data and a new slot entry.
        let remaining_for_filler = page.get_free_space_contiguous() - SLOT_SIZE - 20;
        let filler = vec![0u8; remaining_for_filler];
        page.try_insert_record(1, &filler).unwrap();

        let free_before = page.get_free_space_contiguous();
        assert_eq!(20, free_before);

        // 3. Act: Update record 0 with exactly 20 bytes.
        // Since 20 is larger than "A" (1 byte), it triggers relocation.
        // It should succeed because 20 is exactly equal to the available free_space.
        let update_result = page.try_update_record(0, &[1u8; 20]);

        // 4. Assert
        assert!(
            update_result.is_ok(),
            "Update with exact remaining space should succeed"
        );
        assert_eq!(
            0,
            page.get_free_space_contiguous(),
            "Free space should be completely exhausted"
        );

        let retrieved = page.get_record(0).unwrap();
        assert_eq!(20, retrieved.len());
        assert_eq!(&[1u8; 20], retrieved);
    }

    #[test]
    fn test_update_record_fails_one_byte_over_limit() {
        let mut buffer = [0u8; PAGE_SIZE];
        let mut page = SlottedPageView::new(&mut buffer);
        page.initialize(PageType::LeafNode, None);

        // 1. Setup: add a record
        page.try_insert_record(0, b"Data").unwrap();

        // 2. Get current free space
        let free = page.get_free_space_contiguous();

        // 3. Act: Attempt update with data that is exactly 1 byte larger than free space
        let larger_than_free = vec![0u8; free + 1];
        let result = page.try_update_record(0, &larger_than_free);

        // 4. Assert
        assert!(
            matches!(result, Err(StorageError::PageFull)),
            "Should fail when update is 1 byte larger than capacity"
        );
    }

    #[test]
    fn test_update_record_smaller_size_updates_slot_length() {
        let mut buffer = [0u8; PAGE_SIZE];
        let mut page = SlottedPageView::new(&mut buffer);
        page.initialize(PageType::LeafNode, None);

        // 1. Setup: Add a record with 10 bytes
        page.try_insert_record(0, b"0123456789").unwrap();
        let free_space_after_add = page.get_free_space_contiguous();

        // 2. Act: Update with a smaller record (4 bytes)
        let new_data = b"Abcd";
        page.try_update_record(0, new_data).unwrap();

        // 3. Assert: Data is retrieved correctly
        assert_eq!(Some(new_data.as_slice()), page.get_record(0));

        // 4. Verify physical slot length was actually reduced in the metadata
        let (_, record_size) = page.get_slot(0);
        assert_eq!(4, record_size);

        // 5. Free space should not change (heap data is overwritten but not moved or reclaimed)
        assert_eq!(free_space_after_add, page.get_free_space_contiguous());
    }

    #[test]
    fn test_update_record_invalid_index_returns_error() {
        let mut buffer = [0u8; PAGE_SIZE];
        let mut page = SlottedPageView::new(&mut buffer);
        page.initialize(PageType::LeafNode, None);

        // 1. Setup: Add 1 record
        page.try_insert_record(0, b"Original").unwrap();

        // 2. Act: Attempt to update index 1 (does not exist)
        let result = page.try_update_record(1, b"New Data");

        // 3. Assert: Should return InvalidSlotIndex
        // Note: This test will currently likely PANIC until you add the check in implementation
        assert!(matches!(result, Err(StorageError::InvalidSlotIndex)));
    }

    #[test]
    fn test_delete_record_shifts_slots_left() {
        let mut buffer = [0u8; PAGE_SIZE];
        let mut page = SlottedPageView::new(&mut buffer);
        page.initialize(PageType::LeafNode, None);

        // 1. Setup: Add 3 records
        page.try_insert_record(0, b"Record A").unwrap();
        page.try_insert_record(1, b"Record B").unwrap();
        page.try_insert_record(2, b"Record C").unwrap();

        let free_space_before = page.get_free_space_contiguous();

        // 2. Act: Delete the middle record (index 1)
        page.delete_record(1);

        // 3. Assert: Item count is updated
        assert_eq!(2, page.get_item_count());

        // 4. Assert: Free space increased by exactly SLOT_SIZE (4 bytes)
        // Note: The record data for "Record B" stays in the heap as garbage
        // until compaction, so only the slot array space is reclaimed here.
        assert_eq!(
            free_space_before + SLOT_SIZE,
            page.get_free_space_contiguous()
        );

        // 5. Assert: Verify logical order after shifting
        // Slot 0 should still be A
        assert_eq!(Some(b"Record A".as_slice()), page.get_record(0));
        // Slot 1 should now be C (shifted left)
        assert_eq!(Some(b"Record C".as_slice()), page.get_record(1));
        // Slot 2 should now be empty/invalid
        assert_eq!(None, page.get_record(2));
    }

    #[test]
    fn test_delete_first_record_shifts_all_others() {
        let mut buffer = [0u8; PAGE_SIZE];
        let mut page = SlottedPageView::new(&mut buffer);
        page.initialize(PageType::LeafNode, None);

        // 1. Setup: Add 3 records
        page.try_insert_record(0, b"Record 0").unwrap();
        page.try_insert_record(1, b"Record 1").unwrap();
        page.try_insert_record(2, b"Record 2").unwrap();

        // 2. Act: Delete the very first record (index 0)
        page.delete_record(0);

        // 3. Assert: Item count is now 2
        assert_eq!(2, page.get_item_count());

        // 4. Assert: Verify logical order after shifting
        // Record 1 should now be at logical index 0
        assert_eq!(Some(b"Record 1".as_slice()), page.get_record(0));
        // Record 2 should now be at logical index 1
        assert_eq!(Some(b"Record 2".as_slice()), page.get_record(1));
        // Index 2 should now be invalid
        assert_eq!(None, page.get_record(2));
    }

    #[test]
    fn test_delete_record_invalid_index_is_noop() {
        let mut buffer = [0u8; PAGE_SIZE];
        let mut page = SlottedPageView::new(&mut buffer);
        page.initialize(PageType::LeafNode, None);

        // 1. Setup: Add 1 record
        page.try_insert_record(0, b"Stay").unwrap();
        let initial_free_space = page.get_free_space_contiguous();

        // 2. Act: Attempt to delete an out-of-bounds index (index 1 when only 0 exists)
        // This test identifies the current underflow issue.
        page.delete_record(1);

        // 3. Assert: State should remain unchanged
        assert_eq!(1, page.get_item_count());
        assert_eq!(initial_free_space, page.get_free_space_contiguous());
        assert_eq!(Some(b"Stay".as_slice()), page.get_record(0));
    }

    #[test]
    fn test_interleaved_inserts_and_deletes() {
        let mut buffer = [0u8; PAGE_SIZE];
        let mut page = SlottedPageView::new(&mut buffer);
        page.initialize(PageType::LeafNode, None);

        // Sequence mimicking B-Tree rebalancing/shifts

        // 1. Initial Fill: [A, B, C]
        page.try_insert_record(0, b"A").unwrap();
        page.try_insert_record(1, b"B").unwrap();
        page.try_insert_record(2, b"C").unwrap();

        // 2. Delete from middle: [A, C]
        page.delete_record(1);
        assert_eq!(2, page.get_item_count());
        assert_eq!(Some(b"A".as_slice()), page.get_record(0));
        assert_eq!(Some(b"C".as_slice()), page.get_record(1));

        // 3. Insert into middle: [A, D, C]
        page.try_insert_record(1, b"D").unwrap();
        assert_eq!(3, page.get_item_count());
        assert_eq!(Some(b"A".as_slice()), page.get_record(0));
        assert_eq!(Some(b"D".as_slice()), page.get_record(1));
        assert_eq!(Some(b"C".as_slice()), page.get_record(2));

        // 4. Update the "shifted" tail: [A, D, CCC]
        page.try_update_record(2, b"CCC").unwrap();
        assert_eq!(Some(b"CCC".as_slice()), page.get_record(2));

        // 5. Final Delete of all: []
        page.delete_record(0);
        page.delete_record(0);
        page.delete_record(0);
        assert_eq!(0, page.get_item_count());
        assert_eq!(None, page.get_record(0));
    }

    #[test]
    fn test_get_total_free_space_with_fragmentation() {
        let mut buffer = [0u8; PAGE_SIZE];
        let mut page = SlottedPageView::new(&mut buffer);
        page.initialize(PageType::LeafNode, None);

        // 1. Add three records of 10 bytes each
        // Each add uses: 10 bytes (data) + 4 bytes (slot) = 14 bytes
        page.try_insert_record(0, b"0123456789").unwrap();
        page.try_insert_record(1, b"0123456789").unwrap();
        page.try_insert_record(2, b"0123456789").unwrap();

        let initial_contiguous = page.get_free_space_contiguous();

        // 2. Delete the middle record (index 1)
        // This removes the 4-byte slot from the array, but the 10-byte record
        // remains in the heap as garbage.
        page.delete_record(1);

        // 3. Verify fragmentation
        // Contiguous free space should only increase by the 4 bytes reclaimed from the slot array.
        assert_eq!(
            initial_contiguous + SLOT_SIZE,
            page.get_free_space_contiguous()
        );

        // Total free space should now include the 10-byte "hole" plus the 4 bytes from the slot.
        // We calculate expected total based on PAGE_SIZE - HEADER - (Remaining Items * SlotSize) - (Remaining Data)
        let expected_total = PAGE_SIZE - PAGE_HEADER_SIZE - (2 * SLOT_SIZE) - (2 * 10);
        assert_eq!(expected_total, page.get_free_space());

        // 4. Update a record to relocate it
        // Updating record 0 (10 bytes) to 20 bytes.
        // 20 > 10, so it will be relocated to the end of the current data heap.
        // The old 10 bytes become garbage.
        page.try_update_record(0, &[1u8; 20]).unwrap();

        // Contiguous space decreases by the FULL size of the new record (20 bytes).
        // Total free space decreases because we added 20 bytes of new data, but
        // conceptually it only changed by the difference of what's logically alive.
        let expected_total_after_relocate =
            PAGE_SIZE - PAGE_HEADER_SIZE - (2 * SLOT_SIZE) - (10 + 20);
        assert_eq!(expected_total_after_relocate, page.get_free_space());
    }

    #[test]
    fn test_compact_reclaims_fragmented_space() {
        let mut buffer = [0u8; PAGE_SIZE];
        let mut page = SlottedPageView::new(&mut buffer);
        page.initialize(PageType::LeafNode, None);

        // 1. Setup: Add 3 records
        page.try_insert_record(0, b"Record 0").unwrap();
        page.try_insert_record(1, b"Record 1").unwrap();
        page.try_insert_record(2, b"Record 2").unwrap();

        // 2. Fragment the page by deleting the middle record
        page.delete_record(1);

        let free_before = page.get_free_space();
        let contiguous_before = page.get_free_space_contiguous();

        // Assert fragmentation exists
        assert!(
            contiguous_before < free_before,
            "Page must be fragmented before compaction"
        );

        // 3. Act: Call compact
        page.compact();

        // 4. Assert: Contiguous free space should now equal total free space
        assert_eq!(
            page.get_free_space(),
            page.get_free_space_contiguous(),
            "Compaction should eliminate all internal fragmentation"
        );

        // 5. Assert: Data integrity maintained
        assert_eq!(Some(b"Record 0".as_slice()), page.get_record(0));
        assert_eq!(Some(b"Record 2".as_slice()), page.get_record(1));
        assert_eq!(None, page.get_record(2));

        // 6. Verify physical repacking: The records should be at the very end of the page
        // ItemCount is 2. Total record data is 8 + 8 = 16 bytes.
        // New DataStart should be PAGE_SIZE - 16.
        assert_eq!(
            (PAGE_SIZE - 16) as u16,
            page.get_data_start_offset(),
            "DataStartOffset should point to the start of the repacked contiguous block"
        );
    }

    #[test]
    fn test_compact_reclaims_space_after_relocation_updates() {
        let mut buffer = [0u8; PAGE_SIZE];
        let mut page = SlottedPageView::new(&mut buffer);
        page.initialize(PageType::LeafNode, None);

        // 1. Add a small record
        page.try_insert_record(0, b"Original").unwrap();
        let initial_total_free = page.get_free_space();

        // 2. Update to a much larger size to force relocation
        // "Original" (8 bytes) stays as garbage, 20 new bytes written at end.
        page.try_update_record(0, &[1u8; 20]).unwrap();

        let free_before = page.get_free_space();
        let contiguous_before = page.get_free_space_contiguous();
        assert!(
            contiguous_before < free_before,
            "Updates should have created garbage holes"
        );

        // 3. Act: Compact
        page.compact();

        // 4. Assert
        assert_eq!(page.get_free_space(), page.get_free_space_contiguous());
        // The expected total free space after the 20-byte update (from initial)
        // remains the same, but contiguous should now match it.
        assert_eq!(
            initial_total_free - (20 - 8),
            page.get_free_space_contiguous()
        );

        let data = page.get_record(0).unwrap();
        assert_eq!(data, &[1u8; 20]);
    }

    #[test]
    fn test_compact_after_all_records_deleted_resets_heap() {
        let mut buffer = [0u8; PAGE_SIZE];
        let mut page = SlottedPageView::new(&mut buffer);
        page.initialize(PageType::LeafNode, None);

        page.try_insert_record(0, b"Data A").unwrap();
        page.try_insert_record(1, b"Data B").unwrap();

        // Delete everything
        page.delete_record(0);
        page.delete_record(0);

        assert_eq!(0, page.get_item_count());
        assert!(
            page.get_data_start_offset() < PAGE_SIZE as u16,
            "Heap should still be physically occupied by garbage"
        );

        // Act
        page.compact();

        // Assert
        assert_eq!(
            PAGE_SIZE as u16,
            page.get_data_start_offset(),
            "Compacting empty page should reset DataStartOffset to PAGE_SIZE"
        );
        assert_eq!(
            PAGE_SIZE - PAGE_HEADER_SIZE,
            page.get_free_space_contiguous()
        );
    }

    #[test]
    fn test_compact_on_already_contiguous_page_is_noop() {
        let mut buffer = [0u8; PAGE_SIZE];
        let mut page = SlottedPageView::new(&mut buffer);
        page.initialize(PageType::LeafNode, None);

        page.try_insert_record(0, b"Contiguous").unwrap();
        let data_start_before = page.get_data_start_offset();
        let free_before = page.get_free_space_contiguous();

        // Act
        page.compact();

        // Assert: No change
        assert_eq!(data_start_before, page.get_data_start_offset());
        assert_eq!(free_before, page.get_free_space_contiguous());
    }
}
