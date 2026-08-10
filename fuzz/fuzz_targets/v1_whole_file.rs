#![no_main]

//! Mutates every region of a structurally valid version 1 file.
//!
//! One target covers the version 1 keyword header, raw keyword metadata, lazy
//! key blocks, record header, record index, and record blocks, because the
//! version 1 grammar validates them against each other: mutating one in
//! isolation mostly exercises the same reconciliation code.

#[path = "../support/mod.rs"]
mod support;

use libfuzzer_sys::fuzz_target;
use support::{
    Kind, corrupt_block_envelope, exercise_bytes, fixture_v1, mutate_block_payload, mutate_region,
};

fuzz_target!(|data: &[u8]| {
    for kind in [Kind::Mdx, Kind::Mdd] {
        let mut fixture = fixture_v1(kind);
        let selector = data.first().copied().unwrap_or(0);
        let payload = data.get(1..).unwrap_or_default();

        match selector % 6 {
            0 => {
                let range = fixture.layout.keyword_header.clone();
                mutate_region(&mut fixture.bytes, &range, payload);
            }
            1 => {
                // Raw metadata: no envelope, so mutate it directly.
                let range = fixture.layout.key_index.clone();
                mutate_region(&mut fixture.bytes, &range, payload);
            }
            2 => {
                let index = usize::from(selector) % fixture.layout.key_blocks.len();
                let range = fixture.layout.key_blocks[index].clone();
                mutate_block_payload(&mut fixture.bytes, &range, payload);
                if selector & 0x40 != 0 {
                    corrupt_block_envelope(&mut fixture.bytes, &range, payload);
                }
            }
            3 => {
                let range = fixture.layout.record_header.clone();
                mutate_region(&mut fixture.bytes, &range, payload);
            }
            4 => {
                let range = fixture.layout.record_index.clone();
                mutate_region(&mut fixture.bytes, &range, payload);
            }
            _ => {
                let index = usize::from(selector) % fixture.layout.record_blocks.len();
                let range = fixture.layout.record_blocks[index].clone();
                mutate_block_payload(&mut fixture.bytes, &range, payload);
                if selector & 0x40 != 0 {
                    corrupt_block_envelope(&mut fixture.bytes, &range, payload);
                }
            }
        }

        let opened = exercise_bytes(kind, "v1-whole-file", &fixture.bytes, true);
        if data.is_empty() {
            assert!(opened, "the unmodified version 1 fixture must open");
        }
    }
});
