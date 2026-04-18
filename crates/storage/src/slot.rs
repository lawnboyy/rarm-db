pub const SIZE: u16 = (size_of::<u16>() + size_of::<u16>()) as u16;

pub struct Slot {
    pub record_offset: u16,
    pub record_length: u16,
}
