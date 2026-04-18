use crate::{
    PageType,
    page::{HEADER_SIZE, ITEM_COUNT_OFFSET, PAGE_TYPE_OFFSET},
    page_id::PAGE_SIZE,
};

pub struct SlottedPageView<'a> {
    buffer: &'a mut [u8; PAGE_SIZE],
}

impl<'a> SlottedPageView<'a> {
    pub fn new(buffer: &'a mut [u8; PAGE_SIZE]) -> Self {
        SlottedPageView { buffer }
    }

    pub fn initialize(&mut self, page_type: PageType) {
        // Set the byte at the page type offset...
        self.buffer[PAGE_TYPE_OFFSET] = page_type as u8;
    }

    pub fn get_data_start_offset(&self) -> u16 {
        PAGE_SIZE as u16
    }

    pub fn get_free_space(&self) -> usize {
        PAGE_SIZE - HEADER_SIZE
    }

    pub fn get_item_count(&self) -> u16 {
        let buffer_from_item_count_offset = &self.buffer[ITEM_COUNT_OFFSET..];
        let (item_count_size_bytes, _) = buffer_from_item_count_offset.split_at(size_of::<u16>());
        u16::from_le_bytes(item_count_size_bytes.try_into().unwrap())
    }

    pub fn get_page_type(&self) -> PageType {
        PageType::from(self.buffer[PAGE_TYPE_OFFSET])
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
}
