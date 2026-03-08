use std::{
    collections::HashMap,
    iter,
    sync::{Arc, Mutex},
};

use crate::{
    BufferPoolError, DiskManager, Frame, PageId, PageReadGuard, page_guard::PageWriteGuard,
};

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
        let free_frame = &self.frames[frame_id];
        free_frame.increment_pin_count();
        free_frame.set_page_id(Some(page_id));

        // Add the page ID to the page table.
        self.page_table.lock().unwrap().insert(page_id, frame_id);

        // Acquire the write lock, contruct and return the page write guard with a reference to the frame.
        let write_lock = free_frame.write_data();
        Ok(PageWriteGuard::new(free_frame, write_lock))
    }

    /// Fetches a page and returns it, wrapped in a shared read lock guard that will unpin
    /// the frame when it goes out of scope and is dropped.
    pub async fn fetch_page_read(
        &self,
        page_id: PageId,
    ) -> Result<PageReadGuard<'_>, BufferPoolError> {
        // Call the private helper to return a pinned frame containing the page data.
        if let Ok(frame) = self.pin_frame(page_id).await {
            let read_lock = frame.read_data();
            Ok(PageReadGuard::new(frame, read_lock))
        } else {
            return Err(BufferPoolError::BufferFull);
        }
    }

    /// Fetches a page and returns it, wrapped in an exclusive write lock guard that will unpin
    /// the frame when it goes out of scope and is dropped.
    pub async fn fetch_page_write(
        &self,
        page_id: PageId,
    ) -> Result<PageWriteGuard<'_>, BufferPoolError> {
        // Call the private helper to return a pinned frame containing the page data.
        if let Ok(frame) = self.pin_frame(page_id).await {
            let write_lock = frame.write_data();
            Ok(PageWriteGuard::new(frame, write_lock))
        } else {
            return Err(BufferPoolError::BufferFull);
        }
    }

    /// Attempts to find the page in the cache. Upon a cache miss, if a free frame is available, the
    /// page is read from disk and loaded into the free frame. If no free frames are available, the
    /// evictor is called to evict a page from the cache to free up a frame.
    async fn pin_frame(&self, page_id: PageId) -> Result<&Frame, BufferPoolError> {
        // Check the page table to see if the page is cached...
        // Only hold the lock long enough to fetch the frame ID and pin it...
        let cached_frame_id = {
            let page_table_guard = self.page_table.lock().unwrap();
            // We make a copy of the frame ID because the page table HashMap will return a reference to
            // the frame ID value in memory which can't be guaranteed to be valid after the lock is released
            // at the end of this scope. We'll need to reference the frame ID outside the scope below if we
            // found a cached frame, hence the copy.
            // Note: The syntax here is confusing, but the Some(&frame_id) is not borrowing a reference to
            // the frame_id like it would if we were on the right side of the expression. Instead it is a
            // short hand pattern to deference the pointer and copy the underlying value into our frame_id
            // variable.
            if let Some(&frame_id) = page_table_guard.get(&page_id) {
                // We must pin the frame inside the lock to guarantee that it will not be evicted by another
                // thread prior to the current read completing.
                self.frames[frame_id].increment_pin_count();
                // TODO: Remove this frame from the evictor.
                Some(frame_id)
            } else {
                None
            }
        };

        if let Some(frame_id) = cached_frame_id {
            let frame = &self.frames[frame_id];
            Ok(frame)
        } else {
            // Handle a cache miss by loading the page from disk.
            // First check if we have any free frames...
            let free_frame_id = {
                // Acquire the lock on the free_frames vector.
                let mut free_frame_guard = self.free_frames.lock().unwrap();
                if let Some(free_frame_id) = free_frame_guard.pop() {
                    // Pin the frame inside the lock so we prevent eviction.
                    self.frames[free_frame_id].increment_pin_count();
                    Some(free_frame_id)
                } else {
                    None
                }
            };

            if let Some(frame_id) = free_frame_id {
                // We have a free frame, so load we can return it since it gets loaded inside the free frame lock.
                let frame = &self.frames[frame_id];
                // Load the page from disk into the frame.
                // TODO: We need to handle a phantom fetch race condition in which multiple threads could potentially load
                // the same page into different frames.
                let mut write_guard = self.frames[frame_id].write_data();
                self.disk_manager
                    .read_page(page_id, &mut write_guard)
                    .await
                    .map_err(|e| {
                        BufferPoolError::DiskRead(format!(
                            "Error reading page {} from disk: {}",
                            page_id, e
                        ))
                    })?;
                Ok(frame)
            } else {
                // We had no free frames, so now we have to evict a frame to make free up memory to store the requested page.
                return Err(BufferPoolError::Generic(String::from(
                    "Handle frame eviction.",
                )));
            }
        }
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

    #[tokio::test]
    async fn test_bpm_fetch_page_cache_hit() {
        let dir = tempdir().unwrap();
        let fs = Arc::new(TokioFileSystem::new());
        let disk_manager = Arc::new(DiskManager::new(fs, dir.path().to_path_buf()));

        let table_id = 200;
        disk_manager
            .create_table_file(table_id)
            .await
            .expect("Should create table file");

        let bpm = BufferPoolManager::new(2, disk_manager);

        // Act 1: Create a brand new page and write to it
        let page_id = {
            let mut page_guard = bpm.create_page(table_id).await.expect("Should create page");
            page_guard[0] = 88;
            page_guard[1] = 99;
            page_guard.mark_dirty();

            // Note: We need a way to get the PageId out of the guard!
            page_guard.page_id()
        };
        // The write guard drops here. The frame's pin_count hits 0, and you should add it to the evictor.

        // Act 2: Fetch the EXACT same page for reading
        let read_guard = bpm
            .fetch_page_read(page_id)
            .await
            .expect("Should fetch page successfully");

        // Assert 1: The data should be exactly what we wrote (proving it came from memory, not disk)
        assert_eq!(88, read_guard[0]);
        assert_eq!(99, read_guard[1]);
    }

    #[tokio::test]
    async fn test_bpm_fetch_page_write_cache_hit() {
        let dir = tempdir().unwrap();
        let fs = Arc::new(TokioFileSystem::new());
        let disk_manager = Arc::new(DiskManager::new(fs, dir.path().to_path_buf()));

        let table_id = 300;
        disk_manager
            .create_table_file(table_id)
            .await
            .expect("Should create table file");

        let bpm = BufferPoolManager::new(2, disk_manager);

        // Act 1: Create a brand new page and write initial data
        let page_id = {
            let mut page_guard = bpm.create_page(table_id).await.expect("Should create page");
            page_guard[0] = 11;
            page_guard[1] = 22;
            page_guard.mark_dirty();
            page_guard.page_id()
        };

        // Act 2: Fetch the SAME page for writing (Cache Hit)
        {
            let mut write_guard = bpm
                .fetch_page_write(page_id)
                .await
                .expect("Should fetch page for writing successfully");

            // Verify the old data is there
            assert_eq!(11, write_guard[0]);
            assert_eq!(22, write_guard[1]);

            // Mutate the data
            write_guard[0] = 33;
            write_guard[1] = 44;
            write_guard.mark_dirty();
        } // write_guard drops, frame unpins

        // Act 3: Fetch for reading to verify the second mutation stuck
        let read_guard = bpm
            .fetch_page_read(page_id)
            .await
            .expect("Should fetch page successfully");

        assert_eq!(33, read_guard[0]);
        assert_eq!(44, read_guard[1]);
    }

    #[tokio::test]
    async fn test_bpm_fetch_page_cache_miss_with_free_frame() {
        let dir = tempdir().unwrap();
        let fs = Arc::new(TokioFileSystem::new());
        let disk_manager = Arc::new(DiskManager::new(fs, dir.path().to_path_buf()));

        let table_id = 400;
        disk_manager
            .create_table_file(table_id)
            .await
            .expect("Should create table file");

        // Setup: Pre-allocate and write a page directly to disk using DiskManager
        // This simulates a page that exists in the database but is currently NOT in the Buffer Pool
        let page_id = disk_manager
            .allocate_page(table_id)
            .await
            .expect("Should allocate page");
        let mut disk_buffer = [0u8; crate::page_id::PAGE_SIZE];
        disk_buffer[0] = 77;
        disk_buffer[1] = 88;

        disk_manager
            .write_page(page_id, &disk_buffer)
            .await
            .expect("Should write page directly to disk");

        // Act 1: Initialize BPM
        let bpm = BufferPoolManager::new(2, disk_manager);

        // Act 2: Fetch the page for reading.
        // It is NOT in the page table, so it must trigger a cache miss, pop a free frame, and read from disk.
        let read_guard = bpm
            .fetch_page_read(page_id)
            .await
            .expect("Should fetch page from disk on cache miss");

        // Assert: The data should perfectly match what we wrote to disk
        assert_eq!(77, read_guard[0]);
        assert_eq!(88, read_guard[1]);
    }
}
