#![allow(dead_code)]

use std::collections::HashSet;
use std::env;
use std::ffi::OsStr;
use std::fs::{self, File};
use std::io::{self, Read};
use std::path::{Component, Path, PathBuf};

pub const CORPUS_ENV: &str = "MDICT_CORPUS_DIR";
pub const MANIFEST_NAME: &str = "mdictlib-corpus.tsv";

const MANIFEST_HEADER_V1: &str = "path\tkind\tbytes\tsha256\tentries";
const MANIFEST_HEADER_V2: &str = "path\tkind\tbytes\tsha256\tentries\tkey_sha256\tpayload_sha256";

#[derive(Debug, Clone, Copy)]
enum ManifestVersion {
    V1,
    V2,
}

impl ManifestVersion {
    const fn field_count(self) -> usize {
        match self {
            Self::V1 => 5,
            Self::V2 => 7,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CorpusKind {
    Mdx,
    Mdd,
}

impl CorpusKind {
    fn parse(value: &str) -> Result<Self, String> {
        match value {
            "mdx" => Ok(Self::Mdx),
            "mdd" => Ok(Self::Mdd),
            _ => Err(format!("kind must be `mdx` or `mdd`, got {value:?}")),
        }
    }

    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Mdx => "mdx",
            Self::Mdd => "mdd",
        }
    }
}

#[derive(Debug, Clone)]
pub struct CorpusEntry {
    relative_path: PathBuf,
    canonical_path: PathBuf,
    kind: CorpusKind,
    expected_bytes: u64,
    expected_sha256: [u8; 32],
    expected_entries: u64,
    expected_key_sha256: Option<[u8; 32]>,
    expected_payload_sha256: Option<[u8; 32]>,
}

impl CorpusEntry {
    pub fn relative_path(&self) -> &Path {
        &self.relative_path
    }

    pub fn manifest_path(&self) -> String {
        self.relative_path
            .components()
            .filter_map(|component| match component {
                Component::Normal(value) => Some(value.to_string_lossy()),
                _ => None,
            })
            .collect::<Vec<_>>()
            .join("/")
    }

    pub const fn kind(&self) -> CorpusKind {
        self.kind
    }

    pub const fn expected_bytes(&self) -> u64 {
        self.expected_bytes
    }

    pub const fn expected_entries(&self) -> u64 {
        self.expected_entries
    }

    pub fn expected_sha256_hex(&self) -> String {
        hex(&self.expected_sha256)
    }

    pub fn expected_key_sha256(&self) -> Option<&[u8; 32]> {
        self.expected_key_sha256.as_ref()
    }

    pub fn expected_payload_sha256(&self) -> Option<&[u8; 32]> {
        self.expected_payload_sha256.as_ref()
    }
}

#[derive(Debug)]
pub struct Corpus {
    root: PathBuf,
    entries: Vec<CorpusEntry>,
    manifest_sha256: [u8; 32],
}

impl Corpus {
    pub fn load_from_env() -> Result<Self, String> {
        let root = env::var_os(CORPUS_ENV).ok_or_else(setup_instructions)?;
        let root = PathBuf::from(root);
        Self::load(&root).map_err(|error| format!("{error}\n\n{}", setup_instructions()))
    }

    pub fn load_from_env_or_panic() -> Self {
        Self::load_from_env().unwrap_or_else(|error| panic!("{error}"))
    }

