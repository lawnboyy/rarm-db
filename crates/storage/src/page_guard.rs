use std::ops::Deref;
use std::sync::RwLockReadGuard;

use crate::frame::Frame;
use crate::page_id::PAGE_SIZE;

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
}
