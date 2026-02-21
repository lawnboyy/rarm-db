use async_trait::async_trait;
use std::io::{Result, SeekFrom};
use std::path::Path;
use tokio::fs;
use tokio::io::AsyncReadExt;
use tokio::sync::Mutex;
use tokio::{
    fs::{File, OpenOptions},
    io::{AsyncSeekExt, AsyncWriteExt},
};

/// Abstract interface for file system operations.
#[async_trait]
pub trait FileSystem: Send + Sync {
    /// Creates a new file (or overwrites existing) for reading and writing.
    async fn create_file(&self, path: &Path) -> Result<Box<dyn FileHandle>>;

    async fn delete_file(&self, path: &Path) -> Result<()>;

    async fn file_exists(&self, path: &Path) -> Result<bool>;

    /// Opens a file at the given path.
    async fn open_file(&self, path: &Path) -> Result<Box<dyn FileHandle>>;
}

/// Abstract interface for operations on an open file.
#[async_trait]
pub trait FileHandle: Send + Sync {
    // Methods will be added as needed by tests.
    async fn write_at(&self, data: &[u8], offset: u64) -> Result<()>;
    async fn read_at(&self, buffer: &mut [u8], offset: u64) -> Result<usize>;
}

/// Concrete implementation of FileSystem using tokio::fs.
pub struct TokioFileSystem;

impl TokioFileSystem {
    pub fn new() -> Self {
        Self {}
    }
}

#[async_trait]
impl FileSystem for TokioFileSystem {
    async fn create_file(&self, path: &Path) -> Result<Box<dyn FileHandle>> {
        let file = OpenOptions::new()
            .read(true)
            .write(true)
            .create_new(true)
            .open(path)
            .await?;
        let handle = TokioFileHandle::new(file);

        Ok(Box::new(handle))
    }

    async fn delete_file(&self, path: &Path) -> Result<()> {
        fs::remove_file(path).await
    }

    async fn file_exists(&self, path: &Path) -> Result<bool> {
        fs::try_exists(path).await
    }

    async fn open_file(&self, path: &Path) -> Result<Box<dyn FileHandle>> {
        let file = OpenOptions::new()
            .read(true)
            .write(true)
            .create(false)
            .truncate(false)
            .open(path)
            .await?;
        let handle = TokioFileHandle::new(file);

        Ok(Box::new(handle))
    }
}

/// Concrete implementation of FileHandle wrapping a tokio::fs::File.
pub struct TokioFileHandle {
    // kept private to enforce encapsulation via trait
    _file: Mutex<File>,
}

impl TokioFileHandle {
    pub fn new(file: File) -> Self {
        Self {
            _file: Mutex::new(file),
        }
    }
}

#[async_trait]
impl FileHandle for TokioFileHandle {
    async fn write_at(&self, data: &[u8], offset: u64) -> Result<()> {
        // Acquire the file lock. This yields a "MutexGuard" which acts like &mut File.
        // This allows us to perform stateful operations (moving the cursor) safely.
        let mut file_guard = self._file.lock().await;

        // Seek to the file offset
        file_guard.seek(SeekFrom::Start(offset)).await?;

        // Write the entire buffer at the current position
        file_guard.write_all(data).await?;

        // file_guard goes out of scope and will release the lock
        Ok({})
    }

