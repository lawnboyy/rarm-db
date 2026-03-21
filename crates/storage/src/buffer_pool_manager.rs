use std::{
    collections::HashMap,
    sync::{Arc, Mutex},
};

use tokio::sync::broadcast;

use crate::{
    BufferPoolError, DiskManager, Evictor, Frame, PageId, PageReadGuard, evictor::ClockEvictor,
    page_guard::PageWriteGuard,
};

pub struct BufferPoolManager {
    disk_manager: Arc<DiskManager>,
    evictor: Box<dyn Evictor>,
    /// Cached pages in memory.
    frames: Vec<Frame>,
    /// Free frames available for use.
    free_frames: Mutex<Vec<usize>>,
    /// Map of broadcast channels that tracks any in-flight page fetches. This
    /// allows other threads that are accessing a page to subcribe to an in-
    /// flight request for the page and sleep until the disk read is complete
    /// and the page is cached.
    in_flight_fetches: Mutex<HashMap<PageId, broadcast::Sender<usize>>>,
    /// Map of currently cached pages to the frame ID that holds them.
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
            evictor: Box::new(ClockEvictor::new(size)),
            frames: (0..size).map(|i| Frame::new(i)).collect(),
            free_frames: Mutex::new(initial_frames),
            in_flight_fetches: Mutex::new(HashMap::new()),
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
        Ok(PageWriteGuard::new(
            self.evictor.as_ref(),
            free_frame,
            write_lock,
        ))
    }

    /// Fetches a page and returns it, wrapped in a shared read lock guard that will unpin
    /// the frame when it goes out of scope and is dropped.
    pub async fn fetch_page_read(
        &self,
        page_id: PageId,
    ) -> Result<PageReadGuard<'_>, BufferPoolError> {
        // Call the private helper to return a pinned frame containing the page data.
        if let Ok(frame) = self.fetch_and_pin_frame(page_id).await {
            let read_lock = frame.read_data();
            Ok(PageReadGuard::new(self.evictor.as_ref(), frame, read_lock))
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
        if let Ok(frame) = self.fetch_and_pin_frame(page_id).await {
            let write_lock = frame.write_data();
            Ok(PageWriteGuard::new(
                self.evictor.as_ref(),
                frame,
                write_lock,
            ))
        } else {
            return Err(BufferPoolError::BufferFull);
        }
    }

    /// Attempts to find the page in the cache. Upon a cache miss, if a free frame is available, the
    /// page is read from disk and loaded into the free frame. If no free frames are available, the
    /// evictor is called to evict a page from the cache to free up a frame.
    async fn fetch_and_pin_frame(&self, page_id: PageId) -> Result<&Frame, BufferPoolError> {
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
                self.pin_frame(frame_id);
                Some(frame_id)
            } else {
                None
            }
        };

        // See if the paged is cached...
        if let Some(frame_id) = cached_frame_id {
            // The page is cached, so we can return it.
            let frame = &self.frames[frame_id];
            Ok(frame)
        } else {
            // The page is not cached...
            // Check if there is an in-flight request for this page ID...
            let mut in_flight_guard = self.in_flight_fetches.lock().unwrap();
            let in_flight_result = in_flight_guard.get(&page_id);

            if let Some(in_flight_channel) = in_flight_result {
                // There is an in-flight request for the page, so we will subscribe to the channel and
                // wait for it to complete.
                let mut receiver = in_flight_channel.subscribe();
                // Drop our mutex on the in flight map so others can access it.
                drop(in_flight_guard);
                // Wait for the in-flight page request to complete.
                let frame_id = receiver.recv().await.map_err(|e| {
                    BufferPoolError::InFlightBroadcast(format!(
                        "Error waiting on in flight fetch for page {}: {}",
                        page_id, e
                    ))
                })?;
                // The in flight request of the page is complete, so the frame should contain the requested page
                // data. Now we pin it and return.
                self.frames[frame_id].increment_pin_count();
                return Ok(&self.frames[frame_id]);
            } else {
                // There was no in flight request, and we still hold the mutex for the in flight fetches map. So
                // this thread is the leader and will need to create the broadcast channel for other threads to
                // subscribe to, then fetch the page from disk and load it into a frame in the cache.
                let (tx, _rx) = broadcast::channel::<usize>(1);
                in_flight_guard.insert(page_id, tx.clone());
                // Now that we have our broadcast channel set up we can drop our mutexes and allow other threads to
                // check for in flight fetches.
                drop(in_flight_guard);
                // Handle a cache miss by loading the page from disk.
                // First check if we have any free frames...
                let frame_id = {
                    // Acquire the lock on the free_frames vector.
                    let mut free_frame_guard = self.free_frames.lock().unwrap();
                    if let Some(free_frame_id) = free_frame_guard.pop() {
                        // Pin the frame inside the lock so we prevent eviction.
                        self.frames[free_frame_id].increment_pin_count();
                        free_frame_id
                    } else {
                        // We had no free frames, so now we have to evict a frame to free up memory to store the requested page.
                        if let Some(free_frame_id) = self.evictor.victim() {
                            // We found a victim, so now we can load our page into the free frame.
                            self.frames[free_frame_id].increment_pin_count();
                            free_frame_id
                        } else {
                            return Err(BufferPoolError::BufferFull);
                        }
                    }
                };

                // If we reach this point, we have a free frame available, either one that's never been used
                // or one returned as a result of an eviction.
                let frame = &self.frames[frame_id];
                // Load the page from disk into the frame.
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
                // The leader is done loading in the page, so now it needs to broadcast a message to any other threads that
                // are waiting on this page to be cached.
                // Acquire the locks
                let mut page_table_guard = self.page_table.lock().unwrap();
                let mut in_flight_fetches_guard = self.in_flight_fetches.lock().unwrap();
                // We've already pinned the frame in the free frames mutex above, so we don't need to pin it again...
                // We do need to set the page ID
                self.frames[frame_id].set_page_id(Some(page_id));
                // Update the page table with the newly cached page...
                page_table_guard.insert(page_id, frame_id);

                // Remove the page ID from the in flight fetches map and get the transmitter...
                if let Some(tx) = in_flight_fetches_guard.remove(&page_id) {
                    // Notify other threads that the page fetch is complete.
                    let _ = tx.send(frame_id);
                }

                Ok(frame)
            }
        }
    }

    /// Pins the frame and updates the evictor state to keep them in sync. If the frame
    /// is pinned, then the evictor does not consider it elegible for eviction.
    fn pin_frame(&self, frame_id: usize) {
        // Increment the frame's pin count. If the previous pin count was zero, then
        // this operation has transitioned the frame from unpinned to pinned which
        // means we must tell the evictor to remove the frame from eviction eligibility.
        // If the frame pin operation does not cause the frame to go from unpinned to
        // pinned, then there is no need to update the evictor.
        if self.frames[frame_id].increment_pin_count() == 0 {
            self.evictor.remove(frame_id);
        }
    }

    // --- Test Helpers ---
    // These methods are only compiled during testing to allow us to assert internal state.
    #[cfg(test)]
    pub fn get_free_frame_count(&self) -> usize {
        self.free_frames.lock().unwrap().len()
    }

    #[cfg(test)]
    pub fn get_pin_count(&self, page_id: PageId) -> Option<usize> {
        let page_table = self.page_table.lock().unwrap();
        if let Some(&frame_id) = page_table.get(&page_id) {
            Some(self.frames[frame_id].get_pin_count())
        } else {
            None
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

    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn test_bpm_concurrent_cache_miss_prevents_phantom_fetch() {
        let dir = tempdir().unwrap();
        let fs = Arc::new(TokioFileSystem::new());
        let disk_manager = Arc::new(DiskManager::new(fs, dir.path().to_path_buf()));

        let table_id = 500;
        disk_manager
            .create_table_file(table_id)
            .await
            .expect("Should create table file");

        // Setup: Pre-allocate and write the target page to disk
        let page_id = disk_manager
            .allocate_page(table_id)
            .await
            .expect("Should allocate page");
        let mut disk_buffer = [0u8; crate::page_id::PAGE_SIZE];
        disk_buffer[0] = 99;
        disk_manager
            .write_page(page_id, &disk_buffer)
            .await
            .expect("Should write page");

        let bpm = Arc::new(BufferPoolManager::new(10, disk_manager));

        // FIX: We now use TWO barriers!
        let barrier_ready = Arc::new(std::sync::Barrier::new(11));
        let barrier_done = Arc::new(std::sync::Barrier::new(11));

        let mut handles = vec![];

        for _ in 0..10 {
            let bpm_clone = Arc::clone(&bpm);
            let barrier_ready_clone = Arc::clone(&barrier_ready);
            let barrier_done_clone = Arc::clone(&barrier_done);

            handles.push(std::thread::spawn(move || {
                let rt = tokio::runtime::Builder::new_current_thread()
                    .enable_all()
                    .build()
                    .unwrap();

                rt.block_on(async move {
                    let read_guard = bpm_clone.fetch_page_read(page_id).await.unwrap();
                    assert_eq!(99, read_guard[0]);

                    // BARRIER 1: Tell the main thread we have our locks!
                    barrier_ready_clone.wait();

                    // BARRIER 2: Wait for the main thread to finish asserting before we drop!
                    barrier_done_clone.wait();
                }); // read_guard FINALLY drops here!
            }));
        }

        // BARRIER 1: Wait for all 10 workers to get their guards.
        let b_ready_main = Arc::clone(&barrier_ready);
        tokio::task::spawn_blocking(move || b_ready_main.wait())
            .await
            .unwrap();

        // -----------------------------------------------------
        // THE WORKERS ARE FROZEN. WE ARE SAFE TO ASSERT!
        // -----------------------------------------------------
        assert_eq!(
            9,
            bpm.get_free_frame_count(),
            "Exactly 1 free frame should be consumed."
        );
        assert_eq!(
            Some(10),
            bpm.get_pin_count(page_id),
            "Pin count must be exactly 10."
        );

        // BARRIER 2: Release the workers so they can drop their guards and clean up.
        let b_done_main = Arc::clone(&barrier_done);
        tokio::task::spawn_blocking(move || b_done_main.wait())
            .await
            .unwrap();

        for handle in handles {
            handle.join().unwrap();
        }
    }

    #[tokio::test]
    async fn test_bpm_fetch_page_evicts_unpinned_frame_when_full() {
        let dir = tempdir().unwrap();
        let fs = Arc::new(TokioFileSystem::new());
        let disk_manager = Arc::new(DiskManager::new(fs, dir.path().to_path_buf()));

        let table_id = 600;
        disk_manager
            .create_table_file(table_id)
            .await
            .expect("Should create table file");

        // Setup: Pre-allocate 3 pages directly on disk
        let page_id_1 = disk_manager.allocate_page(table_id).await.unwrap();
        let page_id_2 = disk_manager.allocate_page(table_id).await.unwrap();
        let page_id_3 = disk_manager.allocate_page(table_id).await.unwrap();

        let mut buf1 = [0u8; crate::page_id::PAGE_SIZE];
        buf1[0] = 11;
        let mut buf2 = [0u8; crate::page_id::PAGE_SIZE];
        buf2[0] = 22;
        let mut buf3 = [0u8; crate::page_id::PAGE_SIZE];
        buf3[0] = 33;

        disk_manager.write_page(page_id_1, &buf1).await.unwrap();
        disk_manager.write_page(page_id_2, &buf2).await.unwrap();
        disk_manager.write_page(page_id_3, &buf3).await.unwrap();

        // Act 1: Initialize BPM with ONLY 2 free frames
        let bpm = BufferPoolManager::new(2, disk_manager);

        // Act 2: Fill the buffer pool completely
        {
            let guard1 = bpm.fetch_page_read(page_id_1).await.unwrap();
            let guard2 = bpm.fetch_page_read(page_id_2).await.unwrap();

            assert_eq!(11, guard1[0]);
            assert_eq!(22, guard2[0]);

            assert_eq!(
                0,
                bpm.get_free_frame_count(),
                "Pool should be completely full"
            );
        } // guard1 and guard2 go out of scope here. The pin_counts for both frames drop to 0!

        // Act 3: Fetch the 3rd page.
        // We have 0 free frames, so this MUST consult the ClockEvictor, evict an unpinned frame, and reuse it.
        let guard3 = bpm
            .fetch_page_read(page_id_3)
            .await
            .expect("Should successfully evict a frame and fetch page 3");

        // Assert: We got the correct data for page 3
        assert_eq!(
            33, guard3[0],
            "Evicted frame should contain the newly loaded data"
        );
    }
}
