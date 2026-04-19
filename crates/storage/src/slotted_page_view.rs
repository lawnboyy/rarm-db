use crate::{
    PageType,
    page::{
        PAGE_HEADER_DATA_HEAP_END_OFFSET_OFFSET, PAGE_HEADER_ITEM_COUNT_OFFSET,
        PAGE_HEADER_PAGE_TYPE_OFFSET, PAGE_HEADER_SIZE,
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

    pub fn initialize(&mut self, page_type: PageType) {
        // Set the byte at the page type offset...
        self.buffer[PAGE_HEADER_PAGE_TYPE_OFFSET] = page_type as u8;

        // Data heap grows backwards, so the data heap starting offset is the end of the page.
        self.buffer
            [PAGE_HEADER_DATA_HEAP_END_OFFSET_OFFSET..PAGE_HEADER_DATA_HEAP_END_OFFSET_OFFSET + 2]
            .copy_from_slice(&u16::to_le_bytes(PAGE_SIZE as u16))
    }

    pub fn get_data_start_offset(&self) -> u16 {
        let bytes: [u8; 2] = self.buffer
            [PAGE_HEADER_DATA_HEAP_END_OFFSET_OFFSET..PAGE_HEADER_DATA_HEAP_END_OFFSET_OFFSET + 2]
            .try_into()
            .expect(
                "The index of one or more bytes at the item header offset exceed the page size!",
            );

        u16::from_le_bytes(bytes)
    }

    /// Calculates the free space available on the page using the following:
    /// Free Space = Page Size - Page Header - Slot Array - Data Heap
    pub fn get_free_space(&self) -> usize {
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

    pub fn try_add_record(&mut self, record_data: &[u8], index: u16) -> Result<u16, StorageError> {
        // Check if there is enough space available on the page for this record...
        let free_space = self.get_free_space();
        if free_space < record_data.len() {
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

    fn get_page_header_u16_value(&self, offset: usize) -> u16 {
        let page_header_offset_bytes: [u8; 2] = self.buffer[offset..offset + 2]
            .try_into()
            .expect("Page header value offset exceeded the page size!");
        let page_header_value = u16::from_le_bytes(page_header_offset_bytes);
        page_header_value
    }

    fn set_page_header_u16_value(&mut self, offset: usize, value: u16) {
        let value_le_bytes = u16::to_le_bytes(value);
        self.buffer[offset..offset + 2].copy_from_slice(&value_le_bytes);
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
        page.initialize(PageType::LeafNode);

        assert_eq!(PageType::LeafNode, page.get_page_type());
        assert_eq!(0, page.get_item_count());

        // In a slotted page, DataStartOffset should initialize to the end of the page.
        // Assuming your get_data_start_offset() returns the raw u16/u32 value.
        assert_eq!(PAGE_SIZE as u16, page.get_data_start_offset());

        // Free space should be PAGE_SIZE (8192) - HEADER_SIZE (32) = 8160
        // because there are 0 slots and 0 records.
        assert_eq!(PAGE_SIZE - PAGE_HEADER_SIZE, page.get_free_space());

        let mut buffer2 = [0u8; PAGE_SIZE];
        let mut page2 = SlottedPageView::new(&mut buffer2);

        // We use LeafNode as a sample type for initialization
        page2.initialize(PageType::InternalNode);

        assert_eq!(PageType::InternalNode, page2.get_page_type());
    }

    #[test]
    fn test_add_record_verify_buffer_directly() {
        let mut buffer = [0u8; PAGE_SIZE];
        let mut page = SlottedPageView::new(&mut buffer);
        page.initialize(PageType::LeafNode);

        let data = b"Hello Slotted Page";
        let data_len = data.len() as u16;

        // Act: Add the first record at logical index 0
        page.try_add_record(data, 0)
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
            page.initialize(PageType::LeafNode);

            // 1. Add records to the start and end of the eventual array
            page.try_add_record(b"Record 0", 0).expect("Insert at 0");
            page.try_add_record(b"Record 2", 1).expect("Insert at 1");

            // 2. Insert into the middle (logical index 1)
            // This forces "Record 2" to shift right in the slot array.
            page.try_add_record(b"Record 1", 1)
                .expect("Insert in middle");

            (page.get_item_count(), page.get_free_space())
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
        page.initialize(PageType::LeafNode);

        // Cannot insert at index 1 if index 0 is empty
        let result = page.try_add_record(b"data", 1);
        assert!(
            result.is_err(),
            "Should not allow out-of-bounds insertion index"
        );
    }

    #[test]
    fn test_page_full_returns_error() {
        let mut buffer = [0u8; PAGE_SIZE];
        let mut page = SlottedPageView::new(&mut buffer);
        page.initialize(PageType::LeafNode);

        // Fill the page until it's almost full
        let max_data_size = PAGE_SIZE - PAGE_HEADER_SIZE - SLOT_SIZE;
        let giant_record = vec![0u8; max_data_size];

        page.try_add_record(&giant_record, 0)
            .expect("Should fit giant record");

        // CONTIGUOUS Free space should be 0
        assert_eq!(0, page.get_free_space());

        // Attempting to add anything else should fail
        let result = page.try_add_record(b"extra", 1);
        assert!(
            result.is_err(),
            "Should fail when no space for slot and data"
        );
    }
}
