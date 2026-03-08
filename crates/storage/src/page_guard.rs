use std::ops::{Deref, DerefMut};
use std::sync::{RwLockReadGuard, RwLockWriteGuard};

use crate::PageId;
use crate::frame::Frame;
use crate::page_id::PAGE_SIZE;

/////////////////////////////////////////////////////////////////////////////////////////////////
/// Read guard that references the underlying frame data. This guard provides a convenient
/// mechanism for automatically unpinning a frame once the read guard is dropped.
/////////////////////////////////////////////////////////////////////////////////////////////////
pub struct PageReadGuard<'a> {
    frame: &'a Frame,
    lock_guard: RwLockReadGuard<'a, [u8; PAGE_SIZE]>,
}

impl<'a> PageReadGuard<'a> {
    pub fn new(frame: &'a Frame, lock_guard: RwLockReadGuard<'a, [u8; PAGE_SIZE]>) -> Self {
        Self { frame, lock_guard }
    }
}

// Point to the read lock guard when dereferencing a PageReadGuard struct.
impl Deref for PageReadGuard<'_> {
    type Target = [u8; PAGE_SIZE];

    fn deref(&self) -> &Self::Target {
        &self.lock_guard
    }
}

// Automatically decrement the pin count when the page read guard goes out of scope.
// This avoids the need for callers to manually decrement the pin count when the
// page is no longer needed.
impl Drop for PageReadGuard<'_> {
    fn drop(&mut self) {
        self.frame.decrement_pin_count();
    }
}

/////////////////////////////////////////////////////////////////////////////////////////////////
/// Write guard that references the underlying frame data. This guard provides a convenient
/// mechanism for automatically unpinning a frame once the write guard is dropped.
/////////////////////////////////////////////////////////////////////////////////////////////////
pub struct PageWriteGuard<'a> {
    frame: &'a Frame,
    lock_guard: RwLockWriteGuard<'a, [u8; PAGE_SIZE]>,
}

impl<'a> PageWriteGuard<'a> {
    pub fn new(frame: &'a Frame, lock_guard: RwLockWriteGuard<'a, [u8; PAGE_SIZE]>) -> Self {
        Self { frame, lock_guard }
    }

    pub fn mark_dirty(&self) {
        self.frame.set_dirty(true);
    }

    pub fn page_id(&self) -> PageId {
        self.frame.get_page_id().unwrap()
    }
}

// Point to the read lock guard when dereferencing a PageReadGuard struct.
impl Deref for PageWriteGuard<'_> {
    type Target = [u8; PAGE_SIZE];

    fn deref(&self) -> &Self::Target {
        &self.lock_guard
    }
}

// Automatically decrement the pin count when the page read guard goes out of scope.
// This avoids the need for callers to manually decrement the pin count when the
// page is no longer needed.
impl Drop for PageWriteGuard<'_> {
    fn drop(&mut self) {
        self.frame.decrement_pin_count();
    }
}

impl DerefMut for PageWriteGuard<'_> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.lock_guard
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_page_read_guard_decrements_pin_count_on_drop() {
        // Setup: Create a frame and simulate the Buffer Pool pinning it
        let frame = Frame::new();
        frame.increment_pin_count();
        assert_eq!(
            1,
            frame.get_pin_count(),
            "Pin count should be 1 after BPM fetches it"
        );

        {
            // Act: Create the guard
            // We pass in both the frame reference and the active lock guard
            let lock_guard = frame.read_data();
            let page_guard = PageReadGuard::new(&frame, lock_guard);

            // Assert 1: Transparent read access via Deref
            // If Deref is implemented correctly, we can index directly into the guard!
            assert_eq!(
                0, page_guard[0],
                "Should be able to read data directly through the guard"
            );
        } // page_guard goes out of scope here! Drop is called automatically.

        // Assert 2: The Drop trait should have automatically decremented the pin count
        assert_eq!(
            0,
            frame.get_pin_count(),
            "Pin count should automatically decrement when guard drops"
        );
    }

    #[test]
    fn test_page_write_guard_mutates_data_and_cleans_up_on_drop() {
        // Setup: Create a frame and simulate the Buffer Pool pinning it
        let frame = Frame::new();
        frame.increment_pin_count();
        assert_eq!(1, frame.get_pin_count());

        {
            // Act: Create the write guard
            let lock_guard = frame.write_data();

            // Note: Guard must be declared as `mut` because we are going to mutate its contents!
            let mut page_guard = PageWriteGuard::new(&frame, lock_guard);

            // Assert 1: Transparent write access via DerefMut
            page_guard[0] = 42;
            page_guard[PAGE_SIZE - 1] = 99;

            // Assert 2: Explicitly mark the frame as dirty
            page_guard.mark_dirty();
        } // page_guard goes out of scope here! Drop is called automatically.

        // Assert 3: The Drop trait should have automatically decremented the pin count
        assert_eq!(
            0,
            frame.get_pin_count(),
            "Pin count should automatically decrement when write guard drops"
        );

        // Assert 4: The frame should be marked as dirty
        assert!(
            frame.is_dirty(),
            "Frame should be marked as dirty after calling mark_dirty()"
        );

        // Assert 5: The mutations should be persisted in the frame
        let read_guard = frame.read_data();
        assert_eq!(42, read_guard[0]);
        assert_eq!(99, read_guard[PAGE_SIZE - 1]);
    }
}
