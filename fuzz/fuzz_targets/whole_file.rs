#![no_main]

#[path = "../support/mod.rs"]
mod support;

use libfuzzer_sys::fuzz_target;
use support::{Kind, MAX_WHOLE_FILE_INPUT, exercise_bytes};

fuzz_target!(|data: &[u8]| {
    let bounded = &data[..data.len().min(MAX_WHOLE_FILE_INPUT)];
    let _ = exercise_bytes(Kind::Mdx, "whole-file-mdx", bounded, false);
    let _ = exercise_bytes(Kind::Mdd, "whole-file-mdd", bounded, false);
});
