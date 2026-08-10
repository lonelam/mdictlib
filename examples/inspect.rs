use std::env;

use mdictlib::{MddFile, MdxFile};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut args = env::args().skip(1);
    let mode = args
        .next()
        .ok_or("usage: inspect <mdx|mdd> <path> [lookup_key|--count-only]")?;
    let path = args
        .next()
        .ok_or("usage: inspect <mdx|mdd> <path> [lookup_key|--count-only]")?;
    let lookup_key = args.next();
    if args.next().is_some() {
        return Err("usage: inspect <mdx|mdd> <path> [lookup_key|--count-only]".into());
    }
    let count_only = lookup_key.as_deref() == Some("--count-only");

    match mode.as_str() {
        "mdx" => {
            let file = MdxFile::open(&path)?;
            println!("entries={}", file.len());
            if count_only {
                return Ok(());
            }
            println!("title={:?}", file.header().title());
            println!(
                "description_present={}",
                file.header().description().is_some()
            );
            if let Some(key) = lookup_key {
                match file.lookup(&key)? {
                    Some(record) => {
                        println!("ordinal={}", record.ordinal().get());
                        println!("key={}", record.key());
                        println!("text_prefix={}", prefix(record.text(), 200));
                    }
                    None => println!("not found"),
                }
            } else {
                for entry in file.entries().take(5) {
                    let entry = entry?;
                    println!(
                        "{} {} => {}",
                        entry.ordinal().get(),
                        entry.key(),
                        prefix(entry.text(), 80)
                    );
                }
            }
        }
        "mdd" => {
            let file = MddFile::open(&path)?;
            println!("entries={}", file.len());
            if count_only {
                return Ok(());
            }
            println!("title={:?}", file.header().title());
            if let Some(key) = lookup_key {
                match file.lookup(&key)? {
                    Some(resource) => {
                        println!("ordinal={}", resource.ordinal().get());
                        println!("key={}", resource.key());
                        println!("bytes={}", resource.bytes().len());
                        println!(
                            "prefix={:02x?}",
                            &resource.bytes()[..resource.bytes().len().min(16)]
                        );
                    }
                    None => println!("not found"),
                }
            } else {
                for resource in file.resources().take(5) {
                    let resource = resource?;
                    println!(
                        "{} {} => {} bytes",
                        resource.ordinal().get(),
                        resource.key(),
                        resource.bytes().len()
                    );
                }
            }
        }
        _ => return Err("mode must be mdx or mdd".into()),
    }

    Ok(())
}

fn prefix(text: &str, max_chars: usize) -> String {
    text.chars().take(max_chars).collect()
}
