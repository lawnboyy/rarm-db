use rarmdb_data_model::Key;
use rarmdb_schema_def::TableDefinition;

use crate::SlottedPageView;

pub struct LeafNodeView<'a> {
    pub page_view: SlottedPageView<'a>,
}

impl<'a> LeafNodeView<'a> {
    pub fn new(page_view: SlottedPageView<'a>) -> Self {
        LeafNodeView { page_view }
    }

    pub fn find_key(&self, key: &Key, table_def: &TableDefinition) -> Result<usize, usize> {
        Ok(1)
    }
}