    async fn read_at(&self, buffer: &mut [u8], offset: u64) -> Result<usize> {
        // Acquire the file lock. This yields a "MutexGuard" which acts like &mut File.
        // This allows us to perform stateful operations (moving the cursor) safely.
        let mut file_guard = self._file.lock().await;

        // Seek to the file offset
        file_guard.seek(SeekFrom::Start(offset)).await?;

        let bytes_read = file_guard.read(buffer).await?;

        Ok(bytes_read)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir; // Requires: cargo add tempfile --dev -p rarmdb_storage

    #[tokio::test]
    async fn test_create_file() {
        let dir = tempdir().unwrap();
        let file_path = dir.path().join("test_create.db");
        let fs = TokioFileSystem::new();

        // Act
        let _handle = fs
            .create_file(&file_path)
            .await
            .expect("Failed to create file");

        // Assert
        assert!(
            file_path.exists(),
            "File should exist on disk after creation"
        );
    }

    #[tokio::test]
    async fn test_write_to_file() {
        let dir = tempdir().unwrap();
        let file_path = dir.path().join("test_write.db");
        let fs = TokioFileSystem::new();

        let handle = fs
            .create_file(&file_path)
            .await
            .expect("Should create file");

        let data = b"Hello";
        handle
            .write_at(data, 0)
            .await
            .expect("Should write to file");

        // Use read_at to verify data was written, eliminating the need for sync_all
        let mut buffer = [0u8; 5];
        let bytes_read = handle
            .read_at(&mut buffer, 0)
            .await
            .expect("Should read from file");

        assert_eq!(bytes_read, 5);
        assert_eq!(&buffer, data);
    }

    #[tokio::test]
    async fn test_read_at_specific_offset() {
        let dir = tempdir().unwrap();
        let file_path = dir.path().join("test_read.db");
        let fs = TokioFileSystem::new();

        let handle = fs
            .create_file(&file_path)
            .await
            .expect("Should create file");

        // Setup: Write a 10-byte payload
        handle
            .write_at(b"HelloWorld", 0)
            .await
            .expect("Should write to file");

        // Act: Read 5 bytes starting at offset 5
        let mut buffer = [0u8; 5];
        let bytes_read = handle
            .read_at(&mut buffer, 5)
            .await
            .expect("Should read from file");

        // Assert: We should get the second half of the payload
        assert_eq!(bytes_read, 5);
        assert_eq!(&buffer, b"World");
    }

    #[tokio::test]
    async fn test_open_existing_file() {
        let dir = tempdir().unwrap();
        let file_path = dir.path().join("test_open.db");
        let fs = TokioFileSystem::new();

        // Setup: Create a file, write to it, and explicitly drop the handle to close it
        {
            let handle = fs
                .create_file(&file_path)
                .await
                .expect("Should create file");
            handle
                .write_at(b"PersistentData", 0)
                .await
                .expect("Should write to file");
        } // The handle goes out of scope here, releasing the file lock

        // Act: Open the existing file
        // Note: You will need to add the `open_file` method to the FileSystem trait!
        let handle2 = fs
            .open_file(&file_path)
            .await
            .expect("Should open existing file");

        // Assert: Verify we can read the previously written data
        let mut buffer = [0u8; 14];
        let bytes_read = handle2
            .read_at(&mut buffer, 0)
            .await
            .expect("Should read from opened file");

        assert_eq!(bytes_read, 14);
        assert_eq!(&buffer, b"PersistentData");
    }

    #[tokio::test]
    async fn test_create_existing_file_fails() {
        let dir = tempdir().unwrap();
        let file_path = dir.path().join("test_create_existing.db");
        let fs = TokioFileSystem::new();

        // 1. Create the file initially
        fs.create_file(&file_path)
            .await
            .expect("First creation should succeed");

        // 2. Attempt to create the same file again
        let result = fs.create_file(&file_path).await;

        // 3. Verify it returns an error and specifically an AlreadyExists error
        match result {
            Err(e) => assert_eq!(e.kind(), std::io::ErrorKind::AlreadyExists),
            Ok(_) => panic!("Second creation attempt should have failed, but it succeeded!"),
        }
    }

    #[tokio::test]
    async fn test_open_missing_file_fails() {
        let dir = tempdir().unwrap();
        let file_path = dir.path().join("test_open_missing.db");
        let fs = TokioFileSystem::new();

        // Act: Attempt to open a file that was never created
        let result = fs.open_file(&file_path).await;

        // Assert: Verify it returns an error and specifically a NotFound error
        match result {
            Err(e) => assert_eq!(e.kind(), std::io::ErrorKind::NotFound),
            Ok(_) => panic!("Opening a missing file should have failed, but it succeeded!"),
        }
    }

    #[tokio::test]
    async fn test_file_exists_returns_correct_status() {
        let dir = tempdir().unwrap();
        let file_path = dir.path().join("test_exists.db");
        let fs = TokioFileSystem::new();

        // Act & Assert 1: File doesn't exist yet
        let exists_initially = fs
            .file_exists(&file_path)
            .await
            .expect("Should check existence without error");
        assert!(!exists_initially, "File should not exist yet");

        // Setup: Create the file
        fs.create_file(&file_path)
            .await
            .expect("Should create file");

        // Act & Assert 2: File now exists
        let exists_now = fs
            .file_exists(&file_path)
            .await
            .expect("Should check existence without error");
        assert!(exists_now, "File should exist after creation");
    }

    #[tokio::test]
    async fn test_delete_file_removes_file_from_disk() {
        let dir = tempdir().unwrap();
        let file_path = dir.path().join("test_delete.db");
        let fs = TokioFileSystem::new();

        // Setup: Create a file to delete
        fs.create_file(&file_path)
            .await
            .expect("Should create file");
        assert!(file_path.exists(), "File should exist before deletion");

        // Act: Delete the file
        fs.delete_file(&file_path)
            .await
            .expect("Should delete file without error");

        // Assert: Verify it no longer exists
        assert!(!file_path.exists(), "File should be removed from disk");
    }
}
