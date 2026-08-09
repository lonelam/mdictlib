#![no_main]

use libfuzzer_sys::fuzz_target;
use mdictlib::fuzzing::exercise_compressed_block;

fuzz_target!(|data: &[u8]| {
    for expected_len in [0usize, 1, 8, 32, 256, 4096] {
        exercise_compressed_block(data, expected_len);
    }
});
