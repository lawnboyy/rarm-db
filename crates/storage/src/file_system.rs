use async_trait::async_trait;
use std::io::{Result, SeekFrom};
use std::path::Path;
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
            .create(true)
            .truncate(true)
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
}
