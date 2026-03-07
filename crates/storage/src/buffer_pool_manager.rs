use std::{
    collections::HashMap,
    iter,
    sync::{Arc, Mutex},
};

use crate::{DiskManager, Frame, PageId, page_guard::PageWriteGuard};

pub struct BufferPoolManager {
    disk_manager: Arc<DiskManager>,
    frames: Vec<Frame>,
    free_frames: Mutex<Vec<usize>>,
    page_table: Mutex<HashMap<PageId, usize>>,
}

impl BufferPoolManager {
    pub fn new(size: usize, disk_manager: Arc<DiskManager>) -> Self {
        // Initialize our vector of free frames available.
        let mut initial_frames = vec![0; 0];
        for i in 0..(size) {
            initial_frames.push(i);
        }

        BufferPoolManager {
            disk_manager,
            frames: iter::repeat_with(Frame::new).take(size).collect(),
            free_frames: Mutex::new(initial_frames),
            page_table: Mutex::new(HashMap::new()),
        }
    }

    pub async fn create_page(&self, table_id: u32) -> Result<PageWriteGuard<'_>, String> {
        // Let's create our new page using the disk manager...
        let page_id = self
            .disk_manager
            .allocate_page(table_id)
            .await
            // TODO: Consider doing some better error handling here instead of returning a string.
            .map_err(|e| e.to_string())?;

        // Check for a free frame. If no free frame is available, evict a frame.
        // Acquire the lock on the free frames to see if any are available.
        let frame_id = if let Some(id) = self.free_frames.lock().unwrap().pop() {
            id
        } else {
            todo!("Handle frame eviction.");
        };

        // Pin the frame and set the page ID...
        // TODO: Add the page ID to the page table.
        let free_frame = &self.frames[frame_id];
        free_frame.increment_pin_count();
        free_frame.set_page_id(Some(page_id));
        // self.page_table.lock().unwrap()[page_id] = frame_id;

        // Acquire the write lock, contruct and return the page write guard with a reference to the frame.
        let write_lock = free_frame.write_data();
        Ok(PageWriteGuard::new(&free_frame, write_lock))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::disk_manager::DiskManager;
    use crate::file_system::TokioFileSystem;
    use std::sync::Arc;
    use tempfile::tempdir;

    #[tokio::test]
    async fn test_bpm_new_page_allocates_and_returns_write_guard() {
        let dir = tempdir().unwrap();
        let fs = Arc::new(TokioFileSystem::new());
        let disk_manager = Arc::new(DiskManager::new(fs, dir.path().to_path_buf()));

        let table_id = 100;
        disk_manager
            .create_table_file(table_id)
            .await
            .expect("Should create table file");

        // Act 1: Initialize BPM with a pool capacity of 2
        let bpm = BufferPoolManager::new(2, disk_manager);

        // Act 2: Request a brand new page
        let mut page_guard = bpm
            .create_page(table_id)
            .await
            .expect("Should return a new page write guard");

        // Assert 1: Mutate the data to prove we have exclusive write access
        page_guard[0] = 42;
        page_guard[1] = 99;

        // At this point, the page guard goes out of scope and drops,
        // which should automatically unpin the frame!
    }
}
