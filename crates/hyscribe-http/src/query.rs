// SPDX-License-Identifier: AGPL-3.0-or-later

//! Reading query vars from request input, as `WP_Query` and PHP do. Every
//! function here takes untrusted input, so the fuzz target
//! `http_query` covers them all.

use std::cmp::Ordering;
use std::collections::HashMap;

use hyscribe_model::{Term, TermFilter};
use hyscribe_phpser::{Key, Value, decode, is_empty_string, php_trim};
use hyscribe_render::formatting::{sanitize_title, sanitize_title_for_query};
use hyscribe_render::site::{Archive, Taxonomy};

/// `raw_query` without the `names` arguments, as `&`-joined pairs; an array
/// argument such as `p[]` goes with its name.
pub fn other_args(raw_query: Option<&str>, names: &[&str]) -> String {
    raw_query
        .unwrap_or_default()
        .split('&')
        .filter(|pair| {
            let key = pair.split(['=', '[']).next().unwrap_or_default();
            !pair.is_empty() && !names.contains(&key)
        })
        .map(|pair| format!("&{pair}"))
        .collect()
}

/// The `m` query var's date parts, as `WP_Query` reads them: digits only,
/// `YYYY`, then `MM`, then `DD`, none for `0`, which PHP takes as false.
pub fn date_parts(m: &str) -> (Option<u64>, Option<u64>, Option<u64>) {
    let digits: String = m.chars().filter(char::is_ascii_digit).collect();
    let part = |range: std::ops::Range<usize>| digits.get(range).and_then(|d| d.parse().ok());
    if digits.is_empty() || digits == "0" {
        return (None, None, None);
    }
    let year = part(0..4).or_else(|| digits.parse().ok());
    (year, part(4..6), part(6..8))
}

/// WordPress's `absint()` on request input.
pub fn absint(value: &str) -> u64 {
    hyscribe_phpser::absint(&hyscribe_phpser::Value::String(value.as_bytes().to_vec()))
}

/// The taxonomies `WP::parse_request()` keeps out of the `taxonomy` query
/// var, as they are not publicly queryable.
const PRIVATE_TAXONOMIES: [&str; 5] = [
    "nav_menu",
    "link_category",
    "wp_theme",
    "wp_template_part_area",
    "wp_pattern_category",
];

/// `get_post_format_strings()`: each format's slug, which
/// `_post_format_request()` prefixes, and its name.
const POST_FORMATS: [(&str, &str); 10] = [
    ("standard", "Standard"),
    ("aside", "Aside"),
    ("chat", "Chat"),
    ("gallery", "Gallery"),
    ("link", "Link"),
    ("image", "Image"),
    ("quote", "Quote"),
    ("status", "Status"),
    ("video", "Video"),
    ("audio", "Audio"),
];

/// `get_post_format_string()` of a format term's slug: the name
/// `_post_format_get_term()` gives the term.
fn format_name(slug: &[u8]) -> &'static str {
    let format = String::from_utf8_lossy(slug).replace("post-format-", "");
    POST_FORMATS
        .iter()
        .find(|(slug, _)| *slug == format)
        .map_or("", |(_, name)| name)
}

/// The terms of a [`TaxClause`]: slugs as the request gave them, or
/// category IDs.
#[derive(Clone, Debug, PartialEq, Eq)]
enum ClauseTerms {
    /// `field => slug`.
    Slugs(Vec<Vec<u8>>),
    /// `field => term_id`, from `cat`.
    Ids(Vec<u64>),
}

/// How a [`TaxClause`] matches posts.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Operator {
    /// Posts with any of the terms.
    In,
    /// Posts with none of them.
    NotIn,
    /// Posts with all of them.
    And,
}

/// A clause of the main query's `tax_query`.
#[derive(Clone, Debug, PartialEq, Eq)]
struct TaxClause {
    /// The taxonomy, which may be one WordPress does not have.
    taxonomy: String,
    /// The terms asked for.
    terms: ClauseTerms,
    /// How they match.
    operator: Operator,
}

