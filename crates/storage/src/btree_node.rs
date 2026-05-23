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
    use crate::slotted_page_view::SlottedPageView;
    use crate::{StorageError, record_serializer};

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

    #[test]
    fn test_leaf_node_insert_record() {
        // 1. Setup Schema
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
        let mut node = BTreeNode::new(page_view);

        if let BTreeNode::Leaf(ref mut leaf_view) = node {
            // 3. Insert records in UNSORTED arrival order
            // Insert ID 30
            let rec30 = Record::from(vec![DataValue::Int(30), DataValue::Text("Bob".to_string())]);
            leaf_view
                .insert_record(&rec30, &schema)
                .expect("Insert 30 should succeed");

            // Insert ID 10 (Should go to index 0, shifting 30 to index 1)
            let rec10 = Record::from(vec![
                DataValue::Int(10),
                DataValue::Text("Alice".to_string()),
            ]);
            leaf_view
                .insert_record(&rec10, &schema)
                .expect("Insert 10 should succeed");

            // Insert ID 20 (Should go to index 1, shifting 30 to index 2)
            let rec20 = Record::from(vec![
                DataValue::Int(20),
                DataValue::Text("Charlie".to_string()),
            ]);
            leaf_view
                .insert_record(&rec20, &schema)
                .expect("Insert 20 should succeed");

            // 4. Assert Logical Sorted Order
            assert_eq!(3, leaf_view.page_view.get_item_count());

            // Index 0: ID 10
            let val0 = leaf_view.page_view.get_record(0).unwrap();
            let key0 = record_serializer::deserialize_primary_key(&schema, val0).unwrap();
            assert_eq!(Key::from(DataValue::Int(10)), key0);

            // Index 1: ID 20
            let val1 = leaf_view.page_view.get_record(1).unwrap();
            let key1 = record_serializer::deserialize_primary_key(&schema, val1).unwrap();
            assert_eq!(Key::from(DataValue::Int(20)), key1);

            // Index 2: ID 30
            let val2 = leaf_view.page_view.get_record(2).unwrap();
            let key2 = record_serializer::deserialize_primary_key(&schema, val2).unwrap();
            assert_eq!(Key::from(DataValue::Int(30)), key2);

            // 5. Assert Duplicate Key Error
            let duplicate = Record::from(vec![
                DataValue::Int(20),
                DataValue::Text("Clone".to_string()),
            ]);
            let result = leaf_view.insert_record(&duplicate, &schema);
            assert!(matches!(result, Err(StorageError::DuplicateKey)));
        } else {
            panic!("Node should be a Leaf");
        }
    }

    #[test]
    fn test_leaf_node_insert_page_full() {
        // 1. Setup Schema
        let mut schema = TableDefinition::new("users".to_string()).unwrap();
        schema.add_column(
            ColumnDefinition::new("id".to_string(), PrimitiveDataType::Int, false, None).unwrap(),
        );
        schema.add_column(
            ColumnDefinition::new(
                "bio".to_string(),
                PrimitiveDataType::Varchar(8000),
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
        let mut node = BTreeNode::new(page_view);

        if let BTreeNode::Leaf(ref mut leaf_view) = node {
            // 3. Construct a giant record that takes up almost the entire page capacity
            // Page size is 8192. Header is 32. Max remaining is 8160.
            let giant_bio = "X".repeat(8100);
            let giant_record = Record::from(vec![DataValue::Int(1), DataValue::Text(giant_bio)]);

            leaf_view
                .insert_record(&giant_record, &schema)
                .expect("Giant record should fit on empty page");

            // 4. Try to insert another small record. It should fail with PageFull.
            let small_record =
                Record::from(vec![DataValue::Int(2), DataValue::Text("Y".repeat(50))]);
            let result = leaf_view.insert_record(&small_record, &schema);

            // Assert: `try_add_record` should return `PageFull`, and `insert_record` must bubble it up.
            assert!(
                matches!(result, Err(StorageError::PageFull)),
                "Expected PageFull error when inserting into a saturated leaf node"
            );
        } else {
            panic!("Node should be a Leaf");
        }
    }
}
