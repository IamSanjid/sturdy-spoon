use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub struct LengthNotMatched;

impl std::fmt::Display for LengthNotMatched {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "The provided `&str` didn't match length.")
    }
}

// To nicely align it
const NANOID_BYTES_LEN: usize = std::mem::size_of::<usize>() * 2;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[repr(transparent)]
pub struct NanoId([u8; NANOID_BYTES_LEN]);

impl NanoId {
    pub fn new() -> Self {
        // It guarantees it will generete "sizeof(usize) * 2" u8(s)
        let nanoid_str = nanoid::nanoid!(NANOID_BYTES_LEN);
        let mut nanoid: [u8; NANOID_BYTES_LEN] = [0; NANOID_BYTES_LEN];
        nanoid.copy_from_slice(nanoid_str.as_bytes());

        Self(nanoid)
    }
}

impl std::fmt::Display for NanoId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        // Safety: The `NanoId::new()` confirms all the generated bytes are valid utf8 bytes.
        unsafe { write!(f, "{}", std::str::from_utf8(&self.0).unwrap_unchecked()) }
    }
}

impl std::str::FromStr for NanoId {
    type Err = LengthNotMatched;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        if s.len() != NANOID_BYTES_LEN {
            return Err(LengthNotMatched);
        }

        let mut nanoid: [u8; NANOID_BYTES_LEN] = [0; NANOID_BYTES_LEN];
        nanoid.copy_from_slice(s.as_bytes());

        Ok(NanoId(nanoid))
    }
}