    pub fn load(root: &Path) -> Result<Self, String> {
        if !root.is_dir() {
            return Err(format!(
                "{CORPUS_ENV} points to {}, which is not a directory",
                root.display()
            ));
        }

        let canonical_root = root.canonicalize().map_err(|error| {
            format!(
                "failed to resolve corpus directory {}: {error}",
                root.display()
            )
        })?;
        let manifest_path = canonical_root.join(MANIFEST_NAME);
        let manifest = fs::read_to_string(&manifest_path).map_err(|error| {
            format!(
                "failed to read corpus manifest {}: {error}",
                manifest_path.display()
            )
        })?;
        let mut manifest_digest = Sha256::new();
        manifest_digest.update(manifest.as_bytes());
        let manifest_sha256 = manifest_digest.finish();

        let mut manifest_version = None;
        let mut entries = Vec::new();
        let mut paths = HashSet::new();

        for (index, raw_line) in manifest.lines().enumerate() {
            let line_number = index + 1;
            let line = raw_line.trim_end_matches('\r');
            if line.is_empty() || line.starts_with('#') {
                continue;
            }
            let Some(version) = manifest_version else {
                manifest_version = Some(match line {
                    MANIFEST_HEADER_V1 => ManifestVersion::V1,
                    MANIFEST_HEADER_V2 => ManifestVersion::V2,
                    _ => {
                        return Err(format!(
                            "{}:{line_number}: expected manifest header {MANIFEST_HEADER_V1:?} or {MANIFEST_HEADER_V2:?}",
                            manifest_path.display()
                        ));
                    }
                });
                continue;
            };

            let fields = line.split('\t').collect::<Vec<_>>();
            if fields.len() != version.field_count() {
                return Err(format!(
                    "{}:{line_number}: expected {} tab-separated fields, got {}",
                    manifest_path.display(),
                    version.field_count(),
                    fields.len()
                ));
            }

            let relative_path = parse_relative_path(fields[0])
                .map_err(|error| format!("{}:{line_number}: {error}", manifest_path.display()))?;
            if !paths.insert(relative_path.clone()) {
                return Err(format!(
                    "{}:{line_number}: duplicate path {}",
                    manifest_path.display(),
                    relative_path.display()
                ));
            }

            let kind = CorpusKind::parse(fields[1])
                .map_err(|error| format!("{}:{line_number}: {error}", manifest_path.display()))?;
            validate_extension(&relative_path, kind)
                .map_err(|error| format!("{}:{line_number}: {error}", manifest_path.display()))?;
            let expected_bytes = parse_u64(fields[2], "bytes")
                .map_err(|error| format!("{}:{line_number}: {error}", manifest_path.display()))?;
            let expected_sha256 = parse_sha256(fields[3], "sha256")
                .map_err(|error| format!("{}:{line_number}: {error}", manifest_path.display()))?;
            let expected_entries = parse_u64(fields[4], "entries")
                .map_err(|error| format!("{}:{line_number}: {error}", manifest_path.display()))?;
            let (expected_key_sha256, expected_payload_sha256) = match version {
                ManifestVersion::V1 => (None, None),
                ManifestVersion::V2 => (
                    parse_optional_sha256(fields[5], "key_sha256").map_err(|error| {
                        format!("{}:{line_number}: {error}", manifest_path.display())
                    })?,
                    parse_optional_sha256(fields[6], "payload_sha256").map_err(|error| {
                        format!("{}:{line_number}: {error}", manifest_path.display())
                    })?,
                ),
            };

            entries.push(CorpusEntry {
                relative_path,
                canonical_path: PathBuf::new(),
                kind,
                expected_bytes,
                expected_sha256,
                expected_entries,
                expected_key_sha256,
                expected_payload_sha256,
            });
        }

        if manifest_version.is_none() {
            return Err(format!(
                "corpus manifest {} has no header",
                manifest_path.display()
            ));
        }
        if entries.is_empty() {
            return Err(format!(
                "corpus manifest {} has no dictionary rows",
                manifest_path.display()
            ));
        }

        entries.sort_by(|left, right| left.relative_path.cmp(&right.relative_path));
        let mut corpus = Self {
            root: canonical_root,
            entries,
            manifest_sha256,
        };
        corpus.verify_files()?;
        Ok(corpus)
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    pub fn entries(&self) -> &[CorpusEntry] {
        &self.entries
    }

    pub const fn manifest_sha256(&self) -> &[u8; 32] {
        &self.manifest_sha256
    }

    pub fn entries_of_kind(&self, kind: CorpusKind) -> impl Iterator<Item = &CorpusEntry> + '_ {
        self.entries.iter().filter(move |entry| entry.kind == kind)
    }

