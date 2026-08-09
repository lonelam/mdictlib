#![no_main]

#[path = "../support/mod.rs"]
mod support;

use libfuzzer_sys::fuzz_target;
use support::{Kind, corrupt_block_envelope, exercise_bytes, fixture, mutate_block_payload};

fuzz_target!(|data: &[u8]| {
    for kind in [Kind::Mdx, Kind::Mdd] {
        let mut fixture = fixture(kind);
        let selector = data.first().copied().unwrap_or(0);
        let block_index = usize::from(selector) % fixture.layout.key_blocks.len();
        let range = fixture.layout.key_blocks[block_index].clone();
        mutate_block_payload(
            &mut fixture.bytes,
            &range,
            data.get(1..).unwrap_or_default(),
        );
        if selector & 0x0f == 0x0f {
            corrupt_block_envelope(&mut fixture.bytes, &range, data);
        }
        let opened = exercise_bytes(kind, "key-block", &fixture.bytes, false);
        if data.is_empty() {
            assert!(opened, "the unmodified structured fixture must open");
        }
    }
});
