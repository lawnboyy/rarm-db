use std::sync::Mutex;

pub trait Evictor: Send + Sync {
    // Adds a frame at the given index/ID to the evictor, making
    // it eligible for eviction.
    fn add(&self, frame_id: usize);

    // Removes a frame from the evictor making it ineligible for
    // eviction.
    fn remove(&self, frame_id: usize);

    // The number of frames eligible for eviction.
    fn size(&self) -> usize;

    // Evicts a frame and returns the frame ID.
    fn victim(&self) -> Option<usize>;
}

pub struct ClockEvictorState {
    // Index that the clock hand currently points to.
    clock_hand: usize,
    // For each frame in the evictor we'll track a reference bit as a second chance.
    // This will be set to true when a frame is added to the evictor. When the clock
    // hand sweeps to find a victim, if the reference bit is true, it will be set
    // to false and the clock hand will move to the next eligible frame. Once the
    // hand lands on an eligible frame with a reference bit set to false, it will
    // evict that frame.
    second_chances: Vec<bool>,

    // Look up table that indicates if a frame is eligible for eviction.
    is_in_evictor: Vec<bool>,

    // This is the clock hand position which is bounded by [0..pool_size -1].
    // hand_position: usize,

    // The size refers to the number of evicted frames available for reuse by the BPM.
    size: usize,
}

pub struct ClockEvictor {
    state: Mutex<ClockEvictorState>,
}

impl ClockEvictor {
    pub fn new(pool_size: usize) -> Self {
        let state = Mutex::new(ClockEvictorState {
            clock_hand: 0,
            is_in_evictor: vec![false; pool_size],
            // hand_position: 0,
            second_chances: vec![false; pool_size],
            size: 0,
        });

        ClockEvictor { state }
    }
}

impl Evictor for ClockEvictor {
    fn add(&self, frame_id: usize) {
        let mut state = self.state.lock().unwrap();

        if !state.is_in_evictor[frame_id] {
            state.is_in_evictor[frame_id] = true;
            state.size += 1;
        }

        // Always flip this to true when unpinning. If it was false, that
        // means a clock sweep flipped it to false, but now we are unpinning
        // it again, meaning there has been another interaction with this frame,
        // earning it a second chance to remain cached.
        state.second_chances[frame_id] = true;
    }

    fn remove(&self, frame_id: usize) {
        let mut state = self.state.lock().unwrap();
        // Only update the state here if the frame is currently in the evictor.
        if state.is_in_evictor[frame_id] {
            state.is_in_evictor[frame_id] = false;
            state.size -= 1;
        }
    }

    fn size(&self) -> usize {
        let state = self.state.lock().unwrap();
        state.size
    }