    pub fn path(&self, entry: &CorpusEntry) -> PathBuf {
        entry.canonical_path.clone()
    }

    fn verify_files(&mut self) -> Result<(), String> {
        for entry in &mut self.entries {
            let path = self.root.join(&entry.relative_path);
            let canonical_path = path.canonicalize().map_err(|error| {
                format!(
                    "manifest entry {} is unavailable: {error}",
                    entry.relative_path.display()
                )
            })?;
            if !canonical_path.starts_with(&self.root) {
                return Err(format!(
                    "manifest entry {} resolves outside the corpus directory",
                    entry.relative_path.display()
                ));
            }

            let metadata = canonical_path.metadata().map_err(|error| {
                format!(
                    "failed to inspect manifest entry {}: {error}",
                    entry.relative_path.display()
                )
            })?;
            if !metadata.is_file() {
                return Err(format!(
                    "manifest entry {} is not a regular file",
                    entry.relative_path.display()
                ));
            }
            if metadata.len() != entry.expected_bytes {
                return Err(format!(
                    "manifest entry {} has {} bytes; expected {}",
                    entry.relative_path.display(),
                    metadata.len(),
                    entry.expected_bytes
                ));
            }

            let actual_sha256 = sha256_file(&canonical_path).map_err(|error| {
                format!(
                    "failed to hash manifest entry {}: {error}",
                    entry.relative_path.display()
                )
            })?;
            if actual_sha256 != entry.expected_sha256 {
                return Err(format!(
                    "manifest entry {} has SHA-256 {}; expected {}",
                    entry.relative_path.display(),
                    hex(&actual_sha256),
                    entry.expected_sha256_hex()
                ));
            }
            entry.canonical_path = canonical_path;
        }
        Ok(())
    }
}

pub fn setup_instructions() -> String {
    format!(
        "Set {CORPUS_ENV} to an authorized corpus directory containing {MANIFEST_NAME}. \
The manifest must use the tab-separated v1 header `{MANIFEST_HEADER_V1}` or \
v2 header `{MANIFEST_HEADER_V2}` and one \
row per .mdx/.mdd file. See tests/corpus-manifest.example.tsv."
    )
}

fn parse_relative_path(value: &str) -> Result<PathBuf, String> {
    if value.is_empty() {
        return Err("manifest path is empty".to_owned());
    }
    if value.contains('\\') {
        return Err(format!(
            "manifest path {value:?} must use `/` as its separator"
        ));
    }
    let path = PathBuf::from(value);
    if path
        .components()
        .any(|component| !matches!(component, Component::Normal(_)))
    {
        return Err(format!(
            "manifest path {value:?} must be a normalized relative path"
        ));
    }
    let normalized = path
        .components()
        .filter_map(|component| match component {
            Component::Normal(value) => Some(value.to_string_lossy()),
            _ => None,
        })
        .collect::<Vec<_>>()
        .join("/");
    if normalized != value {
        return Err(format!(
            "manifest path {value:?} is not normalized; use {normalized:?}"
        ));
    }
    Ok(path)
}

fn validate_extension(path: &Path, kind: CorpusKind) -> Result<(), String> {
    let extension = path.extension().and_then(OsStr::to_str);
    if extension.is_some_and(|value| value.eq_ignore_ascii_case(kind.as_str())) {
        Ok(())
    } else {
        Err(format!(
            "path {} does not have the .{} extension declared by its kind",
            path.display(),
            kind.as_str()
        ))
    }
}

fn parse_u64(value: &str, field: &str) -> Result<u64, String> {
    value
        .parse::<u64>()
        .map_err(|_| format!("{field} must be an unsigned decimal integer, got {value:?}"))
}

fn parse_optional_sha256(value: &str, field: &str) -> Result<Option<[u8; 32]>, String> {
    if value.is_empty() {
        Ok(None)
    } else {
        parse_sha256(value, field).map(Some)
    }
}

fn parse_sha256(value: &str, field: &str) -> Result<[u8; 32], String> {
    if value.len() != 64 || !value.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return Err(format!(
            "{field} must contain exactly 64 hexadecimal digits, got {value:?}"
        ));
    }

