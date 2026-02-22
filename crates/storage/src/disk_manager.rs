use std::io::Result;
use std::{path::PathBuf, sync::Arc};

use crate::FileSystem;

pub const TABLE_FILE_EXTENSION: &str = "tbl";

pub struct DiskManager {
    base_path: PathBuf,
    file_system: Arc<dyn FileSystem>,
}

impl DiskManager {
    pub fn new(fs: Arc<dyn FileSystem>, path: PathBuf) -> Self {
        DiskManager {
            base_path: path,
            file_system: fs,
        }
    }

    pub async fn create_table_file(&self, table_id: u32) -> Result<()> {
        // Make sure the base path exists before we attempt to create a table file...
        self.file_system.create_dir_all(&self.base_path).await?;

        let path = self
            .base_path
            .join(table_id.to_string())
            .with_added_extension(TABLE_FILE_EXTENSION);

        // Now create the new table file.
        self.file_system.create_file(&path).await?;
        Ok(())
    }

    pub async fn table_file_exists(&self, table_id: u32) -> Result<bool> {
        let path = self
            .base_path
            .join(table_id.to_string())
            .with_added_extension(TABLE_FILE_EXTENSION);
        self.file_system.file_exists(&path).await
    }
}

#[cfg(test)]
mod tests {
    use super::*; // You will implement DiskManager and PageId above this module
    use crate::file_system::TokioFileSystem;
    use std::sync::Arc;
    use tempfile::tempdir;

    #[tokio::test]
    async fn test_create_and_check_table_file() {
        let dir = tempdir().unwrap();
        let base_path = dir.path().to_path_buf();

        // Wrapping the FileSystem in an Arc is standard practice so it can be
        // shared safely if DiskManager is cloned or shared across threads.
        let fs = Arc::new(TokioFileSystem::new());

        // Act: Initialize the DiskManager
        let disk_manager = DiskManager::new(fs, base_path);
        let table_id = 101;

        // Assert 1: File should not exist initially
        let exists_initially = disk_manager
            .table_file_exists(table_id)
            .await
            .expect("Should check if table exists without error");

        assert!(!exists_initially, "Table file should not exist initially");

        // Act: Create the table file
        disk_manager
            .create_table_file(table_id)
            .await
            .expect("Should successfully create table file");

        // Assert 2: File should exist after creation
        let exists_now = disk_manager
            .table_file_exists(table_id)
            .await
            .expect("Should check if table exists without error");

        assert!(exists_now, "Table file should exist after creation");
    }

    #[tokio::test]
    async fn test_create_table_file_creates_base_path_if_missing() {
        let dir = tempdir().unwrap();
        // Create a path that explicitly does not exist yet
        let base_path = dir.path().join("non_existent_folder").join("data");

        let fs = Arc::new(TokioFileSystem::new());
        let disk_manager = DiskManager::new(fs, base_path.clone());
        let table_id = 202;

        // Act: Create the table file
        disk_manager
            .create_table_file(table_id)
            .await
            .expect("Should successfully create table file even if base path was missing");

        // Assert: Verify the file exists
        let exists = disk_manager
            .table_file_exists(table_id)
            .await
            .expect("Should check if table exists without error");

        assert!(
            exists,
            "Table file should exist, meaning the missing directories were created"
        );
    }
}
