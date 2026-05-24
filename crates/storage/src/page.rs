pub const PAGE_HEADER_SIZE: usize = 32;

// 8-byte aligned (u64)
pub const PAGE_HEADER_LSN_OFFSET: usize = 0;

// 4-byte aligned (u32)
pub const PAGE_HEADER_PARENT_INDEX_OFFSET: usize = 8;
// pub const PAGE_HEADER_DATA_START_OFFSET: usize = 12;
// Type Specific Offset 1 holds
//     - Table Header: Root page offset
//     - Internal Node: Rightmost child pointer
//     - Leaf Node: Previous Sibling Page Index
pub const PAGE_HEADER_ROOT_PAGE_OFFSET: usize = 12;
pub const PAGE_HEADER_RIGHTMOST_CHILD_POINTER_OFFSET: usize = 12;
pub const PAGE_HEADER_PREV_SIBLING_LEAF_PAGE_INDEX_OFFSET: usize = 12;

// Type Specific Offset 2 holds
//     - Leaf Node: Next Sibling Page Index
pub const PAGE_HEADER_NEXT_SIBLING_LEAF_PAGE_INDEX_OFFSET: usize = 16;

// 2-byte aligned (u16)
pub const PAGE_HEADER_ITEM_COUNT_OFFSET: usize = 20;
pub const PAGE_HEADER_DATA_HEAP_END_OFFSET_OFFSET: usize = 22;

// 1-byte aligned (u8)
pub const PAGE_HEADER_PAGE_TYPE_OFFSET: usize = 24;

// Represents an invalid page index (e.g. to represent that a leaf node has no right sibling)
pub const INVALID_PAGE_INDEX: u32 = u32::MAX;

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
