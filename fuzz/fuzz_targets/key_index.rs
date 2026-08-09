#![no_main]

#[path = "../support/mod.rs"]
mod support;

use libfuzzer_sys::fuzz_target;
use support::{Kind, corrupt_block_envelope, exercise_bytes, fixture, mutate_block_payload};

fuzz_target!(|data: &[u8]| {
    for kind in [Kind::Mdx, Kind::Mdd] {
        let mut fixture = fixture(kind);
        let range = fixture.layout.key_index.clone();
        mutate_block_payload(&mut fixture.bytes, &range, data);
        if data.first().is_some_and(|selector| selector & 0x07 == 0x07) {
            corrupt_block_envelope(&mut fixture.bytes, &range, data);
        }
        let opened = exercise_bytes(kind, "key-index", &fixture.bytes, false);
        if data.is_empty() {
            assert!(opened, "the unmodified structured fixture must open");
        }
    }
});
