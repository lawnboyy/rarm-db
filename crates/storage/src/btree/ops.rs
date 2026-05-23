use rarmdb_data_model::Key;
use rarmdb_schema_def::TableDefinition;

use crate::{SlottedPageView, record_serializer};

/// Performs a binary search of the records contained in the leaf node and returns the
/// slot index of the record if found. Otherwise, it returns the slot index of the
/// insertion point.
pub(crate) fn find_key(
    slotted_page: &SlottedPageView,
    key: &Key,
    table_def: &TableDefinition,
) -> Result<usize, usize> {
    let item_count = slotted_page.get_item_count();

    // Binary search algorithm...
    let mut low = 0;
    let mut high = item_count;
    while low < high {
        let mid = low + (high - low) / 2;
        let mid_point_data = slotted_page.get_record(mid).unwrap();
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