/// The main query's `tax_query`, as `WP_Query::parse_tax_query()` builds it
/// from `taxonomy` and `term`, `category_name`, `post_format`, `cat` and
/// `tag`, with the vars `get_queried_object()` falls back to.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct TaxQuery {
    /// The clauses, in `tax_query` order.
    clauses: Vec<TaxClause>,
    /// `get_term( $cat )` where `cat` is set: its lowest ID.
    cat: Option<i64>,
    category_name: Vec<u8>,
    /// A `category_name` that `wp_basename()` empties: `parse_query()` makes
    /// a category archive of it, then `get_posts()` filters nothing by it.
    emptied_category: bool,
    tag: Vec<u8>,
}

/// The posts a tax query matches, and the archive it makes.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct Resolved {
    /// The posts' term filters, which all apply.
    pub filters: Vec<TermFilter>,
    /// `is_category()`, `is_tag()` or `is_tax()`, with the queried term.
    /// A taxonomy WordPress does not have, which finds nothing, reads as a
    /// post format: `is_tax()` without a term shows the same.
    pub archive: Option<Archive>,
    /// `is_tax()`, `is_category()` and `is_tag()`, in the template loader's
    /// order.
    pub kinds: Vec<Taxonomy>,
}

/// The main query's tax query of `query`.
pub fn tax_query(query: &HashMap<String, String>) -> TaxQuery {
    let var = |name: &str| query.get(name).map_or(&b""[..], |v| v.as_bytes());
    // WP::parse_request() turns a taxonomy var's spaces back into `+`.
    let plus = |name: &str| -> Vec<u8> {
        var(name)
            .iter()
            .map(|&b| if b == b' ' { b'+' } else { b })
            .collect()
    };
    let mut clauses = Vec::new();
    let taxonomy = query.get("taxonomy").map_or("", String::as_str);
    if !PRIVATE_TAXONOMIES.contains(&taxonomy)
        && !is_empty_string(taxonomy.as_bytes())
        && !is_empty_string(var("term"))
    {
        clauses.push(TaxClause {
            taxonomy: taxonomy.into(),
            terms: ClauseTerms::Slugs(vec![var("term").to_vec()]),
            operator: Operator::In,
        });
    }
    let category_name = basename(&plus("category_name"));
    let emptied_category =
        !is_empty_string(var("category_name")) && is_empty_string(&category_name);
    clauses.extend(slug_clauses("category", &category_name));
    let mut format = plus("post_format");
    if POST_FORMATS.iter().any(|(f, _)| f.as_bytes() == format) {
        format.splice(0..0, *b"post-format-");
    }
    clauses.extend(slug_clauses("post_format", &format));
    let cat: Vec<u8> = var("cat")
        .iter()
        .copied()
        .filter(|&b| b.is_ascii_digit() || b == b',' || b == b'-')
        .collect();
    let mut cat_set = None;
    if !is_empty_string(&cat) {
        let mut ids: Vec<i64> = runs(&cat, b",")
            .iter()
            .map(|id| Value::String(id.clone()).to_int())
            .collect();
        ids.sort_unstable();
        // `cat` becomes the sorted list, so only `0` alone is empty().
        cat_set = (ids != [0]).then(|| ids.first().copied().unwrap_or_default());
        let (included, excluded): (Vec<i64>, Vec<i64>) = ids
            .into_iter()
            .filter(|&id| id != 0)
            .partition(|id| id.is_positive());
        for (ids, operator) in [(included, Operator::In), (excluded, Operator::NotIn)] {
            if !ids.is_empty() {
                clauses.push(TaxClause {
                    taxonomy: "category".into(),
                    terms: ClauseTerms::Ids(ids.iter().map(|id| id.unsigned_abs()).collect()),
                    operator,
                });
            }
        }
    }
    let mut tag = plus("tag");
    if !tag.is_empty() {
        let (slugs, operator) = if tag.contains(&b',') {
            (runs(&tag, b",\r\n\t "), Operator::In)
        } else if tag.iter().any(|b| b"+\r\n\t ".contains(b)) || cat_set.is_some() {
            (runs(&tag, b"+\r\n\t "), Operator::And)
        } else {
            tag = sanitize_title(&tag);
            (vec![tag.clone()], Operator::In)
        };
        let mut unique: Vec<Vec<u8>> = Vec::new();
        for slug in slugs {
            let slug = sanitize_title(&slug);
            if !unique.contains(&slug) {
                unique.push(slug);
            }
        }
        let mut slugs: Vec<Vec<u8>> = unique.iter().map(|s| sanitize_title_for_query(s)).collect();
        slugs.sort_by(|a, b| php_compare(a, b));
        clauses.push(TaxClause {
            taxonomy: "post_tag".into(),
            terms: ClauseTerms::Slugs(slugs),
            operator,
        });
    }
    TaxQuery {
        clauses,
        cat: cat_set,
        category_name,
        emptied_category,
        tag,
    }
}

