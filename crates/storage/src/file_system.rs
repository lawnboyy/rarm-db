use async_trait::async_trait;
use std::io::Result;
use std::path::Path;
use tokio::fs::{File, OpenOptions};

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
    _file: File,
}

impl TokioFileHandle {
    pub fn new(file: File) -> Self {
        Self { _file: file }
    }
}

#[async_trait]
impl FileHandle for TokioFileHandle {}

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
}
