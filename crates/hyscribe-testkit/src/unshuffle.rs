// SPDX-License-Identifier: AGPL-3.0-or-later

//! The Theme Test Data page of post 1031 shows `[gallery ...
//! orderby="rand"]`, which PHP orders by `RAND()`: no two runs repeat
//!. [`unshuffle`] puts the items of that one gallery in a fixed
//! order, as `reference/pages/ttd-body.py` does to PHP's page before it
//! hashes it, so the two compare as the same items in any order. The
//! `loading="lazy"` of an item, which WordPress gives by position, is
//! taken off with the item and put back where the page has it.

/// How the gallery's opening tag reads from its class on.
pub const MARK: &[u8] = b"class='gallery galleryid-1031 ";

/// The end of the gallery's `div`.
const END: &[u8] = b"\n\t\t</div>\n";

const ITEM: &[u8] = b"<figure class='gallery-item'>";
const ITEM_END: &[u8] = b"</figure>";
const LAZY: &[u8] = b"<img loading=\"lazy\" ";
const IMG: &[u8] = b"<img ";

/// `hay` from `from` to `to`, as far as it goes.
fn part(hay: &[u8], from: usize, to: usize) -> &[u8] {
    hay.get(from..to.min(hay.len())).unwrap_or_default()
}

/// The first place of `needle` in `hay` at or before `to` and at or after
/// `from`.
fn find(hay: &[u8], needle: &[u8], from: usize, to: usize) -> Option<usize> {
    part(hay, from, to)
        .windows(needle.len())
        .position(|w| w == needle)
        .map(|at| at + from)
}

/// `item` with its first `from` put as `to`.
fn swap(item: &[u8], from: &[u8], to: &[u8]) -> Vec<u8> {
    match find(item, from, 0, item.len()) {
        Some(at) => [
            part(item, 0, at),
            to,
            part(item, at + from.len(), item.len()),
        ]
        .concat(),
        None => item.to_vec(),
    }
}

/// Whether `page` shows the shuffled gallery.
pub fn shuffled(page: &[u8]) -> bool {
    find(page, MARK, 0, page.len()).is_some()
}

/// `page` with the items of its shuffled galleries in sorted order.
pub fn unshuffle(page: &[u8]) -> Vec<u8> {
    let whole = page.len();
    let mut out = Vec::new();
    let mut at = 0;
    while let Some(start) = find(page, MARK, at, whole) {
        let end = find(page, END, start, whole).unwrap_or(whole);
        let mut spans = Vec::new();
        let mut from = start;
        while let Some(first) = find(page, ITEM, from, end)
            && let Some(last) = find(page, ITEM_END, first, end)
        {
            from = last + ITEM_END.len();
            spans.push((first, from));
        }
        let items: Vec<&[u8]> = spans.iter().map(|&(a, b)| part(page, a, b)).collect();
        let lazy: Vec<bool> = items
            .iter()
            .map(|item| find(item, LAZY, 0, item.len()).is_some())
            .collect();
        let mut sorted: Vec<Vec<u8>> = items.iter().map(|item| swap(item, LAZY, IMG)).collect();
        sorted.sort();
        out.extend(part(page, at, start));
        let mut put = start;
        for ((first, last), (item, lazy)) in spans.into_iter().zip(sorted.iter().zip(lazy)) {
            out.extend(part(page, put, first));
            out.extend(match lazy {
                true => swap(item, IMG, LAZY),
                false => item.clone(),
            });
            put = last;
        }
        out.extend(part(page, put, end));
        at = end;
    }
    out.extend(part(page, at, whole));
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn item(id: u32, lazy: bool) -> String {
        let loading = if lazy { "loading=\"lazy\" " } else { "" };
        format!("<figure class='gallery-item'>\n<a><img {loading}id=\"{id}\" /></a></figure>")
    }

    fn gallery(items: &[String]) -> String {
        let (first, rest) = items.split_at(1);
        let rest = rest.join("<br />");
        format!(
            "<p>a</p><div id='gallery-1' class='gallery galleryid-1031 x'>{}<br />{rest}\n\t\t</div>\n<p>z</p>",
            first[0]
        )
    }

    #[test]
    fn the_same_items_in_any_order_read_alike() {
        let a = gallery(&[item(2, false), item(1, false), item(3, true)]);
        let b = gallery(&[item(3, false), item(2, false), item(1, true)]);
        assert!(shuffled(a.as_bytes()));
        assert_eq!(unshuffle(a.as_bytes()), unshuffle(b.as_bytes()));
        // Sorted, each slot keeps the loading attribute it had.
        let sorted = String::from_utf8(unshuffle(a.as_bytes())).unwrap();
        let expected = gallery(&[item(1, false), item(2, false), item(3, true)]);
        assert_eq!(sorted, expected);
    }

    #[test]
    fn a_different_item_or_loading_position_is_not_hidden() {
        let a = gallery(&[item(2, false), item(1, false), item(3, true)]);
        let other = gallery(&[item(2, false), item(1, false), item(4, true)]);
        assert_ne!(unshuffle(a.as_bytes()), unshuffle(other.as_bytes()));
        let lazy_first = gallery(&[item(2, true), item(1, false), item(3, false)]);
        assert_ne!(unshuffle(a.as_bytes()), unshuffle(lazy_first.as_bytes()));
        let elsewhere = a.replace("<p>z</p>", "<p>y</p>");
        assert_ne!(unshuffle(a.as_bytes()), unshuffle(elsewhere.as_bytes()));
    }

    #[test]
    fn other_pages_and_galleries_are_left_alone() {
        let other = b"<div class='gallery galleryid-7 x'><figure class='gallery-item'>b</figure><figure class='gallery-item'>a</figure>\n\t\t</div>\n";
        assert!(!shuffled(other));
        assert_eq!(unshuffle(other), other);
        let two = [
            gallery(&[item(2, false), item(1, false)]),
            gallery(&[item(4, false), item(3, false)]),
        ]
        .concat();
        let sorted = String::from_utf8(unshuffle(two.as_bytes())).unwrap();
        assert!(sorted.find("id=\"1\"").unwrap() < sorted.find("id=\"2\"").unwrap());
        assert!(sorted.find("id=\"3\"").unwrap() < sorted.find("id=\"4\"").unwrap());
    }

    #[test]
    fn an_item_without_its_end_stays_put() {
        let page = "<div class='gallery galleryid-1031 x'><figure class='gallery-item'>b</figure><figure class='gallery-item'>a";
        let sorted = unshuffle(page.as_bytes());
        assert_eq!(
            String::from_utf8(sorted).unwrap(),
            "<div class='gallery galleryid-1031 x'><figure class='gallery-item'>b</figure><figure class='gallery-item'>a"
        );
    }
}