/// The clauses of a taxonomy's query var, as `parse_tax_query()` splits
/// it: sorted on commas, then one clause a `+` part, or one for them all.
fn slug_clauses(taxonomy: &str, value: &[u8]) -> Vec<TaxClause> {
    if is_empty_string(value) {
        return vec![];
    }
    let mut terms: Vec<&[u8]> = value.split(|&b| b == b',').map(php_trim).collect();
    terms.sort_by(|a, b| php_compare(a, b));
    let joined = terms.join(&b',');
    let clause = |terms| TaxClause {
        taxonomy: taxonomy.into(),
        terms: ClauseTerms::Slugs(terms),
        operator: Operator::In,
    };
    if joined.contains(&b'+') {
        runs(&joined, b"+")
            .into_iter()
            .map(|t| clause(vec![t]))
            .collect()
    } else {
        vec![clause(runs(&joined, b","))]
    }
}

/// `preg_split( '/[<delimiters>]+/', $text )`.
fn runs(text: &[u8], delimiters: &[u8]) -> Vec<Vec<u8>> {
    let parts: Vec<&[u8]> = text.split(|b| delimiters.contains(b)).collect();
    let last = parts.len() - 1;
    parts
        .into_iter()
        .enumerate()
        // A run of delimiters splits once; only the ends can be empty.
        .filter(|(i, part)| !part.is_empty() || *i == 0 || *i == last)
        .map(|(_, part)| part.to_vec())
        .collect()
}

/// PHP 8's `sort()` order of two strings: numeric ones by value, others
/// byte by byte.
fn php_compare(a: &[u8], b: &[u8]) -> Ordering {
    let (x, y) = (Value::String(a.to_vec()), Value::String(b.to_vec()));
    if x.is_numeric() && y.is_numeric() {
        x.to_float()
            .partial_cmp(&y.to_float())
            .unwrap_or(Ordering::Equal)
    } else {
        a.cmp(b)
    }
}

/// `wp_basename()`: the last part of `path`, `/` or `\\` before it.
pub(crate) fn basename(path: &[u8]) -> Vec<u8> {
    let end = path
        .iter()
        .rposition(|b| !b"/\\".contains(b))
        .map_or(0, |i| i + 1);
    let path = path.get(..end).unwrap_or_default();
    let start = path
        .iter()
        .rposition(|b| b"/\\".contains(b))
        .map_or(0, |i| i + 1);
    path.get(start..).unwrap_or_default().to_vec()
}

/// Whether a visitor can query the taxonomy `name`.
fn queryable(name: &str) -> bool {
    matches!(name, "category" | "post_tag" | "post_format")
}

