#![no_main]

#[path = "../support/mod.rs"]
mod support;

use libfuzzer_sys::fuzz_target;
use support::{
    Kind, corrupt_block_envelope, exercise_bytes, fixture, mutate_block_payload,
    mutate_record_offsets,
};

fuzz_target!(|data: &[u8]| {
    let mut fixture = fixture(Kind::Mdd);
    mutate_record_offsets(&mut fixture, data);

    let selector = data.first().copied().unwrap_or(0);
    let block_index = usize::from(selector) % fixture.layout.record_blocks.len();
    let range = fixture.layout.record_blocks[block_index].clone();
    mutate_block_payload(
        &mut fixture.bytes,
        &range,
        data.get(1..).unwrap_or_default(),
    );
    if selector & 0x1f == 0x1f {
        corrupt_block_envelope(&mut fixture.bytes, &range, data);
    }

    let opened = exercise_bytes(Kind::Mdd, "record-span", &fixture.bytes, true);
    if data.is_empty() {
        assert!(opened, "the unmodified structured fixture must open");
    }
});
