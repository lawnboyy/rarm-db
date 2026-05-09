use crate::{LeafNodeView, SlottedPageView};

pub enum BTreeNode<'a> {
    Internal(SlottedPageView<'a>),
    Leaf(LeafNodeView<'a>),
}

impl<'a> BTreeNode<'a> {
    pub fn new(slotted_page_view: SlottedPageView<'a>) -> Self {
        // TODO: Implement matching based on node type
        let leaf = LeafNodeView::new(slotted_page_view);
        BTreeNode::Leaf(leaf)
    }
}

#[cfg(test)]
mod tests {
    use rarmdb_data_model::{DataValue, Key, Record};
    use rarmdb_schema_def::constraint::Constraint;
    use rarmdb_schema_def::{ColumnDefinition, PrimitiveDataType, TableDefinition};

    use super::*;
    use crate::page::PageType;
    use crate::page_id::PAGE_SIZE;
    use crate::record_serializer;
    use crate::slotted_page_view::SlottedPageView;

    #[test]
    fn test_btree_node_dispatch() {
        let mut buffer = [0u8; PAGE_SIZE];
        let mut page_view = SlottedPageView::new(&mut buffer);
        page_view.initialize(PageType::LeafNode);

        // Act: Wrap the physical view in a logical B-Tree node
        // This is the first goal for your implementation.
        let node = BTreeNode::new(page_view);

        // Assert: Ensure it dispatched to the Leaf variant
        match node {
            BTreeNode::Leaf(_) => assert!(true),
            _ => panic!("Expected LeafNode variant"),
        }
    }

    #[test]
    fn test_leaf_node_binary_search() {
        // 1. Setup Schema: (ID: Int PK, Name: Varchar)
        let mut schema = TableDefinition::new("users".to_string()).unwrap();
        schema.add_column(
            ColumnDefinition::new("id".to_string(), PrimitiveDataType::Int, false, None).unwrap(),
        );
        schema.add_column(
            ColumnDefinition::new(
                "name".to_string(),
                PrimitiveDataType::Varchar(50),
                false,
                None,
            )
            .unwrap(),
        );
        schema.add_constraint(
            Constraint::primary_key("pk".to_string(), vec!["id".to_string()]).unwrap(),
        );

        // 2. Setup Page and Node
        let mut buffer = [0u8; PAGE_SIZE];
        let mut page_view = SlottedPageView::new(&mut buffer);
        page_view.initialize(PageType::LeafNode);

        // 3. Manually add records in sorted order of Key (ID)
        // We use the physical page_view here to set up the data for the logical test.
        let rec0 = record_serializer::serialize(
            &schema.columns,
            &Record::from(vec![
                DataValue::Int(10),
                DataValue::Text("Alice".to_string()),
            ]),
        )
        .unwrap();
        page_view.try_add_record(0, &rec0).unwrap();

        let rec1 = record_serializer::serialize(
            &schema.columns,
            &Record::from(vec![DataValue::Int(30), DataValue::Text("Bob".to_string())]),
        )
        .unwrap();
        page_view.try_add_record(1, &rec1).unwrap();

        // Wrap in Node
        let node = BTreeNode::new(page_view);

        // 4. Act & Assert: Search scenarios on the logical LeafNodeView
        if let BTreeNode::Leaf(leaf_view) = node {
            // Scenario A: Find exact match
            let key_30 = Key::from(DataValue::Int(30));
            // find_key is a method on LeafNodeView, not SlottedPageView!
            assert_eq!(
                Ok(1),
                leaf_view.find_key(&key_30, &schema),
                "Should find key 30 at index 1"
            );

            // Scenario B: Missing key in middle (Gap)
            let key_20 = Key::from(DataValue::Int(20));
            assert_eq!(
                Err(1),
                leaf_view.find_key(&key_20, &schema),
                "Key 20 should be inserted at index 1"
            );

            // Scenario C: Missing key at start
            let key_5 = Key::from(DataValue::Int(5));
            assert_eq!(
                Err(0),
                leaf_view.find_key(&key_5, &schema),
                "Key 5 should be inserted at index 0"
            );

            // Scenario D: Missing key at end
            let key_100 = Key::from(DataValue::Int(100));
            assert_eq!(
                Err(2),
                leaf_view.find_key(&key_100, &schema),
                "Key 100 should be inserted at index 2"
            );
        } else {
            panic!("Node should be a Leaf");
        }
    }
}
