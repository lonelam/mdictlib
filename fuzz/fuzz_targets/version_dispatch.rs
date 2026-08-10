#![no_main]

//! Drives the single version-dispatch point with mismatched headers and bodies.
//!
//! A version 1 header over a version 2 body — and the reverse — must fail
//! cleanly in the selected grammar. If either ever succeeded, some code path
//! would be retrying the other grammar, which the design forbids.

#[path = "../support/mod.rs"]
mod support;

use libfuzzer_sys::fuzz_target;
use support::{Kind, Wire, exercise_bytes, fixture_for, mutate_region, set_declared_major_version};

fuzz_target!(|data: &[u8]| {
    for kind in [Kind::Mdx, Kind::Mdd] {
        for wire in [Wire::V1, Wire::V2] {
            let mut fixture = fixture_for(wire, kind);
            let selector = data.first().copied().unwrap_or(0);
            // Declare a major version that may or may not match the body.
            set_declared_major_version(&mut fixture, selector % 4);
            if let Some(payload) = data.get(1..) {
                let range = fixture.layout.keyword_header.clone();
                mutate_region(&mut fixture.bytes, &range, payload);
            }
            let _ = exercise_bytes(kind, "version-dispatch", &fixture.bytes, true);
        }
    }
});