impl TaxQuery {
    /// The terms [`TaxQuery::resolve`] looks up: `(taxonomy, slug)` pairs,
    /// slugs as `sanitize_title()` leaves them, and category IDs.
    pub fn lookups(&self) -> (Vec<(String, Vec<u8>)>, Vec<u64>) {
        let mut slugs = vec![
            ("category".to_string(), sanitize_title(&self.category_name)),
            ("post_tag".to_string(), sanitize_title(&self.tag)),
        ];
        let mut ids: Vec<u64> = self
            .cat
            .and_then(|id| u64::try_from(id).ok())
            .into_iter()
            .collect();
        for clause in &self.clauses {
            match &clause.terms {
                ClauseTerms::Slugs(terms) => slugs.extend(
                    terms
                        .iter()
                        .map(|slug| (clause.taxonomy.clone(), sanitize_title(slug))),
                ),
                ClauseTerms::Ids(terms) => ids.extend(terms.first()),
            }
        }
        // A taxonomy WordPress does not have finds nothing.
        slugs.retain(|(taxonomy, slug)| queryable(taxonomy) && !is_empty_string(slug));
        (slugs, ids)
    }

    /// The filters and the archive, over `terms`, the terms
    /// [`TaxQuery::lookups`] found, with the taxonomy of each, and
    /// `hierarchy`, the stored `category_children`.
    pub fn resolve(&self, terms: &[(String, Term)], hierarchy: &[u8]) -> Resolved {
        let tree = decode(hierarchy).unwrap_or(Value::Null);
        let by_slug = |taxonomy: &str, slug: &[u8]| {
            let slug = sanitize_title(slug);
            terms
                .iter()
                .find(|(t, term)| t == taxonomy && term.slug == slug)
                .map(|(_, term)| term)
        };
        let by_id = |id: u64| {
            terms
                .iter()
                .find(|(_, term)| term.id == id)
                .map(|(_, term)| term)
        };
        let mut filters = Vec::new();
        for clause in &self.clauses {
            let filter = |terms: Vec<u64>, exclude| TermFilter {
                taxonomy: clause.taxonomy.clone(),
                terms,
                exclude,
            };
            let hierarchical = clause.taxonomy == "category";
            let ids = match &clause.terms {
                ClauseTerms::Ids(ids) => ids.clone(),
                ClauseTerms::Slugs(slugs) => {
                    let asked: Vec<&Vec<u8>> =
                        slugs.iter().filter(|s| !is_empty_string(s)).collect();
                    let ids: Vec<u64> = asked
                        .iter()
                        .filter_map(|s| by_slug(&clause.taxonomy, s))
                        .map(|term| term.id)
                        .collect();
                    if clause.operator == Operator::And {
                        // No slugs restrict nothing; a missing one finds
                        // nothing.
                        if asked.is_empty() {
                            continue;
                        }
                        if ids.len() < slugs.len() {
                            filters.push(filter(vec![], false));
                            continue;
                        }
                        filters.extend(ids.into_iter().map(|id| filter(vec![id], false)));
                        continue;
                    }
                    ids
                }
            };
            let ids = if hierarchical {
                with_children(&tree, ids)
            } else {
                ids
            };
            filters.push(filter(ids, clause.operator == Operator::NotIn));
        }
        // A taxonomy's first clause, and in it the term the archive is of.
        let first = |name: Option<&str>| {
            self.clauses
                .iter()
                .find(|c| c.operator != Operator::NotIn && name.is_none_or(|n| c.taxonomy == n))
        };
        let first_term = |clause: &TaxClause| match &clause.terms {
            ClauseTerms::Slugs(slugs) => slugs.first().and_then(|s| by_slug(&clause.taxonomy, s)),
            ClauseTerms::Ids(ids) => ids.first().and_then(|&id| by_id(id)),
        };
        let category = first(Some("category"));
        let archive = if category.is_some() || self.emptied_category {
            let term = category.and_then(first_term).or_else(|| match self.cat {
                Some(id) => u64::try_from(id).ok().and_then(by_id),
                None => by_slug("category", &self.category_name),
            });
            Some(Archive::Term(Taxonomy::Category, term.cloned()))
        } else if let Some(clause) = first(Some("post_tag")) {
            let term = first_term(clause).or_else(|| by_slug("post_tag", &self.tag));
            Some(Archive::Term(Taxonomy::Tag, term.cloned()))
        } else {
            first(None).map(|clause| {
                let term = first_term(clause).map(|term| Term {
                    name: format_name(&term.slug).into(),
                    ..term.clone()
                });
                Archive::Term(Taxonomy::Format, term)
            })
        };
        let of = |taxonomy: fn(&str) -> bool| {
            self.clauses
                .iter()
                .any(|c| c.operator != Operator::NotIn && taxonomy(&c.taxonomy))
        };
        let kinds = [
            (Taxonomy::Format, of(|t| t != "category" && t != "post_tag")),
            (
                Taxonomy::Category,
                of(|t| t == "category") || self.emptied_category,
            ),
            (Taxonomy::Tag, of(|t| t == "post_tag")),
        ]
        .into_iter()
        .filter_map(|(kind, is)| is.then_some(kind))
        .collect();
        Resolved {
            filters,
            archive,
            kinds,
        }
    }
}

