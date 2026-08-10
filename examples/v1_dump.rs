//! Dumps one dictionary's observable contract for differential comparison.
//!
//! Prints one line per physical entry: `<sha256(payload)>\t<key>`. Keys and
//! payloads are the only things two independent readers can be expected to
//! agree on — record offsets and block structure are internal to each reader —
//! so this is deliberately all that is emitted.
//!
//! Payload digests rather than payloads keep the output bounded for large
//! dictionaries while still detecting any byte-level disagreement.

#[path = "support/sha256.rs"]
mod sha256;

use std::env;
use std::io::{self, BufWriter, Write};
use std::path::PathBuf;
use std::process::ExitCode;

use mdictlib::{KeyOrdinal, MddFile, MdxFile};

use sha256::{Sha256, hex};

fn main() -> ExitCode {
    let mut arguments = env::args_os();
    let _program = arguments.next();
    let Some(path) = arguments.next().map(PathBuf::from) else {
        eprintln!("usage: v1_dump <path>");
        return ExitCode::FAILURE;
    };
    if arguments.next().is_some() {
        eprintln!("v1_dump: unexpected extra argument");
        return ExitCode::FAILURE;
    }

    let is_mdd = path
        .extension()
        .is_some_and(|extension| extension.eq_ignore_ascii_case("mdd"));

    let stdout = io::stdout();
    let mut output = BufWriter::new(stdout.lock());

    let result = if is_mdd {
        dump_mdd(&path, &mut output)
    } else {
        dump_mdx(&path, &mut output)
    };

    match result.and_then(|()| output.flush().map_err(|error| error.to_string())) {
        Ok(()) => ExitCode::SUCCESS,
        Err(message) => {
            eprintln!("v1_dump: {message}");
            ExitCode::FAILURE
        }
    }
}

fn dump_mdx(path: &std::path::Path, output: &mut impl Write) -> Result<(), String> {
    let dictionary = MdxFile::open(path).map_err(|error| format!("open failed: {error}"))?;
    for result in dictionary.entries() {
        let entry = result.map_err(|error| format!("entry failed: {error}"))?;
        let mut hash = Sha256::new();
        hash.update(entry.text().as_bytes());
        writeln!(output, "{}\t{}", hex(&hash.finish()), sanitize(entry.key()))
            .map_err(|error| error.to_string())?;
    }
    Ok(())
}

fn dump_mdd(path: &std::path::Path, output: &mut impl Write) -> Result<(), String> {
    let dictionary = MddFile::open(path).map_err(|error| format!("open failed: {error}"))?;
    for index in 0..dictionary.len() {
        let span = dictionary
            .span_at(KeyOrdinal::new(index))
            .map_err(|error| format!("span failed: {error}"))?
            .ok_or_else(|| format!("ordinal {index} had no span"))?;
        let resource = span
            .read()
            .map_err(|error| format!("resource failed: {error}"))?;
        let mut hash = Sha256::new();
        hash.update(resource.bytes());
        writeln!(output, "{}\t{}", hex(&hash.finish()), sanitize(span.key()))
            .map_err(|error| error.to_string())?;
    }
    Ok(())
}

/// Escapes the characters that would break the line-oriented protocol.
///
/// Real dictionary keys do contain tabs and newlines, so this must be
/// lossless: replacing them would make two readers look like they disagree
/// when they had produced identical keys.
fn sanitize(key: &str) -> String {
    let mut output = String::with_capacity(key.len());
    for character in key.chars() {
        match character {
            '\\' => output.push_str("\\\\"),
            '\t' => output.push_str("\\t"),
            '\n' => output.push_str("\\n"),
            '\r' => output.push_str("\\r"),
            other => output.push(other),
        }
    }
    output
}
