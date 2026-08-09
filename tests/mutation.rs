use std::fs;
use std::panic;
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

use mdictlib::MdxFile;

#[test]
fn mutated_header_files_do_not_panic() {
    let bytes = minimal_header_only_mdx();
    for index in 0..bytes.len() {
        let mut mutated = bytes.clone();
        mutated[index] ^= 0xa5;
        let path = write_temp_file("mutated-header", &mutated);
        let result = panic::catch_unwind(|| {
            let _ = MdxFile::open(&path);
        });
        fs::remove_file(path).ok();
        assert!(result.is_ok(), "MdxFile::open panicked at byte {index}");
    }
}

fn minimal_header_only_mdx() -> Vec<u8> {
    let xml = "<Dictionary GeneratedByEngineVersion=\"2.0\" RequiredEngineVersion=\"2.0\" Encoding=\"UTF-8\"/>";
    let utf16 = utf16le(xml);
    let mut bytes = Vec::new();
    bytes.extend_from_slice(&(utf16.len() as u32).to_be_bytes());
    bytes.extend_from_slice(&utf16);
    bytes.extend_from_slice(&adler32(&utf16).to_le_bytes());
    bytes
}

fn utf16le(text: &str) -> Vec<u8> {
    let mut out = Vec::new();
    for unit in text.encode_utf16() {
        out.extend_from_slice(&unit.to_le_bytes());
    }
    out
}

fn adler32(bytes: &[u8]) -> u32 {
    const MOD_ADLER: u32 = 65_521;
    let mut first = 1u32;
    let mut second = 0u32;
    for byte in bytes {
        first = (first + u32::from(*byte)) % MOD_ADLER;
        second = (second + first) % MOD_ADLER;
    }
    (second << 16) | first
}

fn write_temp_file(prefix: &str, bytes: &[u8]) -> PathBuf {
    let unique = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let path = std::env::temp_dir().join(format!("{prefix}-{unique}.mdx"));
    fs::write(&path, bytes).unwrap();
    path
}
