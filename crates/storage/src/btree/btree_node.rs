use crate::{PageId, SlottedPageView, btree::LeafNodeView};

pub enum BTreeNode<'a> {
    Internal(SlottedPageView<'a>),
    Leaf(LeafNodeView<'a>),
}

impl<'a> BTreeNode<'a> {
    pub fn new(page_id: PageId, slotted_page_view: SlottedPageView<'a>) -> Self {
        // TODO: Implement matching based on node type
        let leaf = LeafNodeView::new(page_id, slotted_page_view);
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
        page_view.initialize(PageType::LeafNode, None);

        // Act: Wrap the physical view in a logical B-Tree node
        // This is the first goal for your implementation.
        let node = BTreeNode::new(PageId::new(0, 0), page_view);

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
        page_view.initialize(PageType::LeafNode, None);

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
        page_view.try_insert_record(0, &rec0).unwrap();

        let rec1 = record_serializer::serialize(
            &schema.columns,
            &Record::from(vec![DataValue::Int(30), DataValue::Text("Bob".to_string())]),
        )
        .unwrap();
        page_view.try_insert_record(1, &rec1).unwrap();

        // Wrap in Node
        let node = BTreeNode::new(PageId::new(0, 0), page_view);

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
        page_view.initialize(PageType::LeafNode, None);
        let mut node = BTreeNode::new(PageId::new(0, 0), page_view);

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
        page_view.initialize(PageType::LeafNode, None);
        let mut node = BTreeNode::new(PageId::new(0, 0), page_view);

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

    #[test]
    fn test_leaf_node_update_record() {
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
        page_view.initialize(PageType::LeafNode, None);
        let mut node = BTreeNode::new(PageId::new(0, 0), page_view);

        if let BTreeNode::Leaf(ref mut leaf_view) = node {
            // 3. Insert initial record (ID=10, Name="Alice")
            let initial_rec = Record::from(vec![
                DataValue::Int(10),
                DataValue::Text("Alice".to_string()),
            ]);
            leaf_view
                .insert_record(&initial_rec, &schema)
                .expect("Initial insert should succeed");

            // 4. Construct updated record (Same PK: 10, updated Name: "Alicia")
            let updated_rec = Record::from(vec![
                DataValue::Int(10),
                DataValue::Text("Alicia".to_string()),
            ]);

            // Act
            leaf_view
                .update_record(&updated_rec, &schema)
                .expect("Update should succeed");

            // Assert: Verify only one record remains, but its value has changed
            assert_eq!(1, leaf_view.page_view.get_item_count());
            let val = leaf_view.page_view.get_record(0).unwrap();
            let record = record_serializer::deserialize(&schema.columns, val).unwrap();
            assert_eq!(
                DataValue::Text("Alicia".to_string()),
                record[1],
                "Name should be updated to 'Alicia'"
            );
        } else {
            panic!("Node should be a Leaf");
        }
    }

    #[test]
    fn test_leaf_node_update_record_not_found() {
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
        page_view.initialize(PageType::LeafNode, None);
        let mut node = BTreeNode::new(PageId::new(0, 0), page_view);

        if let BTreeNode::Leaf(ref mut leaf_view) = node {
            // 3. Insert initial record (ID=10, Name="Alice")
            let initial_rec = Record::from(vec![
                DataValue::Int(10),
                DataValue::Text("Alice".to_string()),
            ]);
            leaf_view
                .insert_record(&initial_rec, &schema)
                .expect("Initial insert should succeed");

            // 4. Construct a record with a non-existent key (ID=20)
            let non_existent_rec =
                Record::from(vec![DataValue::Int(20), DataValue::Text("Bob".to_string())]);

            // Act & Assert: Expect KeyNotFound error
            let result = leaf_view.update_record(&non_existent_rec, &schema);
            assert!(
                matches!(result, Err(StorageError::KeyNotFound)),
                "Expected KeyNotFound error when updating a non-existent record"
            );
        } else {
            panic!("Node should be a Leaf");
        }
    }

    #[test]
    fn test_leaf_node_split_and_insert_with_right_sibling() {
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

        // 2. Setup Sibling Page Buffers & IDs
        let left_id = PageId {
            table_id: 1,
            page_index: 10,
        };
        let right_id = PageId {
            table_id: 1,
            page_index: 11,
        };
        let orig_right_id = PageId {
            table_id: 1,
            page_index: 12,
        };

        let mut left_buffer = [0u8; PAGE_SIZE];
        let mut right_buffer = [0u8; PAGE_SIZE];
        let mut orig_right_buffer = [0u8; PAGE_SIZE];

        let mut left_pv = SlottedPageView::new(&mut left_buffer);
        left_pv.initialize(PageType::LeafNode, None);
        let mut left_view = LeafNodeView::new(left_id, left_pv);

        let mut right_pv = SlottedPageView::new(&mut right_buffer);
        right_pv.initialize(PageType::LeafNode, None);
        let mut right_view = LeafNodeView::new(right_id, right_pv);

        let mut orig_right_pv = SlottedPageView::new(&mut orig_right_buffer);
        orig_right_pv.initialize(PageType::LeafNode, None);
        let mut original_right_view = LeafNodeView::new(orig_right_id, orig_right_pv);

        // 3. Establish initial pointer links using raw page indices (u32)
        left_view.set_next_leaf_index(Some(orig_right_id.page_index));
        original_right_view.set_prev_leaf_index(Some(left_id.page_index));

        // 4. Populate with initial data
        left_view
            .insert_record(
                &Record::from(vec![
                    DataValue::Int(10),
                    DataValue::Text("Alice".to_string()),
                ]),
                &schema,
            )
            .unwrap();
        left_view
            .insert_record(
                &Record::from(vec![DataValue::Int(20), DataValue::Text("Bob".to_string())]),
                &schema,
            )
            .unwrap();
        left_view
            .insert_record(
                &Record::from(vec![
                    DataValue::Int(30),
                    DataValue::Text("Charlie".to_string()),
                ]),
                &schema,
            )
            .unwrap();
        left_view
            .insert_record(
                &Record::from(vec![
                    DataValue::Int(40),
                    DataValue::Text("David".to_string()),
                ]),
                &schema,
            )
            .unwrap();

        original_right_view
            .insert_record(
                &Record::from(vec![DataValue::Int(50), DataValue::Text("Eve".to_string())]),
                &schema,
            )
            .unwrap();
        original_right_view
            .insert_record(
                &Record::from(vec![
                    DataValue::Int(60),
                    DataValue::Text("Frank".to_string()),
                ]),
                &schema,
            )
            .unwrap();

        // New record that caused the split
        let rec25 = Record::from(vec![
            DataValue::Int(25),
            DataValue::Text("Grace".to_string()),
        ]);

        // 5. Act: Execute split and insert (Expected to return the separator Key!)
        let separator_key = left_view
            .split_and_insert(
                &rec25,
                &mut right_view,
                Some(&mut original_right_view),
                &schema,
            )
            .expect("Split and insert should succeed");

        // 6. Assert Sibling Pointer Updates using page indices (u32)
        assert_eq!(
            left_view.get_next_leaf_index(),
            Some(right_id.page_index),
            "Left should now point to new Right sibling page index"
        );
        assert_eq!(
            left_view.get_prev_leaf_index(),
            None,
            "Left prev pointer should remain None"
        );

        assert_eq!(
            right_view.get_prev_leaf_index(),
            Some(left_id.page_index),
            "New Right sibling prev should point to Left page index"
        );
        assert_eq!(
            right_view.get_next_leaf_index(),
            Some(orig_right_id.page_index),
            "New Right sibling next should point to Original Right page index"
        );

        assert_eq!(
            original_right_view.get_prev_leaf_index(),
            Some(right_id.page_index),
            "Original Right prev should now point to new Right sibling page index"
        );

        // 7. Assert Data Key Redistribution & Logical Order
        let mut left_keys = Vec::new();
        for i in 0..left_view.page_view.get_item_count() {
            let record_bytes = left_view.page_view.get_record(i).unwrap();
            let key = record_serializer::deserialize_primary_key(&schema, record_bytes).unwrap();
            left_keys.push(key);
        }

        let mut right_keys = Vec::new();
        for i in 0..right_view.page_view.get_item_count() {
            let record_bytes = right_view.page_view.get_record(i).unwrap();
            let key = record_serializer::deserialize_primary_key(&schema, record_bytes).unwrap();
            right_keys.push(key);
        }

        if let (Some(max_left), Some(min_right)) = (left_keys.last(), right_keys.first()) {
            assert!(
                max_left < min_right,
                "Records were not correctly partitioned across split"
            );
        } else {
            panic!("Both split nodes must have at least one record post-split");
        }

        let mut all_keys = Vec::new();
        all_keys.extend(left_keys);
        all_keys.extend(right_keys.clone());

        let expected_keys = vec![
            Key::from(DataValue::Int(10)),
            Key::from(DataValue::Int(20)),
            Key::from(DataValue::Int(25)),
            Key::from(DataValue::Int(30)),
            Key::from(DataValue::Int(40)),
        ];

        assert_eq!(expected_keys, all_keys);

        // 8. Assert Separator Key Integrity
        // The separator key pushed to the parent must match the lowest key in the right sibling node
        assert_eq!(
            right_keys[0], separator_key,
            "The returned separator key must match the first key of the newly split right sibling"
        );
    }

    #[test]
    fn test_leaf_node_split_and_insert_no_right_sibling() {
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

        // 2. Setup Sibling Page Buffers & IDs
        let left_id = PageId {
            table_id: 1,
            page_index: 10,
        };
        let right_id = PageId {
            table_id: 1,
            page_index: 11,
        };

        let mut left_buffer = [0u8; PAGE_SIZE];
        let mut right_buffer = [0u8; PAGE_SIZE];

        let mut left_pv = SlottedPageView::new(&mut left_buffer);
        left_pv.initialize(PageType::LeafNode, None);
        let mut left_view = LeafNodeView::new(left_id, left_pv);

        let mut right_pv = SlottedPageView::new(&mut right_buffer);
        right_pv.initialize(PageType::LeafNode, None);
        let mut right_view = LeafNodeView::new(right_id, right_pv);

        // 3. Establish initial pointer links (No initial right sibling)
        left_view.set_next_leaf_index(None);

        // 4. Populate with initial data
        left_view
            .insert_record(
                &Record::from(vec![
                    DataValue::Int(10),
                    DataValue::Text("Alice".to_string()),
                ]),
                &schema,
            )
            .unwrap();
        left_view
            .insert_record(
                &Record::from(vec![DataValue::Int(20), DataValue::Text("Bob".to_string())]),
                &schema,
            )
            .unwrap();
        left_view
            .insert_record(
                &Record::from(vec![
                    DataValue::Int(30),
                    DataValue::Text("Charlie".to_string()),
                ]),
                &schema,
            )
            .unwrap();
        left_view
            .insert_record(
                &Record::from(vec![
                    DataValue::Int(40),
                    DataValue::Text("David".to_string()),
                ]),
                &schema,
            )
            .unwrap();

        // New record that caused the split
        let rec25 = Record::from(vec![
            DataValue::Int(25),
            DataValue::Text("Grace".to_string()),
        ]);

        // 5. Act: Execute split and insert (Expected to return the separator Key!)
        let separator_key = left_view
            .split_and_insert(&rec25, &mut right_view, None, &schema)
            .expect("Split and insert should succeed");

        // 6. Assert Sibling Pointer Link Updates (Expected: Left <-> Right -> None)
        assert_eq!(left_view.get_next_leaf_index(), Some(right_id.page_index));
        assert_eq!(left_view.get_prev_leaf_index(), None);

        assert_eq!(right_view.get_prev_leaf_index(), Some(left_id.page_index));
        assert_eq!(right_view.get_next_leaf_index(), None);

        // 7. Assert Separator Key Integrity
        let mut right_keys = Vec::new();
        for i in 0..right_view.page_view.get_item_count() {
            let record_bytes = right_view.page_view.get_record(i).unwrap();
            let key = record_serializer::deserialize_primary_key(&schema, record_bytes).unwrap();
            right_keys.push(key);
        }

        assert_eq!(
            right_keys[0], separator_key,
            "The returned separator key must match the first key of the newly split right sibling"
        );
    }

    #[test]
    fn test_leaf_node_merge_with_next_sibling() {
        // 1. Setup Schema
        let mut schema = TableDefinition::new("users".to_string()).unwrap();
        schema.add_column(
            ColumnDefinition::new("id".to_string(), PrimitiveDataType::Int, false, None).unwrap(),
        );
        schema.add_constraint(
            Constraint::primary_key("pk".to_string(), vec!["id".to_string()]).unwrap(),
        );

        // 2. Setup 3 Sibling Nodes: Left (10), Middle (11), Right (12)
        let left_id = PageId {
            table_id: 1,
            page_index: 10,
        };
        let mid_id = PageId {
            table_id: 1,
            page_index: 11,
        };
        let right_id = PageId {
            table_id: 1,
            page_index: 12,
        };

        let mut left_buffer = [0u8; PAGE_SIZE];
        let mut mid_buffer = [0u8; PAGE_SIZE];
        let mut right_buffer = [0u8; PAGE_SIZE];

        let mut left_pv = SlottedPageView::new(&mut left_buffer);
        left_pv.initialize(PageType::LeafNode, None);
        let mut left_view = LeafNodeView::new(left_id, left_pv);

        let mut mid_pv = SlottedPageView::new(&mut mid_buffer);
        mid_pv.initialize(PageType::LeafNode, None);
        let mut mid_view = LeafNodeView::new(mid_id, mid_pv);

        let mut right_pv = SlottedPageView::new(&mut right_buffer);
        right_pv.initialize(PageType::LeafNode, None);
        let mut right_view = LeafNodeView::new(right_id, right_pv);

        // 3. Establish initial pointer links (Left <-> Mid <-> Right)
        left_view.set_next_leaf_index(Some(mid_id.page_index));

        mid_view.set_prev_leaf_index(Some(left_id.page_index));
        mid_view.set_next_leaf_index(Some(right_id.page_index));

        right_view.set_prev_leaf_index(Some(mid_id.page_index));

        // 4. Populate with data
        // Left Node: Keys 10, 20
        left_view
            .insert_record(&Record::from(vec![DataValue::Int(10)]), &schema)
            .unwrap();
        left_view
            .insert_record(&Record::from(vec![DataValue::Int(20)]), &schema)
            .unwrap();

        // Middle Node: Keys 30, 40
        mid_view
            .insert_record(&Record::from(vec![DataValue::Int(30)]), &schema)
            .unwrap();
        mid_view
            .insert_record(&Record::from(vec![DataValue::Int(40)]), &schema)
            .unwrap();

        // Right Node: Keys 50, 60
        right_view
            .insert_record(&Record::from(vec![DataValue::Int(50)]), &schema)
            .unwrap();
        right_view
            .insert_record(&Record::from(vec![DataValue::Int(60)]), &schema)
            .unwrap();

        // 5. Act: Merge Mid (Right sibling) into Left, passing Right as the "next sibling's sibling"
        left_view
            .merge(&mut mid_view, Some(&mut right_view))
            .expect("Merge should succeed");

        // 6. Assert Data Integrity: Left should now contain 10, 20, 30, 40 in order
        assert_eq!(
            4,
            left_view.page_view.get_item_count(),
            "Left leaf should now contain combined record count"
        );

        let mut left_keys = Vec::new();
        for i in 0..left_view.page_view.get_item_count() {
            let record_bytes = left_view.page_view.get_record(i).unwrap();
            let key = record_serializer::deserialize_primary_key(&schema, record_bytes).unwrap();
            left_keys.push(key);
        }

        let expected_keys = vec![
            Key::from(DataValue::Int(10)),
            Key::from(DataValue::Int(20)),
            Key::from(DataValue::Int(30)),
            Key::from(DataValue::Int(40)),
        ];
        assert_eq!(expected_keys, left_keys);

        // 7. Assert Sibling Pointer Realignment (Left <-> Right, Mid is bypass-linked out)
        assert_eq!(
            left_view.get_next_leaf_index(),
            Some(right_id.page_index),
            "Left should now point directly to Right"
        );
        assert_eq!(
            right_view.get_prev_leaf_index(),
            Some(left_id.page_index),
            "Right should now point back directly to Left"
        );

        // 8. Assert Evicted Sibling is clean / initialized
        assert_eq!(
            0,
            mid_view.page_view.get_item_count(),
            "Merged-away node should be cleared of records"
        );
        assert_eq!(
            None,
            mid_view.get_next_leaf_index(),
            "Merged-away node next leaf index should be cleared"
        );
        assert_eq!(
            None,
            mid_view.get_prev_leaf_index(),
            "Merged-away node prev leaf index should be cleared"
        );
    }

    #[test]
    fn test_leaf_node_merge_no_next_sibling() {
        // 1. Setup Schema
        let mut schema = TableDefinition::new("users".to_string()).unwrap();
        schema.add_column(
            ColumnDefinition::new("id".to_string(), PrimitiveDataType::Int, false, None).unwrap(),
        );
        schema.add_constraint(
            Constraint::primary_key("pk".to_string(), vec!["id".to_string()]).unwrap(),
        );

        // 2. Setup 2 Sibling Nodes at the right boundary of the leaf level: Left (10), Right (11)
        let left_id = PageId {
            table_id: 1,
            page_index: 10,
        };
        let right_id = PageId {
            table_id: 1,
            page_index: 11,
        };

        let mut left_buffer = [0u8; PAGE_SIZE];
        let mut right_buffer = [0u8; PAGE_SIZE];

        let mut left_pv = SlottedPageView::new(&mut left_buffer);
        left_pv.initialize(PageType::LeafNode, None);
        let mut left_view = LeafNodeView::new(left_id, left_pv);

        let mut right_pv = SlottedPageView::new(&mut right_buffer);
        right_pv.initialize(PageType::LeafNode, None);
        let mut right_view = LeafNodeView::new(right_id, right_pv);

        // 3. Establish initial pointer links (Left <-> Right -> None)
        left_view.set_next_leaf_index(Some(right_id.page_index));
        right_view.set_prev_leaf_index(Some(left_id.page_index));
        right_view.set_next_leaf_index(None);

        // 4. Populate with data
        left_view
            .insert_record(&Record::from(vec![DataValue::Int(10)]), &schema)
            .unwrap();
        left_view
            .insert_record(&Record::from(vec![DataValue::Int(20)]), &schema)
            .unwrap();

        right_view
            .insert_record(&Record::from(vec![DataValue::Int(30)]), &schema)
            .unwrap();
        right_view
            .insert_record(&Record::from(vec![DataValue::Int(40)]), &schema)
            .unwrap();

        // 5. Act: Merge Right into Left, passing None as there is no further right sibling
        left_view
            .merge(&mut right_view, None)
            .expect("Merge should succeed");

        // 6. Assert Data Integrity
        assert_eq!(4, left_view.page_view.get_item_count());

        // 7. Assert Sibling Pointer Realignment (Left -> None)
        assert_eq!(
            left_view.get_next_leaf_index(),
            None,
            "Left should have no next sibling now"
        );
        assert_eq!(left_view.get_prev_leaf_index(), None);
    }

    #[test]
    fn test_leaf_node_merge_basic_coalesce() {
        // 1. Setup Table Schema
        let mut schema = TableDefinition::new("customers".to_string()).unwrap();
        schema.add_column(
            ColumnDefinition::new("id".to_string(), PrimitiveDataType::Int, false, None).unwrap(),
        );
        schema.add_constraint(
            Constraint::primary_key("pk".to_string(), vec!["id".to_string()]).unwrap(),
        );

        // 2. Allocate 2 Sibling Leaf Pages (Left index 10, Right index 11)
        let left_id = PageId {
            table_id: 1,
            page_index: 10,
        };
        let right_id = PageId {
            table_id: 1,
            page_index: 11,
        };

        let mut left_buffer = [0u8; PAGE_SIZE];
        let mut right_buffer = [0u8; PAGE_SIZE];

        let mut left_pv = SlottedPageView::new(&mut left_buffer);
        left_pv.initialize(PageType::LeafNode, None);
        let mut left_view = LeafNodeView::new(left_id, left_pv);

        let mut right_pv = SlottedPageView::new(&mut right_buffer);
        right_pv.initialize(PageType::LeafNode, None);
        let mut right_view = LeafNodeView::new(right_id, right_pv);

        // 3. Establish initial pointer links (Left points to Right, Right points back to Left)
        left_view.set_next_leaf_index(Some(right_id.page_index));
        right_view.set_prev_leaf_index(Some(left_id.page_index));
        right_view.set_next_leaf_index(None); // Rightmost boundary

        // 4. Populate with initial record data
        // Left leaf gets Keys: 10, 20
        left_view
            .insert_record(&Record::from(vec![DataValue::Int(10)]), &schema)
            .unwrap();
        left_view
            .insert_record(&Record::from(vec![DataValue::Int(20)]), &schema)
            .unwrap();

        // Right leaf gets Keys: 30, 40 (Simulating an underflow state due to deletions)
        right_view
            .insert_record(&Record::from(vec![DataValue::Int(30)]), &schema)
            .unwrap();
        right_view
            .insert_record(&Record::from(vec![DataValue::Int(40)]), &schema)
            .unwrap();

        // 5. Act: Execute the merge. We pass None because there is no sibling past the right node.
        left_view
            .merge(&mut right_view, None)
            .expect("Basic leaf node merge should succeed");

        // 6. Assert Data Integrity: Left view must now contain all 4 records in perfect order
        assert_eq!(
            4,
            left_view.page_view.get_item_count(),
            "Left node should contain the combined item count"
        );

        let mut resulting_keys = Vec::new();
        for i in 0..left_view.page_view.get_item_count() {
            let record_bytes = left_view.page_view.get_record(i).unwrap();
            let key = record_serializer::deserialize_primary_key(&schema, record_bytes).unwrap();
            resulting_keys.push(key);
        }

        let expected_keys = vec![
            Key::from(DataValue::Int(10)),
            Key::from(DataValue::Int(20)),
            Key::from(DataValue::Int(30)),
            Key::from(DataValue::Int(40)),
        ];
        assert_eq!(
            expected_keys, resulting_keys,
            "Records from right sibling were not correctly appended in order"
        );

        // 7. Assert Sibling Pointer Realignment
        assert_eq!(
            left_view.get_next_leaf_index(),
            None,
            "Left next pointer should skip over right node and be None"
        );
        assert_eq!(
            left_view.get_prev_leaf_index(),
            None,
            "Left prev pointer should remain unchanged"
        );

        // 8. Assert Reclaimed Page Cleanness: The right view must be completely cleared out
        assert_eq!(
            0,
            right_view.page_view.get_item_count(),
            "The right page should contain zero entries post-merge"
        );
        assert_eq!(
            None,
            right_view.get_next_leaf_index(),
            "Right page next index pointer should be reset"
        );
        assert_eq!(
            None,
            right_view.get_prev_leaf_index(),
            "Right page prev index pointer should be reset"
        );
    }

    #[test]
    fn test_leaf_node_merge_with_trailing_right_sibling() {
        let mut schema = TableDefinition::new("customers".to_string()).unwrap();
        schema.add_column(
            ColumnDefinition::new("id".to_string(), PrimitiveDataType::Int, false, None).unwrap(),
        );
        schema.add_constraint(
            Constraint::primary_key("pk".to_string(), vec!["id".to_string()]).unwrap(),
        );

        let left_id = PageId {
            table_id: 1,
            page_index: 10,
        };
        let mid_id = PageId {
            table_id: 1,
            page_index: 11,
        };
        let trailing_id = PageId {
            table_id: 1,
            page_index: 12,
        };

        let mut left_buffer = [0u8; PAGE_SIZE];
        let mut mid_buffer = [0u8; PAGE_SIZE];
        let mut trailing_buffer = [0u8; PAGE_SIZE];

        let mut left_pv = SlottedPageView::new(&mut left_buffer);
        left_pv.initialize(PageType::LeafNode, None);
        let mut left_view = LeafNodeView::new(left_id, left_pv);

        let mut mid_pv = SlottedPageView::new(&mut mid_buffer);
        mid_pv.initialize(PageType::LeafNode, None);
        let mut mid_view = LeafNodeView::new(mid_id, mid_pv);

        let mut trailing_pv = SlottedPageView::new(&mut trailing_buffer);
        trailing_pv.initialize(PageType::LeafNode, None);
        let mut trailing_view = LeafNodeView::new(trailing_id, trailing_pv);

        left_view.set_next_leaf_index(Some(mid_id.page_index));

        mid_view.set_prev_leaf_index(Some(left_id.page_index));
        mid_view.set_next_leaf_index(Some(trailing_id.page_index));

        trailing_view.set_prev_leaf_index(Some(mid_id.page_index));
        trailing_view.set_next_leaf_index(None);

        left_view
            .insert_record(&Record::from(vec![DataValue::Int(10)]), &schema)
            .unwrap();
        left_view
            .insert_record(&Record::from(vec![DataValue::Int(20)]), &schema)
            .unwrap();

        mid_view
            .insert_record(&Record::from(vec![DataValue::Int(30)]), &schema)
            .unwrap();
        mid_view
            .insert_record(&Record::from(vec![DataValue::Int(40)]), &schema)
            .unwrap();

        trailing_view
            .insert_record(&Record::from(vec![DataValue::Int(50)]), &schema)
            .unwrap();
        trailing_view
            .insert_record(&Record::from(vec![DataValue::Int(60)]), &schema)
            .unwrap();

        // Act: Execute merge without the schema parameter
        left_view
            .merge(&mut mid_view, Some(&mut trailing_view))
            .expect("Leaf node merge with trailing sibling should succeed");

        assert_eq!(4, left_view.page_view.get_item_count());
        let mut left_keys = Vec::new();
        for i in 0..left_view.page_view.get_item_count() {
            let record_bytes = left_view.page_view.get_record(i).unwrap();
            let key = record_serializer::deserialize_primary_key(&schema, record_bytes).unwrap();
            left_keys.push(key);
        }
        assert_eq!(
            vec![
                Key::from(DataValue::Int(10)),
                Key::from(DataValue::Int(20)),
                Key::from(DataValue::Int(30)),
                Key::from(DataValue::Int(40))
            ],
            left_keys
        );

        assert_eq!(
            left_view.get_next_leaf_index(),
            Some(trailing_id.page_index)
        );
        assert_eq!(
            trailing_view.get_prev_leaf_index(),
            Some(left_id.page_index)
        );

        assert_eq!(0, mid_view.page_view.get_item_count());
        assert_eq!(None, mid_view.get_next_leaf_index());
        assert_eq!(None, mid_view.get_prev_leaf_index());

        assert_eq!(2, trailing_view.page_view.get_item_count());
    }
}
