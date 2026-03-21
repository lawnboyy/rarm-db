use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{RwLock, RwLockReadGuard, RwLockWriteGuard};

//use crate::page_id;
use crate::{PageId, page_id::PAGE_SIZE};

pub struct Frame {
    data: RwLock<[u8; PAGE_SIZE]>,
    id: usize,
    is_dirty: AtomicBool,
    page_id: RwLock<Option<PageId>>,
    pin_count: AtomicUsize,
}

impl Frame {
    pub fn new(id: usize) -> Self {
        Frame {
            data: RwLock::new([0u8; PAGE_SIZE]),
            id,
            page_id: RwLock::new(None),
            pin_count: 0.into(),
            is_dirty: false.into(),
        }
    }

    pub fn decrement_pin_count(&self) -> usize {
        // Read the pin count, decrement the value, write back in a single atomic operation.
        let prev_count = self.pin_count.fetch_sub(1, Ordering::SeqCst);
        assert!(
            prev_count > 0,
            "Pin count underflow! Attempted to unpin a frame that was not pinned."
        );

        prev_count
    }

    pub fn get_id(&self) -> usize {
        self.id
    }

    pub fn get_page_id(&self) -> Option<PageId> {
        // Read the page ID from the lock and unwrap it to get the guard.
        // Then dereference to return the a copy of the page ID (because
        // it derives the Copy trait).
        *self.page_id.read().unwrap()
    }

    pub fn get_pin_count(&self) -> usize {
        self.pin_count.load(Ordering::SeqCst)
    }

    pub fn increment_pin_count(&self) -> usize {
        // Read the pin count, add 1, and write the value out in a single atomic operation.
        self.pin_count.fetch_add(1, Ordering::SeqCst)
    }

    pub fn is_dirty(&self) -> bool {
        self.is_dirty.load(Ordering::SeqCst)
    }

    pub fn read_data(&self) -> RwLockReadGuard<'_, [u8; PAGE_SIZE]> {
        self.data.read().unwrap()
    }

    pub fn set_dirty(&self, val: bool) {
        self.is_dirty.store(val, Ordering::SeqCst);
    }

    pub fn set_page_id(&self, page_id: Option<PageId>) {
        // Get a write guard...
        let mut write_guard = self.page_id.write().unwrap();
        *write_guard = page_id;
    }

    pub fn write_data(&self) -> RwLockWriteGuard<'_, [u8; PAGE_SIZE]> {
        self.data.write().unwrap()
    }
}

impl Default for Frame {
    fn default() -> Self {
        Self::new(0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_frame_new_initializes_empty_state() {
        // Act
        let frame = Frame::new(0);

        // Assert
        assert_eq!(
            None,
            frame.get_page_id().as_ref(),
            "New frame should not have a page ID assigned"
        );

        assert_eq!(
            0,
            frame.get_pin_count(),
            "New frame should have 0 pin count"
        );

        assert!(!frame.is_dirty(), "New frame should not be dirty");

        // Verify the buffer is completely zeroed out by acquiring the read lock
        let buffer_guard = frame.read_data();
        let expected_buffer = [0u8; PAGE_SIZE];

        assert_eq!(
            expected_buffer.as_ref(),
            buffer_guard.as_ref(),
            "New frame buffer should be zero-filled"
        );
    }

    #[test]
    fn test_frame_metadata_and_data_mutability() {
        let frame = Frame::new(0);

        // 1. Test Page ID mutation
        // Note: You will need to add a RwLock around the PageId, or use an Atomic/Mutex
        // to safely mutate it if it's shared, but a standard RwLock is great here.
        let page_id = PageId {
            table_id: 10,
            page_index: 5,
        };
        frame.set_page_id(Some(page_id));
        assert_eq!(
            Some(page_id),
            frame.get_page_id(),
            "Frame should return the newly assigned page ID"
        );

        // 2. Test Pin Count atomics
        frame.increment_pin_count();
        frame.increment_pin_count();
        assert_eq!(
            2,
            frame.get_pin_count(),
            "Pin count should be 2 after two increments"
        );

        frame.decrement_pin_count();
        assert_eq!(
            1,
            frame.get_pin_count(),
            "Pin count should be 1 after one decrement"
        );

        // 3. Test Dirty Flag atomics
        frame.set_dirty(true);
        assert!(frame.is_dirty(), "Frame should be marked as dirty");

        frame.set_dirty(false);
        assert!(!frame.is_dirty(), "Frame should be marked as clean");

        // 4. Test Data Buffer mutability
        {
            // Acquire the exclusive write lock
            let mut write_guard = frame.write_data();
            write_guard[0] = 42;
            write_guard[PAGE_SIZE - 1] = 99;
        } // The write_guard goes out of scope here, automatically releasing the RwLock!

        // Acquire the shared read lock to verify the writes
        let read_guard = frame.read_data();
        assert_eq!(42, read_guard[0], "First byte should be mutated");
        assert_eq!(99, read_guard[PAGE_SIZE - 1], "Last byte should be mutated");
        assert_eq!(0, read_guard[1], "Untouched bytes should remain zero");
    }
}
