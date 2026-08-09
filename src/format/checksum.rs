use crate::error::{Error, Result};

const MOD_ADLER: u32 = 65_521;

pub fn adler32(bytes: &[u8]) -> u32 {
    let mut a = 1u32;
    let mut b = 0u32;

    for byte in bytes {
        a = (a + u32::from(*byte)) % MOD_ADLER;
        b = (b + a) % MOD_ADLER;
    }

    (b << 16) | a
}

pub fn verify_adler32(context: &'static str, bytes: &[u8], expected: u32) -> Result<()> {
    let actual = adler32(bytes);
    if actual == expected {
        Ok(())
    } else {
        Err(Error::ChecksumMismatch {
            context,
            expected,
            actual,
        })
    }
}
