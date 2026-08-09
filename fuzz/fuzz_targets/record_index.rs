#![no_main]

#[path = "../support/mod.rs"]
mod support;

use libfuzzer_sys::fuzz_target;
use support::{Kind, exercise_bytes, fixture, mutate_region};

fuzz_target!(|data: &[u8]| {
    let mut fixture = fixture(Kind::Mdd);
    let selector = data.first().copied().unwrap_or(0);
    let range = if selector & 1 == 0 {
        fixture.layout.record_header.clone()
    } else {
        fixture.layout.record_index.clone()
    };
    mutate_region(
        &mut fixture.bytes,
        &range,
        data.get(1..).unwrap_or_default(),
    );
    let opened = exercise_bytes(Kind::Mdd, "record-index", &fixture.bytes, true);
    if data.is_empty() {
        assert!(opened, "the unmodified structured fixture must open");
    }
});
