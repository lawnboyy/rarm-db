use rarmdb_data_model::Key;
use rarmdb_schema_def::TableDefinition;

use crate::{SlottedPageView, record_serializer};

pub struct LeafNodeView<'a> {
    pub page_view: SlottedPageView<'a>,
}

impl<'a> LeafNodeView<'a> {
    pub fn new(page_view: SlottedPageView<'a>) -> Self {
        LeafNodeView { page_view }
    }

    /// Performs a binary search of the records contained in the leaf node and returns the
    /// slot index of the record if found. Otherwise, it returns the slot index of the
    /// insertion point.
    pub fn find_key(&self, key: &Key, table_def: &TableDefinition) -> Result<usize, usize> {
        let item_count = self.page_view.get_item_count();

        // Binary search algorithm...
        let mut low = 0;
        let mut high = item_count;
        while low < high {
            let mid = low + (high - low) / 2;
            let mid_point_data = self.page_view.get_record(mid).unwrap();
            let mid_point_key =
                record_serializer::deserialize_primary_key(table_def, mid_point_data).unwrap();

            if *key == mid_point_key {
                return Ok(mid as usize);
            } else if *key > mid_point_key {
                low = mid + 1;
            } else if *key < mid_point_key {
                high = mid;
            }
        }

        Err(low as usize)
    }
}