/// `ids` and, after them, their descendants in `tree`, as
/// get_term_children() walks it, once each where a cycle would loop.
fn with_children(tree: &Value, mut ids: Vec<u64>) -> Vec<u64> {
    let mut next = 0;
    while let Some(&id) = ids.get(next) {
        let children = tree
            .get(&Key::Int(i64::try_from(id).unwrap_or(i64::MAX)))
            .and_then(Value::as_array)
            .unwrap_or_default();
        for (_, child) in children {
            let child = child.to_int().unsigned_abs();
            if !ids.contains(&child) {
                ids.push(child);
            }
        }
        next += 1;
    }
    ids
}

/// `redirect_canonical()`'s last clean up of a query string: spaces and end
/// punctuation dropped after a final `p`, `page_id`, `cat` or `tag`, then
/// those arguments dropped where empty.
pub fn clean_query(query: &str) -> String {
    const PUNCTUATION: [&str; 24] = [
        " ",
        "%20",
        "!",
        "%21",
        "\"",
        "%22",
        "'",
        "%27",
        "(",
        "%28",
        ")",
        "%29",
        ",",
        "%2C",
        ".",
        "%2E",
        ";",
        "%3B",
        "{",
        "%7B",
        "}",
        "%7D",
        "%E2%80%9C",
        "%E2%80%9D",
    ];
    let names = ["p", "page_id", "cat", "tag"];
    let mut query = query.to_string();
    let last = query.rfind('&').map_or(0, |at| at + 1);
    let named = query
        .get(last..)
        .and_then(|arg| arg.split_once('='))
        .is_some_and(|(name, _)| names.contains(&name));
    // Neither `=` nor the names hold punctuation, so this stops at the value.
    while let Some(p) = PUNCTUATION.iter().find(|p| named && query.ends_with(*p)) {
        query.truncate(query.len() - p.len());
    }
    // `#(^|&)(p|page_id|cat|tag)=?(&|$)#` to `&`, matches never overlapping.
    let mut out = String::with_capacity(query.len());
    let mut rest = query.as_str();
    let mut at_start = true;
    while !rest.is_empty() {
        let lead = if rest.starts_with('&') { 1 } else { 0 };
        let empty = (at_start || lead == 1)
            .then(|| {
                names.iter().find_map(|name| {
                    let tail = rest.get(lead..).unwrap_or_default().strip_prefix(name)?;
                    let tail = tail.strip_prefix('=').unwrap_or(tail);
                    // The match takes the `&` after it too.
                    (tail.is_empty() || tail.starts_with('&'))
                        .then(|| rest.len() - tail.len() + usize::from(!tail.is_empty()))
                })
            })
            .flatten();
        match empty {
            Some(len) => {
                out.push('&');
                rest = rest.get(len..).unwrap_or_default();
            }
            None => {
                let c = rest.chars().next().unwrap_or_default();
                out.push(c);
                rest = rest.get(c.len_utf8()..).unwrap_or_default();
            }
        }
        at_start = false;
    }
    out.trim_matches('&').to_string()
}

