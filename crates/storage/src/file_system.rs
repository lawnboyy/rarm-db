use async_trait::async_trait;
use std::io::{Result, SeekFrom};
use std::path::Path;
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
    // async fn read_at(&self, buffer: &mut [u8], offset: u64) -> Result<usize>;
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

        // TODO: Temp fix to race condition. This is bad for performance. We'll undo it once the read_at method is
        // implemented and we can use it in our test.
        file_guard.sync_all().await?; // Force metadata update

        // file_guard goes out of scope and will release the lock
        Ok({})
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
        // This will fail to compile until you add write_at to the FileHandle trait
        handle
            .write_at(data, 0)
            .await
            .expect("Should write to file");

        // Verify with std::fs that data hit the disk (checking size)
        let metadata = std::fs::metadata(&file_path).expect("File should exist");
        let length = metadata.len();
        assert_eq!(length, 5);
    }
}
