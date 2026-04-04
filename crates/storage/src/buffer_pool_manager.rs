use std::{
    collections::HashMap,
    sync::{Arc, Mutex, MutexGuard},
};

use tokio::sync::broadcast;

use crate::{
    BufferPoolError, DiskManager, Evictor, Frame, PageId, PageReadGuard, evictor::ClockEvictor,
    page_guard::PageWriteGuard, page_id::PAGE_SIZE,
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
    page_io_processing: Mutex<HashMap<PageId, broadcast::Sender<Result<(), String>>>>,
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
            page_io_processing: Mutex::new(HashMap::new()),
            page_table: Mutex::new(HashMap::new()),
        }
    }

    pub async fn create_page(&self, table_id: u32) -> Result<PageWriteGuard<'_>, BufferPoolError> {
        // Let's create our new page using the disk manager...
        let page_id = self
            .disk_manager
            .allocate_page(table_id)
            .await
            // TODO: Consider doing some better error handling here instead of returning a string.
            .map_err(|e| BufferPoolError::PageAllocation(e.to_string()))?;

        // Lock the page table so we can find a frame, pin it, and insert the entry into the page table.
        let mut page_table_guard = self.page_table.lock().unwrap();

        // Placeholder for dirty page data if we need to flush an evicted page...
        let mut page_data: Option<(PageId, [u8; PAGE_SIZE])> = None;

        // Check for a free frame. If no free frame is available, evict a frame.
        // Acquire the lock on the free frames to see if any are available.
        let frame_id = if let Some(id) = self.free_frames.lock().unwrap().pop() {
            self.pin_frame(id);
            id
        } else {
            // There are no free frames available so we must evict a page.
            let mut page_processing_guard = self.page_io_processing.lock().unwrap();
            // This method pins the frame.
            let evicted_page_data = self.evict_page(&mut page_table_guard).unwrap();
            let free_frame_id = evicted_page_data.0;
            page_data = evicted_page_data.1;

            // If the evicted page was dirty, we need to flush it to disk before we release the page_table lock,
            // and we need to add a processing channel to notify other threads that this page is getting flushed.
            if self.frames[free_frame_id].is_dirty() {
                // Create a channel to notify other threads that this page is processing...
                let (tx, _rx) = broadcast::channel::<Result<(), String>>(1);
                page_processing_guard.insert(page_id, tx.clone());
                // Now that we have our broadcast channel set up we can drop our mutexes and allow other threads to
                // check if this page is being processed...
                drop(page_processing_guard);
            }

            free_frame_id
        };

        // Pin the frame and set the page ID...
        let free_frame = &self.frames[frame_id];
        free_frame.set_page_id(Some(page_id));

        // Add the page ID to the page table.
        page_table_guard.insert(page_id, frame_id);

        drop(page_table_guard);

        // Now that we have dropped the locks we can flush the dirty page data to disk if necessary...
        if let Some((page_to_flush_id, page_data)) = page_data {
            let result = self
                .disk_manager
                .write_page(page_to_flush_id, &page_data)
                .await;
            if let Err(error) = result {
                return Err(BufferPoolError::DiskWrite(format!(
                    "Could not write page: {} to disk! Error: {}",
                    page_to_flush_id, error
                )));
            }

            // Let waiting threads know that processing is complete and remove the channel for this page ID...
            let mut page_processing_guard = self.page_io_processing.lock().unwrap();
            if let Some(tx) = page_processing_guard.remove(&page_to_flush_id) {
                // Notify other threads that the page fetch is complete.
                let _ = tx.send(Ok(()));
            }
        }

        // Acquire the write lock, contruct and return the page write guard with a reference to the frame.
        let write_lock = free_frame.write_data();
        Ok(PageWriteGuard::new(
            self.evictor.as_ref(),
            free_frame,
            write_lock,
        ))
    }

    // /// Fetches a page and returns it, wrapped in a shared read lock guard that will unpin
    // /// the frame when it goes out of scope and is dropped.
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

    // /// Fetches a page and returns it, wrapped in an exclusive write lock guard that will unpin
    // /// the frame when it goes out of scope and is dropped.
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
        // Lock the page table to prepare read from the cache and pin or to insert a new entry...
        let mut page_table_guard = self.page_table.lock().unwrap();

        // Check the cache...
        if let Some(&frame_id) = page_table_guard.get(&page_id) {
            // Pin the frame while the page table lock is held to prevent an eviction...
            self.pin_frame(frame_id);
            // With the frame pinned, the page table lock can be dropped
            drop(page_table_guard);

            // Check if the page is being fetched by checking the processing map. It should not be possible for the page
            // to be flushing here since it was found in the page table and, hence, has not been evicted.
            let io_processing_guard = self.page_io_processing.lock().unwrap();
            let io_processing_result = io_processing_guard.get(&page_id);
            if let Some(processing_channel) = io_processing_result {
                // The page is being read from disk by another thread, so subscribe to the channel and wait.
                let mut sub = processing_channel.subscribe();
                // Drop the lock to allow other threads to check for in-flight fetches...
                drop(io_processing_guard);
                // Block until the page fetch is complete...
                let result = sub.recv().await;

                // If there is an error waiting on the subscription, return it...
                if let Err(e) = result {
                    return Err(BufferPoolError::PageProcessingBroadcast(format!(
                        "Error waiting on in flight fetch for page {}: {}",
                        page_id, e
                    )));
                }
            }

            // Return the page frame...
            return Ok(&self.frames[frame_id]);
        }

        let available_frame_id = {
            if let Some(free_frame_id) = self.free_frames.lock().unwrap().pop() {
                Some(free_frame_id)
            } else {
                // Evict
                if let Ok((evicted_frame_id, page_data)) = self.evict_page(&mut page_table_guard) {
                    // TODO: If the page is dirty, capture the data and flush to disk outside of all held locks.
                    // Otherwise, set the return the frame...                
                    Some(evicted_frame_id)
                    // Page table lock is dropped here.
                } else {
                    None
                }
            }
        };

        // If no frame ID is found in the free frames and no frame could be evicted, the buffer is full and no
        // the page cannot be fetched.
        if let None = available_frame_id {
            return Err(BufferPoolError::BufferFull);
        }       

        let frame_id = available_frame_id.unwrap();

        // While the locks are held, insert the new entry into the page table...
        page_table_guard.insert(page_id, frame_id);
        // Set the page ID on the frame...
        self.frames[frame_id].set_page_id(Some(page_id));
        // ...and pin the frame.
        self.pin_frame(frame_id);

        // Notify other threads that this page is processing...
        let mut io_processing_guard = self.page_io_processing.lock().unwrap();
        let (tx, _rx) = broadcast::channel::<Result<(), String>>(1);
        io_processing_guard.insert(page_id, tx.clone());

        // Mutexes cannot be held across async/await boundaries. Drop the locks so we can load from disk asynchronously.
        drop(io_processing_guard);
        drop(page_table_guard);

        // Load the page from disk...
        // Create a temporary buffer to hold the page data read from disk. We cannot read the
        // page directly into the frame buffer because it's protected by a RwLock which cannot
        // be held across an 'await' boundary (the disk_manager read operation).
        let mut read_buffer = [0u8; PAGE_SIZE];            
        self.disk_manager
            .read_page(page_id, &mut read_buffer)
            .await
            .map_err(|e| {
                BufferPoolError::DiskRead(format!(
                    "Error reading page {} from disk: {}",
                    page_id, e
                ))
            })?;
        // Now that the disk read is complete, we can lock our frame and copy the data over to it. Other
        // threads accessing this page frame will be blocking on the broadcast channel in the IO processing
        // map.
        let mut write_guard = self.frames[frame_id].write_data();
        write_guard.copy_from_slice(&read_buffer);

        // Now send a message via the broadcast channel to let other threads know the page frame IO processing
        // has completed and remove the channel.
        let mut io_processing_guard = self.page_io_processing.lock().unwrap();
        // Remove the page ID from the in IO processing map and get the sender...
        if let Some(sender) = io_processing_guard.remove(&page_id) {
            // Notify other threads that the page fetch is complete.
            let _ = sender.send(Ok(()));
        }

        Ok(&self.frames[frame_id])
        
    }

    /// Attempts to find the page in the cache. Upon a cache miss, if a free frame is available, the
    /// page is read from disk and loaded into the free frame. If no free frames are available, the
    /// evictor is called to evict a page from the cache to free up a frame.
    // async fn fetch_and_pin_frame(&self, page_id: PageId) -> Result<&Frame, BufferPoolError> {
    //     // Check the page table to see if the page is cached...
    //     // Only hold the lock long enough to fetch the frame ID and pin it...
    //     let cached_frame_id = {
    //         let page_table_guard = self.page_table.lock().unwrap();
    //         // We make a copy of the frame ID because the page table HashMap will return a reference to
    //         // the frame ID value in memory which can't be guaranteed to be valid after the lock is released
    //         // at the end of this scope. We'll need to reference the frame ID outside the scope below if we
    //         // found a cached frame, hence the copy.

    //         // Note: The syntax here is confusing, but the Some(&frame_id) is not borrowing a reference to
    //         // the frame_id like it would if we were on the right side of the expression. Instead it is a
    //         // short hand pattern to deference the pointer and copy the underlying value into our frame_id
    //         // variable.
    //         if let Some(&frame_id) = page_table_guard.get(&page_id) {
    //             // We must pin the frame inside the lock to guarantee that it will not be evicted by another
    //             // thread prior to the current read completing.
    //             self.pin_frame(frame_id);
    //             Some(frame_id)
    //         } else {
    //             None
    //         }
    //     };

    //     // See if the paged is cached...
    //     if let Some(frame_id) = cached_frame_id {
    //         // The page is cached, so we can return it.
    //         let frame = &self.frames[frame_id];
    //         Ok(frame)
    //     } else {
    //         // The page is not cached...
    //         // Check if there is an in-flight request for this page ID...
    //         let mut in_flight_guard = self.in_flight_fetches.lock().unwrap();
    //         let in_flight_result = in_flight_guard.get(&page_id);

    //         if let Some(in_flight_channel) = in_flight_result {
    //             // There is an in-flight request for the page, so we will subscribe to the channel and
    //             // wait for it to complete.
    //             let mut receiver = in_flight_channel.subscribe();
    //             // Drop our mutex on the in flight map so others can access it.
    //             drop(in_flight_guard);
    //             // Wait for the in-flight page request to complete.
    //             let frame_id = receiver.recv().await.map_err(|e| {
    //                 BufferPoolError::InFlightBroadcast(format!(
    //                     "Error waiting on in flight fetch for page {}: {}",
    //                     page_id, e
    //                 ))
    //             })?;
    //             // The in flight request of the page is complete, so the frame should contain the requested page
    //             // data. Now we pin it and return.
    //             self.pin_frame(frame_id);
    //             return Ok(&self.frames[frame_id]);
    //         } else {
    //             // There was no in flight request, and we still hold the mutex for the in flight fetches map. So
    //             // this thread is the leader and will need to create the broadcast channel for other threads to
    //             // subscribe to, then fetch the page from disk and load it into a frame in the cache.
    //             let (tx, _rx) = broadcast::channel::<usize>(1);
    //             in_flight_guard.insert(page_id, tx.clone());
    //             // Now that we have our broadcast channel set up we can drop our mutexes and allow other threads to
    //             // check for in flight fetches.
    //             drop(in_flight_guard);
    //             // Handle a cache miss by loading the page from disk.
    //             // First check if we have any free frames...
    //             let frame_id = {
    //                 // Acquire the lock on the free_frames vector.
    //                 // Make sure we only hold the lock for the 'if' block because the 'else' block contains an 'await'
    //                 // call and we cannot hold the lock across an await boundary.
    //                 if let Some(free_frame_id) = self.free_frames.lock().unwrap().pop() {
    //                     // Pin the frame inside the lock so we prevent eviction.
    //                     self.pin_frame(free_frame_id);
    //                     free_frame_id
    //                 } else {
    //                     // If no free frames are available, then attempt to evict a page.
    //                     self.evict_page().await?
    //                 }
    //             };

    //             // If we reach this point, we have a free frame available, either one that's never been used
    //             // or one returned as a result of an eviction.
    //             let frame = &self.frames[frame_id];
    //             // Load the page from disk into the frame.
    //             let mut write_guard = self.frames[frame_id].write_data();
    //             self.disk_manager
    //                 .read_page(page_id, &mut write_guard)
    //                 .await
    //                 .map_err(|e| {
    //                     BufferPoolError::DiskRead(format!(
    //                         "Error reading page {} from disk: {}",
    //                         page_id, e
    //                     ))
    //                 })?;
    //             // The leader is done loading in the page, so now it needs to broadcast a message to any other threads that
    //             // are waiting on this page to be cached.
    //             // Acquire the locks
    //             let mut page_table_guard = self.page_table.lock().unwrap();
    //             let mut in_flight_fetches_guard = self.in_flight_fetches.lock().unwrap();
    //             // We've already pinned the frame in the free frames mutex above, so we don't need to pin it again...
    //             // We do need to set the page ID
    //             self.frames[frame_id].set_page_id(Some(page_id));
    //             // Update the page table with the newly cached page...
    //             page_table_guard.insert(page_id, frame_id);

    //             // Remove the page ID from the in flight fetches map and get the transmitter...
    //             if let Some(tx) = in_flight_fetches_guard.remove(&page_id) {
    //                 // Notify other threads that the page fetch is complete.
    //                 let _ = tx.send(frame_id);
    //             }

    //             Ok(frame)
    //         }
    //     }
    // }

    fn evict_page(
        &self,
        page_table_guard: &mut MutexGuard<'_, HashMap<PageId, usize>>,
    ) -> Result<(usize, Option<(PageId, [u8; PAGE_SIZE])>), BufferPoolError> {
        if let Some(free_frame_id) = self.evictor.victim() {
            let victim_page_id = self.frames[free_frame_id].get_page_id().unwrap();
            // Now that we have a victim we can remove it from the page table...
            page_table_guard.remove(&victim_page_id);

            // Pin the frame while we have the lock.
            self.pin_frame(free_frame_id);

            // Return the page ID and a copy of the data if the page is dirty...
            if self.frames[free_frame_id].is_dirty() {
                let page_data = self.frames[free_frame_id].read_data().clone();
                return Ok((
                    free_frame_id,
                    Some((self.frames[free_frame_id].get_page_id().unwrap(), page_data)),
                ));
            } else {
                // If the page is not dirty we don't need to flush to disk, so return an invalid page ID
                return Ok((free_frame_id, None));
            }
        }

        return Err(BufferPoolError::BufferFull);
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
    use crate::file_system::{FileSystem, TokioFileSystem};
    use std::sync::Arc;
    use tempfile::tempdir;

    #[tokio::test]
    async fn test_bpm_create_page_allocates_and_returns_write_guard() {
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

        // Expected PageId for the first allocation
        let expected_page_id = PageId {
            table_id,
            page_index: 0,
        };

        // Act 2: Request a brand new page
        let mut page_guard = bpm
            .create_page(table_id)
            .await
            .expect("Should return a new page write guard");

        // Assert 1: Mutate the data to prove we have exclusive write access
        page_guard[0] = 42;
        page_guard[1] = 99;
        page_guard.mark_dirty();

        // Assert 2: The frame must be safely in the page table and pinned!
        assert_eq!(
            Some(1),
            bpm.get_pin_count(expected_page_id),
            "The newly created page must be pinned in the page table"
        );

        // At this point, the page guard goes out of scope and drops,
        // which should automatically unpin the frame!
    }

    #[tokio::test]
    async fn test_bpm_create_page_evicts_dirty_frame_and_flushes() {
        let dir = tempdir().unwrap();
        let fs = Arc::new(TokioFileSystem::new());
        // Clone the Arc to pass to DiskManager so we can still use `fs` at the end of the test
        let disk_manager = Arc::new(DiskManager::new(
            Arc::clone(&fs) as Arc<dyn FileSystem>,
            dir.path().to_path_buf(),
        ));

        let table_id = 200;
        disk_manager
            .create_table_file(table_id)
            .await
            .expect("Should create table file");

        // Act 1: Initialize BPM with ONLY 1 frame
        let bpm = BufferPoolManager::new(1, disk_manager);

        let expected_page_id_0 = PageId {
            table_id,
            page_index: 0,
        };
        let expected_page_id_1 = PageId {
            table_id,
            page_index: 1,
        };

        // Setup: Create the first page, mutate it, and let it unpin
        {
            let mut guard1 = bpm
                .create_page(table_id)
                .await
                .expect("Should create first page");
            guard1[0] = 111;
            guard1[PAGE_SIZE - 1] = 222;
            guard1.mark_dirty();
        } // guard1 drops here, unpinning frame 0

        // Act 2: Create a NEW page. This forces the eviction and flush of page 0.
        {
            let _guard2 = bpm
                .create_page(table_id)
                .await
                .expect("Should evict and create second page");

            // Assert 1: Page 1 is pinned, Page 0 is gone from memory
            assert_eq!(
                Some(1),
                bpm.get_pin_count(expected_page_id_1),
                "Page 1 should be pinned"
            );
            assert_eq!(
                None,
                bpm.get_pin_count(expected_page_id_0),
                "Page 0 should be evicted"
            );
        }

        // Assert 2: Verify the dirty data was actually flushed to disk during eviction!
        let path = dir.path().join(format!("{}.tbl", table_id));
        let handle = fs.open_file(&path).await.expect("File should exist");
        let mut buffer = [0u8; PAGE_SIZE];
        handle
            .read_at(&mut buffer, 0)
            .await
            .expect("Should read page 0 from disk");

        assert_eq!(
            111, buffer[0],
            "First byte of evicted page should be flushed to disk"
        );
        assert_eq!(
            222,
            buffer[PAGE_SIZE - 1],
            "Last byte of evicted page should be flushed to disk"
        );
    }

    #[test]
    fn test_evict_page_returns_error_when_pool_full() {
        let dir = tempdir().unwrap();
        let fs = Arc::new(TokioFileSystem::new());
        let disk_manager = Arc::new(DiskManager::new(fs, dir.path().to_path_buf()));

        // Act 1: Initialize BPM with ONLY 1 frame
        let bpm = BufferPoolManager::new(1, disk_manager);

        // Act 2: Manually pin the only frame so the evictor has no eligible candidates
        bpm.pin_frame(0);

        // Act 3: Attempt to evict a page
        let result = {
            let mut page_table_guard = bpm.page_table.lock().unwrap();
            bpm.evict_page(&mut page_table_guard)
        };

        // Assert: It MUST gracefully return an error
        assert!(
            matches!(result, Err(BufferPoolError::BufferFull)),
            "evict_page should return BufferFull when the evictor has no victims"
        );
    }

    #[test]
    fn test_evict_page_evicts_clean_page_and_updates_page_table() {
        let dir = tempdir().unwrap();
        let fs = Arc::new(TokioFileSystem::new());
        let disk_manager = Arc::new(DiskManager::new(fs, dir.path().to_path_buf()));

        // Initialize BPM with 1 frame
        let bpm = BufferPoolManager::new(1, disk_manager);
        let page_id = PageId {
            table_id: 10,
            page_index: 0,
        };

        // Setup: Empty the free list and manually simulate a clean cached page in frame 0
        bpm.free_frames.lock().unwrap().clear();
        bpm.frames[0].set_page_id(Some(page_id));
        bpm.frames[0].set_dirty(false);
        bpm.page_table.lock().unwrap().insert(page_id, 0);

        // Tell the evictor this frame is eligible for eviction
        bpm.evictor.add(0);

        // Act: Evict the page
        let result = {
            let mut page_table_guard = bpm.page_table.lock().unwrap();
            bpm.evict_page(&mut page_table_guard)
        };

        // Assert 1: It should succeed, return frame 0, and return None for the dirty data
        assert!(
            matches!(result, Ok((0, None))),
            "evict_page should return Ok((0, None)) when a clean page is evicted"
        );

        // Assert 2: The evicted page MUST be removed from the page_table
        assert_eq!(
            None,
            bpm.get_pin_count(page_id),
            "evict_page must remove the victim's page_id from the page_table"
        );

        // Assert 3: The frame must be pinned by evict_page so the evictor doesn't give it out again
        assert_eq!(
            1,
            bpm.frames[0].get_pin_count(),
            "evict_page must pin the frame before returning it"
        );
    }

    #[test]
    fn test_evict_page_evicts_dirty_page_and_returns_data_to_flush() {
        let dir = tempdir().unwrap();
        let fs = Arc::new(TokioFileSystem::new());
        let disk_manager = Arc::new(DiskManager::new(fs, dir.path().to_path_buf()));

        // Initialize BPM with 1 frame
        let bpm = BufferPoolManager::new(1, disk_manager);
        let page_id = PageId {
            table_id: 20,
            page_index: 1,
        };

        // Setup: Manually simulate a DIRTY cached page in frame 0
        bpm.free_frames.lock().unwrap().clear();
        bpm.frames[0].set_page_id(Some(page_id));
        bpm.frames[0].set_dirty(true); // MARK DIRTY
        bpm.page_table.lock().unwrap().insert(page_id, 0);

        // Write some recognizable magic bytes to the frame's buffer
        {
            let mut write_guard = bpm.frames[0].write_data();
            write_guard[0] = 123;
            write_guard[PAGE_SIZE - 1] = 234;
        }

        bpm.evictor.add(0);

        // Act: Evict the page
        let result = {
            let mut page_table_guard = bpm.page_table.lock().unwrap();
            bpm.evict_page(&mut page_table_guard)
        };

        // Assert 1: It should succeed and return Some(...) for the dirty data
        let (frame_id, dirty_payload_opt) = result.expect("Should successfully evict");
        assert_eq!(0, frame_id);

        let (evicted_page_id, dirty_data) =
            dirty_payload_opt.expect("Dirty page must return data to flush");

        // Assert 2: It returned the correct PageId and the exact copied bytes!
        assert_eq!(page_id, evicted_page_id);
        assert_eq!(123, dirty_data[0]);
        assert_eq!(234, dirty_data[PAGE_SIZE - 1]);

        // Assert 3: Verification of state transitions
        assert_eq!(
            None,
            bpm.get_pin_count(page_id),
            "Must remove from page table"
        );
        assert_eq!(1, bpm.frames[0].get_pin_count(), "Must pin the frame");
    }

    #[tokio::test]
    async fn test_bpm_fetch_page_miss_loads_from_disk_then_hit_returns_cached_frame() {
        let dir = tempdir().unwrap();
        let fs = Arc::new(TokioFileSystem::new());
        let disk_manager = Arc::new(DiskManager::new(
            Arc::clone(&fs) as Arc<dyn FileSystem>,
            dir.path().to_path_buf(),
        ));

        let table_id = 300;
        disk_manager.create_table_file(table_id).await.unwrap();

        // Setup: Bypass the BPM to write directly to disk, simulating an existing populated database.
        let page_id = disk_manager.allocate_page(table_id).await.unwrap();
        let mut initial_data = [0u8; PAGE_SIZE];
        initial_data[0] = 77;
        initial_data[PAGE_SIZE - 1] = 88;
        disk_manager
            .write_page(page_id, &initial_data)
            .await
            .unwrap();

        // Act 1: Initialize BPM (0 pages cached)
        let bpm = BufferPoolManager::new(2, disk_manager);

        // Act 2: Fetch the page (Should trigger a Cache Miss and load from disk)
        let guard1 = bpm
            .fetch_page_read(page_id)
            .await
            .expect("Should fetch page from disk");

        // Assert 1: Verify it correctly read the data from disk
        assert_eq!(77, guard1[0], "First byte should match disk");
        assert_eq!(88, guard1[PAGE_SIZE - 1], "Last byte should match disk");

        // Assert 2: Verify it is in the page table with pin count = 1
        assert_eq!(
            Some(1),
            bpm.get_pin_count(page_id),
            "Frame should be pinned once"
        );

        // Act 3: Fetch the SAME page again (Should trigger a Cache Hit)
        let _guard2 = bpm
            .fetch_page_read(page_id)
            .await
            .expect("Should hit cache");

        // Assert 3: Verify it did not allocate a new frame, but incremented the pin count of the existing one!
        assert_eq!(
            Some(2),
            bpm.get_pin_count(page_id),
            "Cache hit should increment pin count to 2"
        );
        assert_eq!(
            1,
            bpm.get_free_frame_count(),
            "Should only have consumed 1 free frame total"
        );
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

    //     #[tokio::test]
    //     async fn test_bpm_fetch_page_write_cache_hit() {
    //         let dir = tempdir().unwrap();
    //         let fs = Arc::new(TokioFileSystem::new());
    //         let disk_manager = Arc::new(DiskManager::new(fs, dir.path().to_path_buf()));

    //         let table_id = 300;
    //         disk_manager
    //             .create_table_file(table_id)
    //             .await
    //             .expect("Should create table file");

    //         let bpm = BufferPoolManager::new(2, disk_manager);

    //         // Act 1: Create a brand new page and write initial data
    //         let page_id = {
    //             let mut page_guard = bpm.create_page(table_id).await.expect("Should create page");
    //             page_guard[0] = 11;
    //             page_guard[1] = 22;
    //             page_guard.mark_dirty();
    //             page_guard.page_id()
    //         };

    //         // Act 2: Fetch the SAME page for writing (Cache Hit)
    //         {
    //             let mut write_guard = bpm
    //                 .fetch_page_write(page_id)
    //                 .await
    //                 .expect("Should fetch page for writing successfully");

    //             // Verify the old data is there
    //             assert_eq!(11, write_guard[0]);
    //             assert_eq!(22, write_guard[1]);

    //             // Mutate the data
    //             write_guard[0] = 33;
    //             write_guard[1] = 44;
    //             write_guard.mark_dirty();
    //         } // write_guard drops, frame unpins

    //         // Act 3: Fetch for reading to verify the second mutation stuck
    //         let read_guard = bpm
    //             .fetch_page_read(page_id)
    //             .await
    //             .expect("Should fetch page successfully");

    //         assert_eq!(33, read_guard[0]);
    //         assert_eq!(44, read_guard[1]);
    //     }

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
        // --- CUSTOM MOCK TO EXPOSE RACE CONDITIONS ---
        // By wrapping the file system and injecting an artificial 50ms delay,
        // we guarantee the Leader will yield its thread to the Followers.
        // This forces the Followers to see the page table entry while the read is
        // still strictly "in-flight". If they don't wait on the channel, they will read 0s!
        struct SlowFileSystem(TokioFileSystem);
        
        #[async_trait::async_trait]
        impl FileSystem for SlowFileSystem {
            async fn create_dir_all(&self, path: &std::path::Path) -> std::io::Result<()> { self.0.create_dir_all(path).await }
            async fn create_file(&self, path: &std::path::Path) -> std::io::Result<Box<dyn crate::file_system::FileHandle>> {
                let inner = self.0.create_file(path).await?;
                Ok(Box::new(SlowFileHandle(inner)))
            }
            async fn delete_file(&self, path: &std::path::Path) -> std::io::Result<()> { self.0.delete_file(path).await }
            async fn file_exists(&self, path: &std::path::Path) -> std::io::Result<bool> { self.0.file_exists(path).await }
            async fn open_file(&self, path: &std::path::Path) -> std::io::Result<Box<dyn crate::file_system::FileHandle>> {
                let inner = self.0.open_file(path).await?;
                Ok(Box::new(SlowFileHandle(inner)))
            }
        }

        struct SlowFileHandle(Box<dyn crate::file_system::FileHandle>);
        
        #[async_trait::async_trait]
        impl crate::file_system::FileHandle for SlowFileHandle {
            async fn len(&self) -> std::io::Result<u64> { self.0.len().await }
            async fn read_at(&self, buffer: &mut [u8], offset: u64) -> std::io::Result<usize> {
                println!("  [Disk] Leader thread yielding for artificial disk delay...");
                tokio::time::sleep(std::time::Duration::from_millis(50)).await;
                println!("  [Disk] Leader thread finished disk read delay!");
                self.0.read_at(buffer, offset).await
            }
            async fn set_len(&self, len: u64) -> std::io::Result<()> { self.0.set_len(len).await }
            async fn write_at(&self, data: &[u8], offset: u64) -> std::io::Result<()> { self.0.write_at(data, offset).await }
        }

        let dir = tempdir().unwrap();
        let fs = Arc::new(SlowFileSystem(TokioFileSystem::new()));
        let disk_manager = Arc::new(DiskManager::new(
            Arc::clone(&fs) as Arc<dyn crate::file_system::FileSystem>, 
            dir.path().to_path_buf()
        ));

        let table_id = 500;
        disk_manager
            .create_table_file(table_id)
            .await
            .expect("Should create table file");

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

        // FIX: We only need ONE barrier now. 
        // We sacrifice the frozen-state pin_count check to avoid deadlocking the Leader.
        let barrier_start = Arc::new(std::sync::Barrier::new(11));

        let mut handles = vec![];

        for i in 0..10 {
            let bpm_clone = Arc::clone(&bpm);
            let barrier_start_clone = Arc::clone(&barrier_start);

            handles.push(std::thread::spawn(move || {
                let rt = tokio::runtime::Builder::new_current_thread()
                    .enable_all()
                    .build()
                    .unwrap();

                rt.block_on(async move {
                    barrier_start_clone.wait();
                    println!("[Worker {}] Rushing the cache!", i);

                    let frame = bpm_clone.fetch_and_pin_frame(page_id).await.unwrap();
                    
                    // Scope the read guard so we drop the RwLock BEFORE asserting.
                    // This prevents us from deadlocking the Leader who needs the write lock!
                    let first_byte = {
                        let read_lock = frame.read_data();
                        let read_guard = PageReadGuard::new(bpm_clone.evictor.as_ref(), frame, read_lock);
                        println!("[Worker {}] Acquired read_data() lock!", i);
                        read_guard[0]
                    }; // read_guard AND read_lock drop here!

                    // Now assert! If this is a Follower that bypassed the wait queue,
                    // it will read 0 instead of 99 and panic cleanly right here.
                    assert_eq!(
                        99, 
                        first_byte, 
                        "Follower thread read garbage data! It did not wait for the leader's IO channel to finish."
                    );
                });
            }));
        }

        let b_start_main = Arc::clone(&barrier_start);
        tokio::task::spawn_blocking(move || b_start_main.wait()).await.unwrap();

        // Wait for all tasks to finish. If any follower read garbage data, 
        // it will have panicked, and this unwrap() will cleanly fail the test.
        for handle in handles {
            handle.join().unwrap();
        }

        // If we reach here, the wait queue is implemented correctly!
        assert_eq!(
            9,
            bpm.get_free_frame_count(),
            "Exactly 1 free frame should be consumed."
        );
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

    //     #[tokio::test]
    //     async fn test_bpm_evicts_dirty_frame_and_writes_to_disk() {
    //         let dir = tempdir().unwrap();
    //         let fs = Arc::new(TokioFileSystem::new());
    //         let disk_manager = Arc::new(DiskManager::new(fs, dir.path().to_path_buf()));

    //         let table_id = 700;
    //         disk_manager
    //             .create_table_file(table_id)
    //             .await
    //             .expect("Should create table file");

    //         // Setup: Pre-allocate 2 pages directly on disk
    //         let page_id_1 = disk_manager.allocate_page(table_id).await.unwrap();
    //         let page_id_2 = disk_manager.allocate_page(table_id).await.unwrap();

    //         // Act 1: Initialize BPM with ONLY 1 frame to force immediate evictions
    //         let bpm = BufferPoolManager::new(1, disk_manager);

    //         // Act 2: Fetch Page 1, modify it, and mark it DIRTY
    //         {
    //             let mut guard1 = bpm.fetch_page_write(page_id_1).await.unwrap();

    //             // Write some recognizable magic bytes
    //             guard1[0] = 123;
    //             guard1[1] = 234;

    //             // CRITICAL: We tell the guard that this page has been modified
    //             guard1.mark_dirty();
    //         }
    //         // guard1 drops here.
    //         // The pin count for Frame 0 drops to 0, and the evictor is notified via `evictor.add()`.

    //         // Act 3: Fetch Page 2.
    //         // We only have 1 frame, so this forces a cache miss. The BPM MUST consult the evictor,
    //         // select Frame 0 as the victim, and recognize that Frame 0 is dirty.
    //         // It MUST write Page 1 to disk before loading Page 2!
    //         {
    //             let guard2 = bpm.fetch_page_read(page_id_2).await.unwrap();

    //             // Just verifying we got Page 2 successfully (it should be empty/zeroed out)
    //             assert_eq!(0, guard2[0], "Page 2 should be empty/zeroed");
    //         }
    //         // guard2 drops here. Frame 0 is now unpinned again, holding Page 2.

    //         // Act 4: Fetch Page 1 AGAIN.
    //         // This forces another cache miss, evicting Page 2, and reading Page 1 back from disk.
    //         let guard1_reloaded = bpm.fetch_page_read(page_id_1).await.unwrap();

    //         // Assert: The Phantom Data Check!
    //         // If the BPM didn't flush the dirty page to disk during Act 3,
    //         // it will just read the original, empty page from disk, and these assertions will fail!
    //         assert_eq!(
    //             123, guard1_reloaded[0],
    //             "Dirty page was not flushed to disk before eviction!"
    //         );
    //         assert_eq!(
    //             234, guard1_reloaded[1],
    //             "Dirty page was not flushed to disk before eviction!"
    //         );
    //     }

    #[tokio::test]
    async fn test_bpm_fetch_page_returns_error_when_all_frames_pinned() {
        let dir = tempdir().unwrap();
        let fs = Arc::new(TokioFileSystem::new());
        let disk_manager = Arc::new(DiskManager::new(fs, dir.path().to_path_buf()));

        let table_id = 800;
        disk_manager
            .create_table_file(table_id)
            .await
            .expect("Should create table file");

        let page_id_1 = disk_manager.allocate_page(table_id).await.unwrap();
        let page_id_2 = disk_manager.allocate_page(table_id).await.unwrap();

        // Act 1: Initialize BPM with ONLY 1 frame
        let bpm = BufferPoolManager::new(1, disk_manager);

        // Act 2: Fetch Page 1 and HOLD the guard.
        // Frame 0 now has a pin_count of 1 and is removed from the evictor.
        let _guard1 = bpm.fetch_page_read(page_id_1).await.unwrap();

        // Act 3: Attempt to fetch Page 2.
        // The pool is full, and the evictor has 0 eligible victims.
        // This MUST gracefully return an error (or None), not panic or deadlock!
        let result = bpm.fetch_page_read(page_id_2).await;

        assert!(
            result.is_err(),
            "BPM should return an error when no frames are available for eviction"
        );
    }

    //     #[tokio::test]
    //     async fn test_bpm_create_page_evicts_frame_when_full() {
    //         let dir = tempdir().unwrap();
    //         let fs = Arc::new(TokioFileSystem::new());
    //         let disk_manager = Arc::new(DiskManager::new(fs, dir.path().to_path_buf()));

    //         let table_id = 900;
    //         disk_manager
    //             .create_table_file(table_id)
    //             .await
    //             .expect("Should create table file");

    //         // Setup: Pre-allocate 1 page directly on disk
    //         let page_id_1 = disk_manager.allocate_page(table_id).await.unwrap();

    //         // Act 1: Initialize BPM with ONLY 1 frame
    //         let bpm = BufferPoolManager::new(1, disk_manager);

    //         // Act 2: Fill the buffer pool completely
    //         {
    //             let _guard1 = bpm.fetch_page_read(page_id_1).await.unwrap();
    //         } // guard1 goes out of scope, Frame 0 is unpinned and eligible for eviction

    //         // Act 3: Create a NEW page!
    //         // This should trigger the DiskManager to allocate a new page,
    //         // AND trigger the BPM to evict Page 1 to make room for it in memory.
    //         let guard2 = bpm
    //             .create_page(table_id)
    //             .await
    //             .expect("Should successfully evict a frame and create a new page");

    //         // Assert: We got a new page with the next sequential index (1)
    //         assert_eq!(
    //             1,
    //             guard2.page_id().page_index,
    //             "The newly created page should have index 1"
    //         );
    //     }
}