    fn victim(&self) -> Option<usize> {
        // Use the clock replacer aglorithm to find a victim frame.
        // First lock mutex to access the inner state...
        let mut state = self.state.lock().unwrap();
        let mut i = state.clock_hand;
        let len = state.is_in_evictor.len();
        // Loop through the added frames at least twice and check their reference bit.
        for _ in 0..(len * 2) {
            // Only consider frames that have been added to the evictor
            if state.is_in_evictor[i] {
                if !state.second_chances[i] {
                    // Remove the frame from the evictor and return the frame ID.
                    state.is_in_evictor[i] = false;
                    state.size -= 1;
                    // Move the clock hand position past the evicted frame.
                    state.clock_hand = (i + 1) % len;
                    return Some(i);
                } else {
                    state.second_chances[i] = false;
                }
            }

            // Cyclicly iterate through the elligible frames.
            i = (i + 1) % len;
        }

        state.clock_hand = i;
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_clock_replacer_initializes_empty() {
        let replacer = ClockEvictor::new(3);

        assert_eq!(0, replacer.size(), "New replacer should have size 0");
        assert_eq!(
            None,
            replacer.victim(),
            "New replacer should have no victim"
        );
    }

    #[test]
    fn test_clock_evictor_unpin_and_sweep() {
        let evictor = ClockEvictor::new(3);

        // Unpinning a frame adds it to the evictor
        evictor.add(0);
        assert_eq!(
            1,
            evictor.size(),
            "Size should be 1 after unpinning frame 0"
        );

        // Unpinning the same frame again should not increase the size
        evictor.add(0);
        assert_eq!(
            1,
            evictor.size(),
            "Size should remain 1 when unpinning an already eligible frame"
        );

        // First victim sweep:
        // 1. Hand looks at frame 0 (ref_bit=true). Sets ref_bit=false, moves to 1.
        // 2. Hand skips 1 and 2 (not in evictor).
        // 3. Hand looks at frame 0 again (ref_bit=false). Evicts 0!
        assert_eq!(Some(0), evictor.victim(), "Should sweep and evict frame 0");

        // Size should be back to 0
        assert_eq!(0, evictor.size(), "Size should be 0 after eviction");
        assert_eq!(None, evictor.victim(), "Should have no victim when empty");
    }

    #[test]
    fn test_clock_evictor_second_chance_and_hand_position() {
        let evictor = ClockEvictor::new(4);

        // Setup: Unpin 0, 1, and 2. Hand starts at 0.
        // State: [0: true, 1: true, 2: true, 3: empty]
        evictor.add(0);
        evictor.add(1);
        evictor.add(2);

        // First Sweep:
        // - Hand at 0 (true -> false), advances to 1
        // - Hand at 1 (true -> false), advances to 2
        // - Hand at 2 (true -> false), advances to 3
        // - Hand at 3 (skips)
        // - Hand at 0 (false -> EVICT). Returns 0. Hand advances to 1.
        assert_eq!(
            Some(0),
            evictor.victim(),
            "Should sweep all, clear bits, and evict 0"
        );

        // Current State: Hand is at 1. [1: false, 2: false] are in the evictor.

        // Re-arm: We unpin 2 again. This must flip its second chance back to true!
        evictor.add(2);

        // Second Sweep:
        // - Hand starts at 1. Its bit is still false -> EVICT immediately! Returns 1. Hand advances to 2.
        assert_eq!(
            Some(1),
            evictor.victim(),
            "Should evict 1 immediately because its bit was left false from the first sweep"
        );

        // Third Sweep:
        // - Hand starts at 2. Its bit is TRUE (because we re-armed it).
        // - Hand at 2 (true -> false), advances to 3.
        // - Hand loops through 3, 0, 1 (all empty).
        // - Hand at 2 (false -> EVICT). Returns 2. Hand advances to 3.
        assert_eq!(
            Some(2),
            evictor.victim(),
            "Should sweep, clear 2's renewed bit, loop around, and then evict 2"
        );

        assert_eq!(None, evictor.victim(), "Should be empty now");
    }

    #[test]
    fn test_clock_evictor_pin_removes_from_evictor() {
        let evictor = ClockEvictor::new(3);

        // Setup: Unpin all frames
        evictor.add(0);
        evictor.add(1);
        evictor.add(2);
        assert_eq!(3, evictor.size());

        // Pinning frame 1 should remove it from the evictor entirely
        evictor.remove(1);
        assert_eq!(2, evictor.size(), "Size should decrease after pinning");

        // First Sweep:
        // - Hand at 0 (true -> false), advances to 1
        // - Hand at 1 (skips, because it was pinned!), advances to 2
        // - Hand at 2 (true -> false), advances to 0
        // - Hand at 0 (false -> EVICT). Returns 0. Hand advances to 1.
        assert_eq!(
            Some(0),
            evictor.victim(),
            "Should sweep 0, skip 1, sweep 2, loop around and evict 0"
        );

        // Next Sweep:
        // - Hand starts at 1 (skips again)
        // - Hand at 2 (false -> EVICT). Returns 2. Hand advances to 0.
        assert_eq!(Some(2), evictor.victim(), "Should skip 1 and evict 2");

        // Now empty
        assert_eq!(0, evictor.size());
        assert_eq!(
            None,
            evictor.victim(),
            "Should be empty after evicting the remaining eligible frames"
        );
    }
}
