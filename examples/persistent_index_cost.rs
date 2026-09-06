use std::env;
use std::fs::OpenOptions as FsOpenOptions;
use std::io::Write;
use std::path::PathBuf;
use std::time::Instant;

use mdictlib::{KeyIndexOptions, Limits, MdxFile, OpenOptions};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut args = env::args_os().skip(1);
    let source = PathBuf::from(
        args.next()
            .ok_or("usage: persistent_index_cost <source.mdx> <output.aaidx>")?,
    );
    let output = PathBuf::from(
        args.next()
            .ok_or("usage: persistent_index_cost <source.mdx> <output.aaidx>")?,
    );
    let mode = args.next().unwrap_or_else(|| "path".into());
    if args.next().is_some() || (mode != "path" && mode != "writer" && mode != "memory") {
        return Err(
            "usage: persistent_index_cost <source.mdx> <output.aaidx> [path|writer|memory]".into(),
        );
    }

    let opened_at = Instant::now();
    let open_options = OpenOptions::new().with_limits(Limits::large_dictionary());
    let dictionary = MdxFile::open_with_options(&source, &open_options)?;
    eprintln!(
        "open_ms={} rows={}",
        opened_at.elapsed().as_millis(),
        dictionary.len()
    );

    if mode == "memory" {
        let started = Instant::now();
        let _ = dictionary.locate("__mdictlib_benchmark_absent_key__")?;
        let usage = dictionary.memory_usage()?;
        eprintln!(
            "locator_ms={} locator_bytes={} current_bytes={} peak_bytes={}",
            started.elapsed().as_millis(),
            usage.locator_bytes(),
            usage.current_bytes(),
            usage.peak_bytes()
        );
        return Ok(());
    }

    let options = KeyIndexOptions::new().with_scratch_directory(
        output
            .parent()
            .filter(|path| !path.as_os_str().is_empty())
            .unwrap_or_else(|| std::path::Path::new(".")),
    );
    let started = Instant::now();
    let report = if mode == "writer" {
        let mut destination = FsOpenOptions::new()
            .create_new(true)
            .write(true)
            .open(&output)?;
        let report = dictionary.build_key_index(&mut destination, &options, || false)?;
        destination.flush()?;
        destination.sync_all()?;
        report
    } else {
        dictionary.build_key_index_to_path(&output, &options, || false)?
    };
    eprintln!(
        "build_ms={} index_bytes={}",
        started.elapsed().as_millis(),
        report.bytes_written()
    );
    Ok(())
}
