#[derive(Debug, Clone)]
pub struct Bitfield {
    bytes: Vec<u8>,
}

impl Bitfield {
    pub fn from_bytes(bytes: &[u8]) -> Self {
        Self {
            bytes: Vec::from(bytes),
        }
    }

    // stub — flesh out in step 4
    pub fn has_piece(&self, _index: u32) -> bool {
        false
    }
}
