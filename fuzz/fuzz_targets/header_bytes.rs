#![no_main]

use libfuzzer_sys::fuzz_target;
use mdictlib::fuzzing::exercise_header_bytes;

fuzz_target!(|data: &[u8]| {
    exercise_header_bytes(data);
});