/// The date part of a list's query, as `WP_Query` reads `m`, `year`,
/// `monthnum` and `day`.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct DateQuery {
    /// `YEAR(post_date)`.
    pub year: Option<u64>,
    /// `MONTH(post_date)`.
    pub month: Option<u64>,
    /// `DAYOFMONTH(post_date)`.
    pub day: Option<u64>,
    /// `m` and the other vars ask for different dates, which nothing has.
    pub conflicting: bool,
    /// The archive the title names: the year, then the month, then the
    /// day, the first of them asked for.
    pub kind: Option<Archive>,
    /// `error=404`: a month past 12, or a day not in its month, which the
    /// 404 page answers with a 200.
    pub invalid: bool,
    /// Where `redirect_canonical()` sends an invalid date: its month's
    /// `m=YYYYMM`, or for a month past 12 its year's `m=YYYY`.
    pub canonical: Option<String>,
}

/// `m`, `year`, `monthnum` and `day` as `WP_Query::parse_query()` reads
/// them: `day`, then `monthnum`, then `year` each make a date archive
/// unless one before did, and `m` adds its own.
pub fn date_query(
    m: Option<&str>,
    year: Option<&str>,
    monthnum: Option<&str>,
    day: Option<&str>,
) -> DateQuery {
    let (m_year, m_month, m_day) = m.map_or((None, None, None), date_parts);
    let var = |value: Option<&str>| value.map_or(0, absint);
    let (year, month, day) = (var(year), var(monthnum), var(day));
    let (mut is_day, mut is_month, mut is_year, mut invalid) = (false, false, false, false);
    if day > 0 {
        if month > 0 && year > 0 && !checkdate(month, day, year) {
            invalid = true;
        } else {
            is_day = true;
        }
    }
    if month > 0 && !is_day {
        if month > 12 {
            invalid = true;
        } else {
            is_month = true;
        }
    }
    is_year |= year > 0 && !is_day && !is_month;
    match (m_year, m_month, m_day) {
        (_, _, Some(_)) => is_day = true,
        (_, Some(_), None) => is_month = true,
        (Some(_), None, None) => is_year = true,
        (None, None, None) => {}
    }
    let kind = [
        (is_year, Archive::Year),
        (is_month, Archive::Month),
        (is_day, Archive::Day),
    ]
    .into_iter()
    .find_map(|(is, kind)| is.then_some(kind))
    .filter(|_| !invalid);
    let canonical = match (year, month, day) {
        (1.., 1.., 1..) if invalid => Some(format!("m={year}{month:02}")),
        (1.., 13.., _) => Some(format!("m={year}")),
        _ => None,
    };
    let merge = |from_m: Option<u64>, var: u64| match (from_m, (var > 0).then_some(var)) {
        (Some(a), Some(b)) => (Some(a), a != b),
        (a, b) => (a.or(b), false),
    };
    let ((year, a), (month, b), (day, c)) = (
        merge(m_year, year),
        merge(m_month, month),
        merge(m_day, day),
    );
    DateQuery {
        year,
        month,
        day,
        conflicting: a || b || c,
        kind,
        invalid,
        canonical,
    }
}

/// PHP's `checkdate()`: a month of the year and a day of that month, in
/// years 1 to 32767.
fn checkdate(month: u64, day: u64, year: u64) -> bool {
    let leap = year.is_multiple_of(4) && (!year.is_multiple_of(100) || year.is_multiple_of(400));
    let days = match month {
        2 if leap => 29,
        2 => 28,
        4 | 6 | 9 | 11 => 30,
        1..=12 => 31,
        _ => 0,
    };
    (1..=days).contains(&day) && (1..=32767).contains(&year)
}
