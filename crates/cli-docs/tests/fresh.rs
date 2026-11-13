// SPDX-FileCopyrightText: 2026 Sway Things contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The committed man pages and completions in `data/` are exactly what the generator writes.

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

use cli_docs::{COMPLETIONS_DIR, MAN_DIR, files};

#[test]
fn committed_files_are_up_to_date() {
    let data = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../data");
    let generated: BTreeMap<PathBuf, Vec<u8>> = files().unwrap().into_iter().collect();
    let mut committed = BTreeMap::new();
    for directory in [MAN_DIR, COMPLETIONS_DIR] {
        for entry in fs::read_dir(data.join(directory)).unwrap() {
            let path = entry.unwrap().path();
            let relative = path.strip_prefix(&data).unwrap().to_owned();
            committed.insert(relative, fs::read(&path).unwrap());
        }
    }

    let mut problems = Vec::new();
    for (path, content) in &generated {
        match committed.get(path) {
            None => problems.push(format!("missing: data/{}", path.display())),
            Some(on_disk) if on_disk != content => {
                problems.push(format!("out of date: data/{}", path.display()));
            }
            Some(_) => {}
        }
    }
    for path in committed
        .keys()
        .filter(|path| !generated.contains_key(*path))
    {
        problems.push(format!("not generated: data/{}", path.display()));
    }
    assert!(
        problems.is_empty(),
        "{}\nrun scripts/update-cli-docs.sh",
        problems.join("\n")
    );
}
