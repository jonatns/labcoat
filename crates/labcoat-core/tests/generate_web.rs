//! Golden-file test for `labcoat generate web`: fixture lockfile + ABI
//! artifacts in, byte-exact TypeScript out. No network access.
//!
//! To bless new expected output after an intentional codegen change:
//!
//! ```sh
//! LABCOAT_BLESS=1 cargo test -p labcoat-core --test generate_web
//! ```
//!
//! The live counterpart (`labcoat generate web` inside a project with a
//! seeded Labcoat Network, then type-checking and reading through the
//! artifacts from the web app) is a manual check documented in
//! docs/GENERATE-WEB.md.

use labcoat_core::generate::{generate_web, load_build_abis, WebInputs};
use std::path::{Path, PathBuf};

fn fixture_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/generate-web")
}

#[test]
fn generates_the_expected_typescript_tree() {
    let root = fixture_root();
    let abis = load_build_abis(&root.join("project/build")).unwrap();
    assert_eq!(
        abis.iter().map(|a| a.artifact.as_str()).collect::<Vec<_>>(),
        vec!["overwrite-series", "overwrite-test-token"],
    );

    let lockfile = labcoat_core::lockfile::load(&root.join("project")).unwrap();
    let files = generate_web(&WebInputs {
        network: "labcoat",
        bitcoin_network: "regtest",
        rpc_url: "http://127.0.0.1:18443",
        lockfile: &lockfile,
        abis: &abis,
    })
    .unwrap();

    let expected_root = root.join("expected");
    if std::env::var_os("LABCOAT_BLESS").is_some() {
        let _ = std::fs::remove_dir_all(&expected_root);
        for file in &files {
            let path = expected_root.join(&file.path);
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(&path, &file.contents).unwrap();
        }
        return;
    }

    let mut expected_paths = Vec::new();
    for dir in [expected_root.clone(), expected_root.join("abi")] {
        for entry in std::fs::read_dir(&dir)
            .unwrap_or_else(|_| panic!("missing golden files — run with LABCOAT_BLESS=1 first"))
        {
            let path = entry.unwrap().path();
            if path.extension().and_then(|e| e.to_str()) == Some("ts") {
                expected_paths.push(
                    path.strip_prefix(&expected_root)
                        .unwrap()
                        .to_string_lossy()
                        .into_owned(),
                );
            }
        }
    }
    expected_paths.sort();
    let mut generated_paths: Vec<String> = files.iter().map(|f| f.path.clone()).collect();
    generated_paths.sort();
    assert_eq!(
        generated_paths, expected_paths,
        "generated file set differs from the golden tree",
    );

    for file in &files {
        let expected = std::fs::read_to_string(expected_root.join(&file.path))
            .unwrap_or_else(|_| panic!("missing golden file {}", file.path));
        assert_eq!(
            file.contents, expected,
            "{} differs from its golden file — bless with LABCOAT_BLESS=1 if intentional",
            file.path,
        );
    }
}
