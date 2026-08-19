//! Golden-file test for `labcoat state migrate`: fixture v1 labcoat.lock
//! in, byte-exact version-2 state out, and a byte-identical regenerated
//! address book — including the untouched foreign-network subtree. No
//! network access.
//!
//! To bless new expected output after an intentional schema change (this
//! also re-normalizes the fixture lockfile through `lockfile::save`, the
//! round-trip precondition):
//!
//! ```sh
//! LABCOAT_BLESS=1 cargo test -p labcoat-core --test state_migrate
//! ```

use labcoat_core::state::{self, ChainIdentity, MigrationInputs};
use labcoat_core::{lockfile, state_backend};
use std::path::{Path, PathBuf};

const LINEAGE: &str = "00000000-0000-4000-8000-000000000000";
const NOW_MILLIS: u64 = 1_755_000_000_000;

fn fixture_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/state-migrate")
}

fn chain() -> ChainIdentity {
    ChainIdentity {
        network: "labcoat".to_string(),
        bitcoin_network: "regtest".to_string(),
        block1_hash: Some("11".repeat(32)),
        labcoat_network_instance_id: Some("22222222-2222-4222-8222-222222222222".to_string()),
    }
}

#[test]
fn migration_matches_the_golden_state_and_round_trips_the_lockfile() {
    let root = fixture_root();
    let project = root.join("project");
    let lock = lockfile::load(&project).unwrap();

    let state = state::migrate_v1(&MigrationInputs {
        lockfile: &lock,
        network: "labcoat",
        environment: "default",
        chain: chain(),
        lineage: LINEAGE.to_string(),
    });
    let text = state::to_json_string(&state);

    let expected_path = root.join("expected/state.json");
    if std::env::var_os("LABCOAT_BLESS").is_some() {
        // Normalize the fixture through the real writer so the byte-level
        // round-trip guarantee below is exercised on a labcoat-written file.
        lockfile::save(&project, &lock).unwrap();
        std::fs::create_dir_all(expected_path.parent().unwrap()).unwrap();
        std::fs::write(&expected_path, &text).unwrap();
        return;
    }

    let expected = std::fs::read_to_string(&expected_path)
        .unwrap_or_else(|_| panic!("missing golden file — run with LABCOAT_BLESS=1 first"));
    assert_eq!(
        text, expected,
        "migrated state differs from its golden file — bless with LABCOAT_BLESS=1 if intentional",
    );

    // Every v1 record of the selected network became one imported instance;
    // nothing else was invented.
    assert_eq!(state.resources.len(), 3);
    assert_eq!(
        state
            .resources
            .values()
            .map(|r| r.instances.len())
            .sum::<usize>(),
        3
    );

    // Drive the real orchestration on a scratch copy of the project and
    // prove the address book survives byte-identically — the foreign
    // signet subtree included — with the original bytes in the backup.
    let scratch = std::env::temp_dir().join(format!(
        "labcoat-state-migrate-golden-{}",
        std::process::id()
    ));
    std::fs::remove_dir_all(&scratch).ok();
    std::fs::create_dir_all(&scratch).unwrap();
    std::fs::copy(
        project.join(lockfile::LOCKFILE),
        scratch.join(lockfile::LOCKFILE),
    )
    .unwrap();
    let original = std::fs::read(scratch.join(lockfile::LOCKFILE)).unwrap();

    let outcome = state::migrate(
        &scratch,
        "default",
        "labcoat",
        chain(),
        LINEAGE.to_string(),
        NOW_MILLIS,
    )
    .unwrap();
    assert_eq!(outcome.resources, 3);
    assert_eq!(outcome.instances, 3);
    assert_eq!(outcome.serial, 1);
    assert!(outcome.lockfile_regenerated);

    assert_eq!(
        std::fs::read(scratch.join(lockfile::LOCKFILE)).unwrap(),
        original,
        "the regenerated labcoat.lock must be byte-identical",
    );
    assert_eq!(std::fs::read(outcome.backup.unwrap()).unwrap(), original);

    // The persisted state is the golden state after the commit's single
    // serial increment.
    let persisted =
        std::fs::read_to_string(state_backend::state_path(&scratch, "default").unwrap()).unwrap();
    assert_eq!(
        persisted,
        expected.replace("\"serial\": 0", "\"serial\": 1")
    );

    std::fs::remove_dir_all(scratch).ok();
}
