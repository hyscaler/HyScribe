// SPDX-License-Identifier: AGPL-3.0-or-later

//! `wp_filter_comment()` with the default `pre_comment_*` filters. PHP runs
//! them on slashed data, so each filter here does too, between
//! `addslashes()` and the `wp_unslash()` that comes before the insert.

use hyscribe_html::TagProcessor;
use hyscribe_kses::{esc_url_raw, wp_strip_all_tags};
use hyscribe_phpser::{addslashes, php_trim, stripslashes};
use hyscribe_render::balance::force_balance_tags;
use hyscribe_render::formatting::sanitize_email;
use hyscribe_render::kses::{wp_kses_comment, wp_kses_data, wp_rel_ugc};

use crate::{name_filters, wp_filter_kses};

/// The fields `wp_filter_comment()` filters, unslashed; the user agent, IP
/// address and user have no default filters.
pub(super) struct Fields<'a> {
    pub(super) author: &'a mut Vec<u8>,
    pub(super) email: &'a mut Vec<u8>,
    pub(super) url: &'a mut Vec<u8>,
    pub(super) content: &'a mut Vec<u8>,
}

impl Fields<'_> {
    /// Filters the fields in place: the content through kses when `kses`,
    /// links marked `ugc` for the site at `home`, and tags balanced when
    /// `balance` (`use_balanceTags`).
    pub(super) fn filter(self, kses: bool, home: &[u8], balance: bool) {
        let slashed = |field: &mut Vec<u8>, filters: &dyn Fn(Vec<u8>) -> Vec<u8>| {
            *field = stripslashes(&filters(addslashes(field)));
        };
        slashed(self.author, &|name| name_filters(&name));
        slashed(self.email, &|email| {
            wp_filter_kses(&sanitize_email(php_trim(&email)), wp_kses_data)
        });
        slashed(self.url, &|url| {
            let url = esc_url_raw(&wp_strip_all_tags(&url, false));
            wp_filter_kses(&url, wp_kses_data)
        });
        slashed(self.content, &|content| {
            let mut content = convert_invalid_entities(&content);
            if kses {
                content = wp_filter_kses(&content, wp_kses_comment);
                content = addslashes(&note_mention_classes(&stripslashes(&content)));
            }
            let content = addslashes(&wp_rel_ugc(&stripslashes(&content), home));
            balance_tags(&content, balance)
        });
    }
}

/// `_wp_kses_sanitize_note_mention_classes()`: a `span` keeps only the
/// classes of a note's mention, `wp-note-mention` and `user-N`.
fn note_mention_classes(content: &[u8]) -> Vec<u8> {
    let mut processor = TagProcessor::new(content);
    while processor.next_tag(Some(b"SPAN"), None) {
        for class in processor.class_list() {
            let user = class.strip_prefix(b"user-").is_some_and(|n| {
                n.first().is_some_and(|d| (b'1'..=b'9').contains(d))
                    && n.iter().all(u8::is_ascii_digit)
            });
            if class != b"wp-note-mention" && !user {
                processor.remove_class(&class);
            }
        }
    }
    processor.get_updated_html().to_vec()
}

/// `balanceTags()`: `force_balance_tags()` when `use_balanceTags` is on.
pub(super) fn balance_tags(text: &[u8], balance: bool) -> Vec<u8> {
    if balance {
        force_balance_tags(text)
    } else {
        text.to_vec()
    }
}

/// `convert_invalid_entities()`: the numeric references to the C1 controls
/// `&#128;` to `&#159;` become the Windows-1252 characters they meant.
pub(super) fn convert_invalid_entities(content: &[u8]) -> Vec<u8> {
    const WINDOWS_1252: [&[u8]; 32] = [
        b"&#8364;", b"", b"&#8218;", b"&#402;", b"&#8222;", b"&#8230;", b"&#8224;", b"&#8225;",
        b"&#710;", b"&#8240;", b"&#352;", b"&#8249;", b"&#338;", b"", b"&#381;", b"", b"",
        b"&#8216;", b"&#8217;", b"&#8220;", b"&#8221;", b"&#8226;", b"&#8211;", b"&#8212;",
        b"&#732;", b"&#8482;", b"&#353;", b"&#8250;", b"&#339;", b"", b"&#382;", b"&#376;",
    ];
    let mut out = Vec::with_capacity(content.len());
    let mut at = 0;
    while let Some(byte) = content.get(at) {
        let reference = content.get(at..at + 6).and_then(|entity| match entity {
            [
                b'&',
                b'#',
                b'1',
                tens @ b'0'..=b'9',
                ones @ b'0'..=b'9',
                b';',
            ] => {
                let n = usize::from(tens - b'0') * 10 + usize::from(ones - b'0');
                WINDOWS_1252.get(n.checked_sub(28)?)
            }
            _ => None,
        });
        match reference {
            Some(replacement) => {
                out.extend_from_slice(replacement);
                at += 6;
            }
            None => {
                out.push(*byte);
                at += 1;
            }
        }
    }
    out
}
