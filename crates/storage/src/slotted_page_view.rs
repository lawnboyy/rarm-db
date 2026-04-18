use crate::{
    PageType,
    page::{
        DATA_HEAP_END_OFFSET_HEADER_OFFSET, HEADER_SIZE, ITEM_COUNT_HEADER_OFFSET,
        PAGE_TYPE_HEADER_OFFSET,
    },
    page_id::PAGE_SIZE,
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
        self.buffer[PAGE_TYPE_HEADER_OFFSET] = page_type as u8;

        // Data heap grows backwards, so the data heap starting offset is the end of the page.
        self.buffer[DATA_HEAP_END_OFFSET_HEADER_OFFSET..DATA_HEAP_END_OFFSET_HEADER_OFFSET + 2]
            .copy_from_slice(&u16::to_le_bytes(PAGE_SIZE as u16))
    }

    pub fn get_data_start_offset(&self) -> u16 {
        let bytes: [u8; 2] = self.buffer
            [DATA_HEAP_END_OFFSET_HEADER_OFFSET..DATA_HEAP_END_OFFSET_HEADER_OFFSET + 2]
            .try_into()
            .expect(
                "The index of one or more bytes at the item header offset exceed the page size!",
            );

        u16::from_le_bytes(bytes)
    }

    pub fn get_free_space(&self) -> usize {
        PAGE_SIZE - HEADER_SIZE
    }

    pub fn get_item_count(&self) -> u16 {
        let bytes: [u8; 2] = self.buffer[ITEM_COUNT_HEADER_OFFSET..ITEM_COUNT_HEADER_OFFSET + 2]
            .try_into()
            .expect(
                "The index of one or more bytes at the item header offset exceed the page size!",
            );

        u16::from_le_bytes(bytes)
    }

    pub fn get_page_type(&self) -> PageType {
        PageType::from(self.buffer[PAGE_TYPE_HEADER_OFFSET])
    }

    pub fn try_add_record(&mut self, record_data: &[u8], index: u16) -> Result<u16, StorageError> {
        // TODO: Check if there is enough space available on the page for this record...

        let initial_item_count = self.get_item_count();
        // The insertion index cannot be greater than the item count. Appending a record to the end of
        // the slot array would mean insertion index == item count.
        if index > initial_item_count {
            return Err(StorageError::InvalidSlotIndex);
        }

        // Determine the new data heap offset where the record data will be written to...
        let record_size = record_data.len();
        let data_heap_offset_bytes: [u8; 2] = self.buffer
            [DATA_HEAP_END_OFFSET_HEADER_OFFSET..DATA_HEAP_END_OFFSET_HEADER_OFFSET + 2]
            .try_into()
            .expect("Data heap offset exceeded the page size!");
        let data_heap_offset = u16::from_le_bytes(data_heap_offset_bytes);
        let new_data_heap_offset = data_heap_offset - record_size as u16;

        // Write the record to the data heap...
        self.buffer[new_data_heap_offset as usize..new_data_heap_offset as usize + record_size]
            .copy_from_slice(record_data);

        // Update the slot array
        // Write the record offset...
        // TODO: Adjust this based on the number of records...
        self.buffer[HEADER_SIZE..HEADER_SIZE + 2]
            .copy_from_slice(&new_data_heap_offset.to_le_bytes());
        // Write the record size...
        self.buffer[HEADER_SIZE + 2..HEADER_SIZE + 4]
            .copy_from_slice(&(record_size as u16).to_le_bytes());

        //  Update the item count
        let item_count = initial_item_count + 1;
        let item_count_bytes = u16::to_le_bytes(item_count);
        self.buffer[ITEM_COUNT_HEADER_OFFSET..ITEM_COUNT_HEADER_OFFSET + 2]
            .copy_from_slice(&item_count_bytes);

        Ok(index)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{PageType, page::HEADER_SIZE, page_id::PAGE_SIZE};

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
        assert_eq!(PAGE_SIZE - HEADER_SIZE, page.get_free_space());

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
        let slot0_start = HEADER_SIZE;
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
}
