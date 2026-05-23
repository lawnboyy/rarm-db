use std::fmt::{Display, Formatter, Result};

pub const PAGE_SIZE: usize = 8192;

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct PageId {
    pub table_id: u32,
    pub page_index: u32,
}

impl PageId {
    pub fn new(table_id: u32, page_index: u32) -> Self {
        PageId {
            table_id,
            page_index,
        }
    }
}

impl Display for PageId {
    fn fmt(&self, f: &mut Formatter<'_>) -> Result {
        // We use the write! macro to stream the text directly into the formatter.
        write!(f, "Table: {}, Page: {}", self.table_id, self.page_index)
    }
}
