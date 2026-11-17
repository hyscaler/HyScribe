// SPDX-License-Identifier: AGPL-3.0-or-later

use hyscribe_testkit::unhex;

/// A file of the upload corpus (`reference/uploads/corpus.sh`).
pub(crate) fn corpus(name: &str) -> Vec<u8> {
    let path = format!("{}/tests/corpus/{name}", env!("CARGO_MANIFEST_DIR"));
    std::fs::read(&path).unwrap_or_else(|err| panic!("{path}: {err}"))
}

/// A hostile case (`reference/uploads/hostile.py`), also a fuzz seed.
pub(crate) fn hostile(name: &str) -> Vec<u8> {
    let path = format!("{}/tests/hostile/{name}", env!("CARGO_MANIFEST_DIR"));
    std::fs::read(&path).unwrap_or_else(|err| panic!("{path}: {err}"))
}

/// The `kind` lines of `uploads.tsv` (`reference/uploads/golden.sh`): each
/// corpus file and PHP's serialized answer.
pub(crate) fn golden(kind: &str) -> Vec<(String, Vec<u8>)> {
    include_str!("../tests/golden/uploads.tsv")
        .lines()
        .filter_map(|line| {
            let mut fields = line.split('\t');
            (fields.next() == Some(kind)).then(|| {
                (
                    fields.next().unwrap().to_string(),
                    unhex(fields.next().unwrap()),
                )
            })
        })
        .collect()
}