    let mut output = [0u8; 32];
    for (index, destination) in output.iter_mut().enumerate() {
        let offset = index * 2;
        *destination = (hex_nibble(value.as_bytes()[offset])? << 4)
            | hex_nibble(value.as_bytes()[offset + 1])?;
    }
    Ok(output)
}

fn hex_nibble(byte: u8) -> Result<u8, String> {
    match byte {
        b'0'..=b'9' => Ok(byte - b'0'),
        b'a'..=b'f' => Ok(byte - b'a' + 10),
        b'A'..=b'F' => Ok(byte - b'A' + 10),
        _ => Err("invalid hexadecimal digit".to_owned()),
    }
}

pub fn sha256_file(path: &Path) -> io::Result<[u8; 32]> {
    let mut file = File::open(path)?;
    let mut digest = Sha256::new();
    let mut buffer = [0u8; 64 * 1024];
    loop {
        let read = file.read(&mut buffer)?;
        if read == 0 {
            break;
        }
        digest.update(&buffer[..read]);
    }
    Ok(digest.finish())
}

pub fn hex(bytes: &[u8]) -> String {
    const DIGITS: &[u8; 16] = b"0123456789abcdef";
    let mut output = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        output.push(char::from(DIGITS[usize::from(byte >> 4)]));
        output.push(char::from(DIGITS[usize::from(byte & 0x0f)]));
    }
    output
}

pub struct Sha256 {
    state: [u32; 8],
    buffer: [u8; 64],
    buffer_len: usize,
    bit_len: u64,
}

impl Sha256 {
    pub const fn new() -> Self {
        Self {
            state: [
                0x6a09_e667,
                0xbb67_ae85,
                0x3c6e_f372,
                0xa54f_f53a,
                0x510e_527f,
                0x9b05_688c,
                0x1f83_d9ab,
                0x5be0_cd19,
            ],
            buffer: [0; 64],
            buffer_len: 0,
            bit_len: 0,
        }
    }

    pub fn update(&mut self, mut bytes: &[u8]) {
        self.bit_len = self
            .bit_len
            .wrapping_add((bytes.len() as u64).wrapping_mul(8));

        if self.buffer_len != 0 {
            let needed = 64 - self.buffer_len;
            let copied = needed.min(bytes.len());
            self.buffer[self.buffer_len..self.buffer_len + copied]
                .copy_from_slice(&bytes[..copied]);
            self.buffer_len += copied;
            bytes = &bytes[copied..];
            if self.buffer_len == 64 {
                self.compress_buffer();
                self.buffer_len = 0;
            } else {
                return;
            }
        }

        while bytes.len() >= 64 {
            let mut block = [0u8; 64];
            block.copy_from_slice(&bytes[..64]);
            compress_sha256(&mut self.state, &block);
            bytes = &bytes[64..];
        }

        self.buffer[..bytes.len()].copy_from_slice(bytes);
        self.buffer_len = bytes.len();
    }

    pub fn finish(mut self) -> [u8; 32] {
        self.buffer[self.buffer_len] = 0x80;
        self.buffer_len += 1;

        if self.buffer_len > 56 {
            self.buffer[self.buffer_len..].fill(0);
            self.compress_buffer();
            self.buffer_len = 0;
        }

        self.buffer[self.buffer_len..56].fill(0);
        self.buffer[56..64].copy_from_slice(&self.bit_len.to_be_bytes());
        self.compress_buffer();

        let mut output = [0u8; 32];
        for (chunk, value) in output.chunks_exact_mut(4).zip(self.state) {
            chunk.copy_from_slice(&value.to_be_bytes());
        }
        output
    }

    fn compress_buffer(&mut self) {
        let block = self.buffer;
        compress_sha256(&mut self.state, &block);
    }
}

