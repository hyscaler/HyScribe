// SPDX-License-Identifier: AGPL-3.0-or-later

//! HyScribe's own IPTC reader: what PHP's `iptcparse()` gives for a JPEG's
//! `APP13` segment, written from the IPTC IIM specification and matched to
//! PHP by `reference/uploads/iptc-golden.php`.

/// One dataset's key, value and the offset after it, from the tag marker
/// at `at`.
type Dataset<'a> = (String, &'a [u8], usize);

/// Each key and its values.
pub type Datasets<'a> = Vec<(String, Vec<&'a [u8]>)>;

/// `iptcparse( $app13 )`: each dataset's key (`2#105`) and its values in
/// order, keys in the order first seen; `None` for `false`, when there is
/// no dataset.
pub fn iptcparse(b: &[u8]) -> Option<Datasets<'_>> {
    // The first tag marker of record 1 or 2 starts the datasets.
    let mut at = b.windows(2).position(|w| matches!(w, [0x1C, 1 | 2]))?;
    let mut out: Datasets = Vec::new();
    while let Some((key, value, next)) = dataset(b, at) {
        at = next;
        match out.iter_mut().find(|(known, _)| *known == key) {
            Some((_, values)) => values.push(value),
            None => out.push((key, vec![value])),
        }
    }
    (!out.is_empty()).then_some(out)
}

fn dataset(b: &[u8], at: usize) -> Option<Dataset<'_>> {
    let [0x1C, record, dataset, high, low, ..] = *b.get(at..).unwrap_or_default() else {
        return None;
    };
    let (length, start) = if high & 0x80 == 0 {
        (usize::from(u16::from_be_bytes([high, low])), at + 5)
    } else {
        // An extended dataset: its length in the four bytes after the two
        // that say so.
        let bytes = b.get(at + 5..at + 9)?;
        b.get(at + 9)?;
        (
            bytes.iter().fold(0, |n, &v| n * 256 + usize::from(v)),
            at + 9,
        )
    };
    let value = b.get(start..start.saturating_add(length))?;
    Some((format!("{record}#{dataset:03}"), value, start + length))
}

#[cfg(test)]
mod tests;
