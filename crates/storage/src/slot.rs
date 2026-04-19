pub const SLOT_SIZE: usize = size_of::<u16>() + size_of::<u16>();

pub struct Slot {
    pub record_offset: u16,
    pub record_length: u16,
}
