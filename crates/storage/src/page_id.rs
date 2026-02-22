pub const PAGE_SIZE: u16 = 8192;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PageId {
    pub table_id: u32,
    pub page_index: u32,
}
