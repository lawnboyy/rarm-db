use std::collections::HashMap;
use std::io::Result;
use std::sync::RwLock;
use std::{path::PathBuf, sync::Arc};

use crate::file_system::FileHandle;
use crate::page_id::PAGE_SIZE;
use crate::{FileSystem, PageId};

pub const TABLE_FILE_EXTENSION: &str = "tbl";

pub struct DiskManager {
    base_path: PathBuf,
    file_handles: RwLock<HashMap<u32, Arc<dyn FileHandle>>>,
    file_system: Arc<dyn FileSystem>,
}

impl DiskManager {
    pub fn new(fs: Arc<dyn FileSystem>, path: PathBuf) -> Self {
        DiskManager {
            base_path: path,
            file_handles: RwLock::new(HashMap::new()),
            file_system: fs,
        }
    }

    /// Allocates a new page for the table file associated with the given
    /// ID. Writes out a zeroed out page sized buffer to initialize the
    /// page.
    pub async fn allocate_page(&self, table_id: u32) -> Result<PageId> {
        // Look up the file handle for the given table ID...
        let file_handle = self.get_file_handle(table_id).await?;
        // Determine the next page index using the file length and page size.
        let file_length = file_handle.len().await?;
        let next_index = file_length / PAGE_SIZE as u64;

        let zero_buffer = [0u8; PAGE_SIZE];
        file_handle
            .write_at(&zero_buffer, next_index * PAGE_SIZE as u64)
            .await?;

        let page_index: u32 = next_index
            .try_into()
            .expect("Database table exceeded the maximum table size!");

        Ok(PageId {
            table_id,
            page_index,
        })
    }

    pub async fn create_table_file(&self, table_id: u32) -> Result<()> {
        // Make sure the base path exists before we attempt to create a table file...
        self.file_system.create_dir_all(&self.base_path).await?;

        let path = self
            .base_path
            .join(table_id.to_string())
            .with_extension(TABLE_FILE_EXTENSION);

        // Now create the new table file.
        let file_handle = self.file_system.create_file(&path).await?;

        // Add file handle to the hashmap cache
        // Get a write lock on the file handle cache.
        let mut cache = self.file_handles.write().unwrap();
        // Convert our file handle box to an arc so we can insert it into the cache...
        let arc_handle = file_handle.into();
        cache.insert(table_id, Arc::clone(&arc_handle));

        Ok(())
    }

    async fn get_file_handle(&self, table_id: u32) -> Result<Arc<dyn FileHandle>> {
        // First check if we have a file handle for the table ID in our cache.
        // Wrapped in an inner scope to release the lock if we have a cache miss.
        {
            let cache = self.file_handles.read().unwrap();
            if let Some(file_handle) = cache.get(&table_id) {
                return Ok(Arc::clone(file_handle));
            }
        }

        // If we get here, then we had a cache miss, so we will open the file to get a handle. There is
        // a chance that another thread has already opened this file and added it to the cache. But
        // we have to do this prior to getting a write lock because the standard RwLock cannot be
        // held across async boundaries and the open_file call is async. After opening the file,
        // if it already exists in the cache, we'll throw this handle away. It's slightly inefficient
        // but simplest approach.
        let path = self
            .base_path
            .join(table_id.to_string())
            .with_extension(TABLE_FILE_EXTENSION);
        let file_handle = self.file_system.open_file(&path).await?;
        let arc_handle = Arc::from(file_handle);

        // Now we acquire a write lock to update the cache.
        let mut cache = self.file_handles.write().unwrap();

        // It's possible another thread has opened the file and added the handle to the cache
        // while we were waiting on the lock, so check the cache again.
        if let Some(cached_handle) = cache.get(&table_id) {
            return Ok(Arc::clone(cached_handle));
        }

        // It's still not in the cache and we have an exclusive write lock, so we can add the
        // file handle to the cache now.
        cache.insert(table_id, Arc::clone(&arc_handle));
        Ok(arc_handle)
    }

    pub async fn read_page(&self, page_id: PageId, buffer: &mut [u8; PAGE_SIZE]) -> Result<()> {
        // First get the file handle for this table from the page ID...
        let file_handle = self.get_file_handle(page_id.table_id).await?;

        file_handle
            .read_at(buffer, page_id.page_index as u64 * PAGE_SIZE as u64)
            .await?;

        Ok(())
    }

    pub async fn table_file_exists(&self, table_id: u32) -> Result<bool> {
        let path = self
            .base_path
            .join(table_id.to_string())
            .with_extension(TABLE_FILE_EXTENSION);
        self.file_system.file_exists(&path).await
    }

