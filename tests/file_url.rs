//! Opening a dictionary the way a mobile file picker names it.
//!
//! iOS answers its document picker with `NSURL`s and Android's Storage Access
//! Framework does the same, so an application that passes the picker's answer
//! through arrives with `file:///…` where a path is expected. These are the
//! public entry points, checked against real files rather than against the
//! decoder alone.

mod support;

use std::path::PathBuf;

use mdictlib::{Error, MddFile, MdxFile};

use support::{FixtureBuilder, TempDictionary};

/// The same file, addressed as a picker would address it.
fn file_url(dictionary: &TempDictionary) -> PathBuf {
    let path = dictionary.path().to_str().expect("a UTF-8 temporary path");
    PathBuf::from(format!("file://{path}"))
}

#[test]
fn an_mdx_opens_from_a_file_url() {
    let dictionary = FixtureBuilder::mdx([("alpha", "record")])
        .build()
        .write("file-url-mdx");

    let from_path = MdxFile::open(dictionary.path()).unwrap();
    let from_url = MdxFile::open(file_url(&dictionary)).unwrap();

    assert_eq!(from_url.len(), from_path.len());
    assert_eq!(
        from_url
            .lookup("alpha")
            .unwrap()
            .map(|entry| entry.text().to_string()),
        from_path
            .lookup("alpha")
            .unwrap()
            .map(|entry| entry.text().to_string()),
    );
}

#[test]
fn an_mdd_opens_from_a_file_url() {
    let resources = FixtureBuilder::mdd([("\\alpha.png", b"bytes".to_vec())])
        .build()
        .write("file-url-mdd");

    let from_url = MddFile::open(file_url(&resources)).unwrap();

    assert_eq!(
        from_url.len(),
        MddFile::open(resources.path()).unwrap().len()
    );
}

#[test]
fn a_percent_escaped_name_opens_the_file_it_names() {
    let dictionary = FixtureBuilder::mdx([("alpha", "record")])
        .build()
        .write("file url with spaces");
    let path = dictionary.path().to_str().expect("a UTF-8 temporary path");
    let escaped = PathBuf::from(format!("file://{}", path.replace(' ', "%20")));

    assert!(
        path.contains(' '),
        "the fixture name carries the spaces under test"
    );
    assert_eq!(
        MdxFile::open(escaped).unwrap().len(),
        MdxFile::open(dictionary.path()).unwrap().len(),
    );
}

#[test]
fn a_remote_file_url_is_refused_before_anything_is_read() {
    let error = MdxFile::open("file://dictionaries.example.com/oald.mdx").unwrap_err();

    // Not an I/O error: nothing was opened, because nothing local was named.
    assert!(matches!(error, Error::InvalidData(_)), "{error:?}");
}
