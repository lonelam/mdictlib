#![no_main]

//! Truncates a valid version 1 file at every offset the input selects.
//!
//! Version 1 has no keyword-header checksum, so truncation and geometry
//! reconciliation are the only defenses against a header that declares more
//! than the file holds.

#[path = "../support/mod.rs"]
mod support;

use libfuzzer_sys::fuzz_target;
use support::{Kind, exercise_bytes, fixture_v1};

fuzz_target!(|data: &[u8]| {
    for kind in [Kind::Mdx, Kind::Mdd] {
        let fixture = fixture_v1(kind);
        let total = fixture.bytes.len();
        let keep = match data.len() {
            0 => total,
            _ => {
                let selector = data
                    .iter()
                    .fold(0usize, |acc, byte| acc.wrapping_mul(31).wrapping_add(usize::from(*byte)));
                selector % (total + 1)
            }
        };
        let opened = exercise_bytes(kind, "v1-truncation", &fixture.bytes[..keep], true);
        if keep == total {
            assert!(opened, "the untruncated version 1 fixture must open");
        }
    }
});
