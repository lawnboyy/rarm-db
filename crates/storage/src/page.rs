pub const HEADER_SIZE: usize = 32;

pub const ITEM_COUNT_HEADER_OFFSET: usize = 8;
pub const DATA_HEAP_END_OFFSET_HEADER_OFFSET: usize = 12;
pub const PAGE_TYPE_HEADER_OFFSET: usize = 28;

#[repr(u8)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PageType {
    Invalid = 0,
    /// A B+Tree leaf page, containing actual table row data.
    LeafNode = 1,
    /// A B+Tree internal page, containing keys and pointers to child pages.
    InternalNode = 2,
    /// Metadata about the table, such as the root page index.
    TableHeader = 3,
}

impl From<u8> for PageType {
    fn from(value: u8) -> Self {
        match value {
            1 => PageType::LeafNode,
            2 => PageType::InternalNode,
            3 => PageType::TableHeader,
            _ => PageType::Invalid,
        }
    }
}