    pub async fn write_page(&self, page_id: PageId, buffer: &[u8; PAGE_SIZE]) -> Result<()> {
        // First get the file handle for this table from the page ID...
        let file_handle = self.get_file_handle(page_id.table_id).await?;

        file_handle
            .write_at(buffer, page_id.page_index as u64 * PAGE_SIZE as u64)
            .await?;

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*; // You will implement DiskManager and PageId above this module
    use crate::{PageId, file_system::TokioFileSystem, page_id::PAGE_SIZE};
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

    #[tokio::test]
    async fn test_write_and_read_page() {
        let dir = tempdir().unwrap();
        let fs = Arc::new(TokioFileSystem::new());
        let disk_manager = DiskManager::new(fs, dir.path().to_path_buf());
        let table_id = 303;

        // Setup: Create the table
        disk_manager
            .create_table_file(table_id)
            .await
            .expect("Should create table");

        // Note: You will need to define `PageId` and `PAGE_SIZE` (e.g., 4096)
        let page_id = PageId {
            table_id,
            page_index: 0,
        };

        // Create a distinct 4KB payload to write
        let mut write_buffer = [0u8; PAGE_SIZE];
        write_buffer[0] = 42;
        write_buffer[PAGE_SIZE - 1] = 99;

        // Act 1: Write the page
        disk_manager
            .write_page(page_id, &write_buffer)
            .await
            .expect("Should write page without error");

        // Act 2: Read the page back
        let mut read_buffer = [0u8; PAGE_SIZE];
        disk_manager
            .read_page(page_id, &mut read_buffer)
            .await
            .expect("Should read page without error");

        // Assert: The data read back should perfectly match the data written
        assert_eq!(
            write_buffer.as_ref(),
            read_buffer.as_ref(),
            "Data read from disk should match data written to disk"
        );
    }

    #[tokio::test]
    async fn test_allocate_page_assigns_sequential_page_ids() {
        let dir = tempdir().unwrap();
        let fs = Arc::new(TokioFileSystem::new());
        let disk_manager = DiskManager::new(
            Arc::clone(&fs) as Arc<dyn FileSystem>,
            dir.path().to_path_buf(),
        );
        let table_id = 404;

        // Setup: Create the table
        disk_manager
            .create_table_file(table_id)
            .await
            .expect("Should create table");

        // Act 1: Allocate the first page
        let page0 = disk_manager
            .allocate_page(table_id)
            .await
            .expect("Should allocate first page");

        // Assert 1: Should be page index 0
        assert_eq!(
            0, page0.page_index,
            "First allocated page should be index 0"
        );
        assert_eq!(table_id, page0.table_id);

        // Act 2: Allocate the second page
        let page1 = disk_manager
            .allocate_page(table_id)
            .await
            .expect("Should allocate second page");

        // Assert 2: Should be page index 1
        assert_eq!(
            1, page1.page_index,
            "Second allocated page should be index 1"
        );

        // Assert 3: Verify the file actually grew on disk (2 pages * 8192 bytes = 16384 bytes)
        let path = dir.path().join(format!("{}.tbl", table_id));
        let handle = fs.open_file(&path).await.expect("File should exist");
        let file_len = handle.len().await.expect("Should get file length");

        assert_eq!(
            (PAGE_SIZE * 2) as u64,
            file_len,
            "File length should equal exactly 2 pages"
        );
    }

    #[tokio::test]
    async fn test_allocate_page_zero_fills_new_page() {
        let dir = tempdir().unwrap();
        let fs = Arc::new(TokioFileSystem::new());
        let disk_manager = DiskManager::new(fs, dir.path().to_path_buf());
        let table_id = 505;

        // Setup: Create the table
        disk_manager
            .create_table_file(table_id)
            .await
            .expect("Should create table");

        // Act: Allocate a page
        let page_id = disk_manager
            .allocate_page(table_id)
            .await
            .expect("Should allocate page");

        // Assert: Read the page back and verify it's entirely zeros
        // We initialize with 1s to ensure the read actually overwrites our buffer with 0s
        let mut read_buffer = [1u8; PAGE_SIZE];
        disk_manager
            .read_page(page_id, &mut read_buffer)
            .await
            .expect("Should read allocated page");

        let expected_buffer = [0u8; PAGE_SIZE];
        assert_eq!(
            expected_buffer.as_ref(),
            read_buffer.as_ref(),
            "Newly allocated page should be zero-filled"
        );
    }
}
