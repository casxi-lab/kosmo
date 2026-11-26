//! The Scratchpad keeps its state only in memory: submitting, delivering,
//! evicting and reusing leave the files of the working directory as they
//! were.
//!
//! This file has one test, so no other test of its binary runs beside it.

mod support;

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

use support::{Checked, cases};

/// Every file under `dir`, recursively, with its length and modification
/// time; symbolic links are listed, not followed.
fn files(dir: &Path) -> Checked<BTreeMap<PathBuf, (u64, Option<SystemTime>)>> {
    let mut found = BTreeMap::new();
    let mut pending = vec![dir.to_path_buf()];
    while let Some(next) = pending.pop() {
        for item in fs::read_dir(&next)? {
            let item = item?;
            let kind = item.file_type()?;
            if kind.is_dir() {
                pending.push(item.path());
            } else {
                let metadata = fs::symlink_metadata(item.path())?;
                found.insert(item.path(), (metadata.len(), metadata.modified().ok()));
            }
        }
    }
    Ok(found)
}

#[test]
fn the_scratchpad_writes_no_file() -> Checked {
    // Cargo runs integration tests in the package directory.
    let dir = std::env::current_dir()?;
    let before = files(&dir)?;
    assert!(before.contains_key(&dir.join("Cargo.toml")));

    cases::late_result()?;
    cases::eviction()?;
    cases::whole_input()?;
    cases::part_of_a_result()?;
    cases::fill_one_over_a_hole()?;
    cases::fill_two_times_a_hole()?;

    assert_eq!(files(&dir)?, before);
    Ok(())
}