fn compress_sha256(state: &mut [u32; 8], block: &[u8; 64]) {
    const K: [u32; 64] = [
        0x428a_2f98,
        0x7137_4491,
        0xb5c0_fbcf,
        0xe9b5_dba5,
        0x3956_c25b,
        0x59f1_11f1,
        0x923f_82a4,
        0xab1c_5ed5,
        0xd807_aa98,
        0x1283_5b01,
        0x2431_85be,
        0x550c_7dc3,
        0x72be_5d74,
        0x80de_b1fe,
        0x9bdc_06a7,
        0xc19b_f174,
        0xe49b_69c1,
        0xefbe_4786,
        0x0fc1_9dc6,
        0x240c_a1cc,
        0x2de9_2c6f,
        0x4a74_84aa,
        0x5cb0_a9dc,
        0x76f9_88da,
        0x983e_5152,
        0xa831_c66d,
        0xb003_27c8,
        0xbf59_7fc7,
        0xc6e0_0bf3,
        0xd5a7_9147,
        0x06ca_6351,
        0x1429_2967,
        0x27b7_0a85,
        0x2e1b_2138,
        0x4d2c_6dfc,
        0x5338_0d13,
        0x650a_7354,
        0x766a_0abb,
        0x81c2_c92e,
        0x9272_2c85,
        0xa2bf_e8a1,
        0xa81a_664b,
        0xc24b_8b70,
        0xc76c_51a3,
        0xd192_e819,
        0xd699_0624,
        0xf40e_3585,
        0x106a_a070,
        0x19a4_c116,
        0x1e37_6c08,
        0x2748_774c,
        0x34b0_bcb5,
        0x391c_0cb3,
        0x4ed8_aa4a,
        0x5b9c_ca4f,
        0x682e_6ff3,
        0x748f_82ee,
        0x78a5_636f,
        0x84c8_7814,
        0x8cc7_0208,
        0x90be_fffa,
        0xa450_6ceb,
        0xbef9_a3f7,
        0xc671_78f2,
    ];

    let mut words = [0u32; 64];
    for (index, word) in words[..16].iter_mut().enumerate() {
        let offset = index * 4;
        *word = u32::from_be_bytes([
            block[offset],
            block[offset + 1],
            block[offset + 2],
            block[offset + 3],
        ]);
    }
    for index in 16..64 {
        let s0 = words[index - 15].rotate_right(7)
            ^ words[index - 15].rotate_right(18)
            ^ (words[index - 15] >> 3);
        let s1 = words[index - 2].rotate_right(17)
            ^ words[index - 2].rotate_right(19)
            ^ (words[index - 2] >> 10);
        words[index] = words[index - 16]
            .wrapping_add(s0)
            .wrapping_add(words[index - 7])
            .wrapping_add(s1);
    }

    let [mut a, mut b, mut c, mut d, mut e, mut f, mut g, mut h] = *state;
    for index in 0..64 {
        let sum1 = e.rotate_right(6) ^ e.rotate_right(11) ^ e.rotate_right(25);
        let choice = (e & f) ^ ((!e) & g);
        let temporary1 = h
            .wrapping_add(sum1)
            .wrapping_add(choice)
            .wrapping_add(K[index])
            .wrapping_add(words[index]);
        let sum0 = a.rotate_right(2) ^ a.rotate_right(13) ^ a.rotate_right(22);
        let majority = (a & b) ^ (a & c) ^ (b & c);
        let temporary2 = sum0.wrapping_add(majority);

        h = g;
        g = f;
        f = e;
        e = d.wrapping_add(temporary1);
        d = c;
        c = b;
        b = a;
        a = temporary1.wrapping_add(temporary2);
    }

    state[0] = state[0].wrapping_add(a);
    state[1] = state[1].wrapping_add(b);
    state[2] = state[2].wrapping_add(c);
    state[3] = state[3].wrapping_add(d);
    state[4] = state[4].wrapping_add(e);
    state[5] = state[5].wrapping_add(f);
    state[6] = state[6].wrapping_add(g);
    state[7] = state[7].wrapping_add(h);
}
