//! Executable architecture contract for wire-version isolation.
//!
//! These rules are the reason `mdictlib` can support more than one MDict
//! version without a version conditional threaded through lookup, iteration,
//! ordinal access, record decoding, and MDD streaming. A comment cannot hold
//! that line as the crate grows, so the boundary is asserted against the
//! source text itself.

use std::fs;
use std::path::{Path, PathBuf};

fn crate_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

/// Reads a source file, with its path preserved for failure messages.
fn read(relative: &str) -> String {
    let path = crate_root().join(relative);
    fs::read_to_string(&path).unwrap_or_else(|error| panic!("cannot read {relative}: {error}"))
}

/// Collects every `.rs` file under a directory, recursively.
fn rust_files_under(relative: &str) -> Vec<String> {
    fn walk(root: &Path, base: &Path, out: &mut Vec<String>) {
        let entries = fs::read_dir(root).unwrap_or_else(|error| {
            panic!("cannot list {}: {error}", root.display());
        });
        for entry in entries {
            let path = entry.expect("directory entry is readable").path();
            if path.is_dir() {
                walk(&path, base, out);
            } else if path.extension().is_some_and(|ext| ext == "rs") {
                let relative = path
                    .strip_prefix(base)
                    .expect("walked path is inside the crate root");
                out.push(relative.to_string_lossy().replace('\\', "/"));
            }
        }
    }

    let base = crate_root();
    let mut files = Vec::new();
    walk(&base.join(relative), &base, &mut files);
    files.sort();
    assert!(!files.is_empty(), "expected Rust sources under {relative}");
    files
}

/// Strips `//`-style comments so a doc comment mentioning a forbidden name
/// does not fail the contract. Block comments are not used for prose here.
fn code_only(source: &str) -> String {
    source
        .lines()
        .map(|line| match line.find("//") {
            Some(index) => &line[..index],
            None => line,
        })
        .collect::<Vec<_>>()
        .join("\n")
}

#[test]
fn the_core_and_facades_cannot_name_a_wire_version() {
    // The whole point of ValidatedLayout: everything downstream of the format
    // facade is version-blind. If one of these names appears here, a version
    // branch has leaked back into shared code.
    let forbidden = [
        "WireVersion",
        "is_v1",
        "is_v2",
        "format::v1",
        "format::v2",
        "::v1::",
        "::v2::",
    ];

    let mut targets = rust_files_under("src/core");
    targets.push("src/mdx.rs".to_owned());
    targets.push("src/mdd.rs".to_owned());
    targets.push("src/lookup.rs".to_owned());
    targets.push("src/types.rs".to_owned());

    for file in targets {
        let source = code_only(&read(&file));
        for name in forbidden {
            assert!(
                !source.contains(name),
                "{file} names `{name}`; version dispatch must stay inside src/format/mod.rs"
            );
        }
    }
}

#[test]
fn grammar_modules_cannot_see_the_core_or_each_other() {
    for (module, sibling) in [("src/format/v1", "v2"), ("src/format/v2", "v1")] {
        for file in rust_files_under(module) {
            let source = code_only(&read(&file));
            for name in ["crate::core", "crate::mdx", "crate::mdd", "crate::lookup"] {
                assert!(
                    !source.contains(name),
                    "{file} reaches {name}; grammars may only depend on format::common"
                );
            }
            for name in [
                format!("crate::format::{sibling}"),
                format!("super::{sibling}"),
            ] {
                assert!(
                    !source.contains(&name),
                    "{file} reaches {name}; the two grammars must stay independent"
                );
            }
        }
    }
}

#[test]
fn only_the_format_facade_matches_on_the_wire_version() {
    let facade = code_only(&read("src/format/mod.rs"));
    assert!(
        facade.contains("enum WireVersion"),
        "src/format/mod.rs must define WireVersion"
    );
    assert_eq!(
        facade.matches("match version").count(),
        1,
        "src/format/mod.rs must contain exactly one version match"
    );

    let mut others = rust_files_under("src/format/common");
    others.extend(rust_files_under("src/format/v1"));
    others.extend(rust_files_under("src/format/v2"));
    for file in others {
        let source = code_only(&read(&file));
        assert!(
            !source.contains("WireVersion"),
            "{file} names WireVersion; only the facade may resolve a version"
        );
    }
}

#[test]
fn shared_code_uses_no_trait_object_dispatch() {
    // Wire operations are concrete function pointers selected once at open.
    // A `dyn` on this path would add a vtable indirection per lazy block and
    // reintroduce exactly the dynamic version dispatch this design avoids.
    let mut targets = rust_files_under("src/core");
    targets.extend(rust_files_under("src/format"));
    for file in targets {
        let source = code_only(&read(&file));
        assert!(
            !source.contains("dyn "),
            "{file} uses trait-object dispatch on the parsing path"
        );
    }
}

#[test]
fn no_runtime_conversion_between_wire_versions() {
    // A converter would let a v1 bug hide behind the v2 grammar and would
    // silently double the memory cost of opening a file.
    for file in rust_files_under("src/format") {
        let source = code_only(&read(&file));
        for marker in ["to_v2", "into_v2", "as_v2", "to_v1", "into_v1", "as_v1"] {
            assert!(
                !source.contains(marker),
                "{file} looks like it converts between wire versions"
            );
        }
    }
}

#[test]
fn grammars_reach_the_core_only_through_the_validated_layout() {
    for module in ["src/format/v1", "src/format/v2"] {
        let produces_layout = rust_files_under(module)
            .iter()
            .any(|file| read(file).contains("ValidatedLayout"));
        assert!(
            produces_layout,
            "{module} must produce a ValidatedLayout as its only output"
        );
    }
}
