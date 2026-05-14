#[derive(Debug, Clone)]
pub struct Bitfield {
    bytes: Vec<u8>,
}

impl Bitfield {
    pub fn empty() -> Self {
        Bitfield { bytes: Vec::new() }
    }

    pub fn from_bytes(bytes: &[u8]) -> Self {
        Bitfield {
            bytes: Vec::from(bytes),
        }
    }

    pub fn has_piece(&self, index: u32) -> bool {
        let byte_index = (index / 8) as usize;
        let bit = 7 - (index % 8);

        if byte_index >= self.bytes.len() {
            return false;
        }

        (self.bytes[byte_index] >> bit) & 1 == 1
    }

    pub fn set_piece(&mut self, index: u32) {
        let byte_index = (index / 8) as usize;
        let bit = 7 - (index % 8);

        debug_assert!(byte_index < self.bytes.len(), "...");
        if byte_index >= self.bytes.len() {
            return;
        }

        self.bytes[byte_index] |= 1 << bit;
    }

    pub fn count_set(&self) -> u32 {
        self.bytes.iter().map(|b| b.count_ones()).sum()
    }
}
