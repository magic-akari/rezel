/// Read one little-endian 32-bit value split over two compact words.
#[must_use]
pub fn pair(data: &[u16], offset: usize) -> u32 {
    u32::from(data[offset]) | (u32::from(data[offset + 1]) << 16)
}
