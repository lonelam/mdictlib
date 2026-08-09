use std::fs;
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

use mdictlib::{Error, MdxFile};

#[test]
fn rejects_bad_header_checksum() {
    let xml = "<Dictionary GeneratedByEngineVersion=\"2.0\" RequiredEngineVersion=\"2.0\" Encoding=\"UTF-8\"/>";
    let utf16 = utf16le(xml);
    let mut bytes = Vec::new();
    bytes.extend_from_slice(&(utf16.len() as u32).to_be_bytes());
    bytes.extend_from_slice(&utf16);
    bytes.extend_from_slice(&0xdead_beefu32.to_le_bytes());

    let path = write_temp_file("bad-header", &bytes);
    let error = MdxFile::open(&path).unwrap_err();
    fs::remove_file(path).ok();

    assert!(matches!(error, Error::ChecksumMismatch { .. }));
}

fn utf16le(text: &str) -> Vec<u8> {
    let mut out = Vec::new();
    for unit in text.encode_utf16() {
        out.extend_from_slice(&unit.to_le_bytes());
    }
    out
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
