// SPDX-License-Identifier: AGPL-3.0-or-later

//! The rewrite rules `WP_Rewrite::rewrite_rules()` generates for a
//! permalink structure: regexes in the order requests are matched against
//! them, each with the query it stands for.

/// The options the rules depend on.
#[derive(Debug, Default)]
pub(crate) struct Settings<'a> {
    pub(crate) structure: &'a str,
    pub(crate) category_base: &'a str,
    pub(crate) tag_base: &'a str,
    pub(crate) page_on_front: &'a str,
    /// The path of the home URL, which keeps the robots, favicon and
    /// sitemap rules out when it is not the root.
    pub(crate) home_path: &'a str,
}

const INDEX: &str = "index.php";
const FEED_REGEX2: &str = "(feed|rdf|rss|rss2|atom)/?$";
const TRACKBACK_REGEX: &str = "trackback/?$";
const PAGE_REGEX: &str = "page/?([0-9]{1,})/?$";
const COMMENT_REGEX: &str = "comment-page-([0-9]{1,})/?$";
const EMBED_REGEX: &str = "embed/?$";

/// Endpoint masks, which decide which comment page rules a structure gets,
/// and whether it walks its folders, is paged and has comment feeds.
#[derive(Clone, Copy, PartialEq)]
enum Mask {
    None,
    Permalink,
    Root,
    Comments,
    Pages,
}

/// Rules in PHP array order: setting a regex already there keeps its place.
#[derive(Default)]
struct Rules(Vec<(String, String)>);

impl Rules {
    fn set(&mut self, regex: String, query: String) {
        match self.0.iter_mut().find(|(r, _)| *r == regex) {
            Some(rule) => rule.1 = query,
            None => self.0.push((regex, query)),
        }
    }

    /// `array_merge( $self, $later )`.
    fn merge(mut self, later: Rules) -> Rules {
        for (regex, query) in later.0 {
            self.set(regex, query);
        }
        self
    }
}

impl<const N: usize> From<[(String, String); N]> for Rules {
    fn from(rules: [(String, String); N]) -> Rules {
        Rules::default().merge(Rules(rules.into()))
    }
}

/// `WP_Rewrite`: the rewrite tags with their regexes and query vars, and
/// the page on the front, which gets the root's comment page rule.
struct Rewrite<'a> {
    tags: Vec<(&'static str, &'static str, &'static str)>,
    page_on_front: &'a str,
}

pub(crate) mod pattern;

use crate::{Site, lossy, page_exists};
use hyscribe_db::DbError;
use hyscribe_phpser::{Value, parse_str, urldecode, urlencode};
use hyscribe_render::permalinks::{Permalinks, tokens};
use pattern::Pattern;

/// A site's rules, compiled once: each regex with its pattern and query,
/// and whether verbose page rules apply. Empty with plain permalinks.
#[derive(Debug, Default)]
pub(crate) struct Rewriter {
    rules: Vec<(String, Pattern, String)>,
    verbose: bool,
}

impl Rewriter {
    pub(crate) fn new(settings: &Settings) -> Rewriter {
        compile(rules(settings), verbose_page_rules(settings.structure))
    }

    pub(crate) fn is_empty(&self) -> bool {
        self.rules.is_empty()
    }
}

/// Rules whose regex the matcher does not read are left out.
fn compile(rules: Vec<(String, String)>, verbose: bool) -> Rewriter {
    Rewriter {
        rules: rules
            .into_iter()
            .filter_map(|(regex, query)| Some((Pattern::new(&regex)?, regex, query)))
            .map(|(pattern, regex, query)| (regex, pattern, query))
            .collect(),
        verbose,
    }
}

/// The rules, in order.
pub(crate) fn rules(settings: &Settings) -> Vec<(String, String)> {
    let structure = settings.structure;
    if structure.is_empty() {
        return Vec::new();
    }
    let links = Permalinks::new(structure, settings.category_base, settings.tag_base);
    let (front, root) = (links.front(), links.root());
    let mut rewrite = Rewrite {
        tags: vec![
            ("%year%", "([0-9]{4})", "year="),
            ("%monthnum%", "([0-9]{1,2})", "monthnum="),
            ("%day%", "([0-9]{1,2})", "day="),
            ("%hour%", "([0-9]{1,2})", "hour="),
            ("%minute%", "([0-9]{1,2})", "minute="),
            ("%second%", "([0-9]{1,2})", "second="),
            ("%postname%", "([^/]+)", "name="),
            ("%post_id%", "([0-9]+)", "p="),
            ("%author%", "([^/]+)", "author_name="),
            ("%pagename%", "([^/]+?)", "pagename="),
            ("%search%", "(.+)", "s="),
            ("%category%", "(.+?)", "category_name="),
            ("%post_tag%", "([^/]+)", "tag="),
            ("%post_format%", "([^/]+)", "post_format="),
        ],
        page_on_front: settings.page_on_front,
    };
    let extra = ["category", "post_tag", "post_format"]
        .iter()
        .filter_map(|taxonomy| links.term_structure(taxonomy))
        .map(|structure| rewrite.generate(&structure, Mask::None));

    let mut top = Rules::from(
        [
            ("^wp-json/?$", "index.php?rest_route=/"),
            ("^wp-json/(.*)?", "index.php?rest_route=/$matches[1]"),
            ("^index.php/wp-json/?$", "index.php?rest_route=/"),
            (
                "^index.php/wp-json/(.*)?",
                "index.php?rest_route=/$matches[1]",
            ),
            ("^wp-sitemap\\.xml$", "index.php?sitemap=index"),
            ("^wp-sitemap\\.xsl$", "index.php?sitemap-stylesheet=sitemap"),
            (
                "^wp-sitemap-index\\.xsl$",
                "index.php?sitemap-stylesheet=index",
            ),
            (
                "^wp-sitemap-([a-z]+?)-([a-z\\d_-]+?)-(\\d+?)\\.xml$",
                "index.php?sitemap=$matches[1]&sitemap-subtype=$matches[2]&paged=$matches[3]",
            ),
            (
                "^wp-sitemap-([a-z]+?)-(\\d+?)\\.xml$",
                "index.php?sitemap=$matches[1]&paged=$matches[2]",
            ),
        ]
        .map(|(r, q)| (r.to_string(), q.to_string())),
    );
    for rules in extra {
        top = top.merge(rules);
    }
    if matches!(settings.home_path, "" | "/") {
        top = top.merge(Rules::from(
            [
                ("robots\\.txt$", "index.php?robots=1"),
                ("favicon\\.ico$", "index.php?favicon=1"),
                ("sitemap\\.xml", "index.php?sitemap=index"),
            ]
            .map(|(r, q)| (r.to_string(), q.to_string())),
        ));
    }
    top = top.merge(Rules::from(
        [
            (
                ".*wp-(atom|rdf|rss|rss2|feed|commentsrss2)\\.php$",
                "index.php?feed=old",
            ),
            (".*wp-app\\.php(/.*)?$", "index.php?error=403"),
            (".*wp-register.php$", "index.php?register=true"),
        ]
        .map(|(r, q)| (r.to_string(), q.to_string())),
    ));

    let post = rewrite.generate(structure, Mask::Permalink);
    let date = rewrite.generate(&links.date_structure(), Mask::None);
    let root_rules = rewrite.generate(&format!("{root}/"), Mask::Root);
    let comments = rewrite.generate(&format!("{root}comments"), Mask::Comments);
    let search = rewrite.generate(&format!("{root}search/%search%"), Mask::None);
    let author = rewrite.generate(&format!("{front}author/%author%"), Mask::None);
    // `page_rewrite_rules()` widens the page name tag once the post rules
    // are built.
    for tag in &mut rewrite.tags {
        if tag.0 == "%pagename%" {
            tag.1 = "(.?.+?)";
        }
    }
    let page = rewrite.generate(&format!("{root}%pagename%"), Mask::Pages);
    let verbose = verbose_page_rules(structure);
    let (first, second) = if verbose { (page, post) } else { (post, page) };
    [root_rules, comments, search, author, date, first, second]
        .into_iter()
        .fold(top, Rules::merge)
        .0
}

/// `use_verbose_page_rules`: the structure starts with a token that page
/// paths could be mistaken for.
fn verbose_page_rules(structure: &str) -> bool {
    let first = structure
        .split_once('%')
        .and_then(|(_, rest)| rest.split_once('%'))
        .map(|(token, _)| token);
    matches!(first, Some("postname" | "category" | "tag" | "author"))
}

fn preg_index(n: usize) -> String {
    format!("$matches[{n}]")
}

impl Rewrite<'_> {
    /// `str_replace( $this->rewritecode, $replacements, $subject )`.
    fn replace(&self, subject: &str, query: bool) -> String {
        self.tags
            .iter()
            .fold(subject.to_string(), |s, (code, regex, var)| {
                s.replace(code, if query { var } else { regex })
            })
    }

    /// `generate_rewrite_rules()`, without endpoints (none are registered).
    fn generate(&self, structure: &str, mask: Mask) -> Rules {
        let (paged, for_comments) = (mask != Mask::Comments, mask == Mask::Comments);
        let walk_dirs = !matches!(mask, Mask::Comments | Mask::Pages);
        let feed_regex = format!("feed/{FEED_REGEX2}");
        let front = structure
            .get(..structure.find('%').unwrap_or(0))
            .unwrap_or_default();
        let mut queries: Vec<String> = Vec::new();
        for (i, token) in tokens(structure).into_iter().enumerate() {
            let previous = queries.last().map(|q| format!("{q}&")).unwrap_or_default();
            queries.push(format!(
                "{previous}{}{}",
                self.replace(token, true),
                preg_index(i + 1)
            ));
        }
        let rest = if front == "/" || front.is_empty() {
            structure.to_string()
        } else {
            structure.replace(front, "")
        };
        let rest = rest.trim_matches('/');
        let dirs: Vec<&str> = if walk_dirs {
            rest.split('/').collect()
        } else {
            vec![rest]
        };
        let mut post_rewrite = Rules::default();
        let mut path = front.trim_start_matches('/').to_string();
        for dir in dirs {
            path = format!("{path}{dir}/").trim_start_matches('/').to_string();
            let mut regex = self.replace(&path, false);
            let num_toks = tokens(&path).len();
            let query = num_toks
                .checked_sub(1)
                .and_then(|i| queries.get(i))
                .cloned()
                .unwrap_or_default();
            let next = preg_index(num_toks + 1);
            let feed_query = format!(
                "{INDEX}?{query}&feed={next}{}",
                if for_comments { "&withcomments=1" } else { "" }
            );
            let embed = (
                format!("{regex}{EMBED_REGEX}"),
                format!("{INDEX}?{query}&embed=true"),
            );
            let mut rewrite = Rules::from([
                (format!("{regex}{feed_regex}"), feed_query.clone()),
                (format!("{regex}{FEED_REGEX2}"), feed_query),
                embed.clone(),
            ]);
            if paged {
                rewrite.set(
                    format!("{regex}{PAGE_REGEX}"),
                    format!("{INDEX}?{query}&paged={next}"),
                );
            }
            if matches!(mask, Mask::Pages | Mask::Permalink) {
                rewrite.set(
                    format!("{regex}{COMMENT_REGEX}"),
                    format!("{INDEX}?{query}&cpage={next}"),
                );
            } else if mask == Mask::Root && !matches!(self.page_on_front, "" | "0") {
                rewrite.set(
                    format!("{regex}{COMMENT_REGEX}"),
                    format!(
                        "{INDEX}?{query}&page_id={}&cpage={next}",
                        self.page_on_front
                    ),
                );
            }
            if num_toks > 0 {
                let time = [
                    "%year%",
                    "%monthnum%",
                    "%day%",
                    "%hour%",
                    "%minute%",
                    "%second%",
                ];
                let post = ["%postname%", "%post_id%", "%pagename%"]
                    .iter()
                    .any(|t| path.contains(t))
                    || time.iter().all(|t| path.contains(t));
                let page = path.contains("%pagename%");
                if post {
                    let trackback = (
                        format!("{regex}{TRACKBACK_REGEX}"),
                        format!("{INDEX}?{query}&tb=1"),
                    );
                    let base: String = regex.trim_end_matches('/').replace(['(', ')'], "");
                    let sub = |sub: &str| {
                        let attachment = format!("{INDEX}?attachment={}", preg_index(1));
                        Rules::from([
                            (format!("{sub}?$"), attachment.clone()),
                            (
                                format!("{sub}{TRACKBACK_REGEX}"),
                                format!("{attachment}&tb=1"),
                            ),
                            (
                                format!("{sub}{feed_regex}"),
                                format!("{attachment}&feed={}", preg_index(2)),
                            ),
                            (
                                format!("{sub}{FEED_REGEX2}"),
                                format!("{attachment}&feed={}", preg_index(2)),
                            ),
                            (
                                format!("{sub}{COMMENT_REGEX}"),
                                format!("{attachment}&cpage={}", preg_index(2)),
                            ),
                            (
                                format!("{sub}{EMBED_REGEX}"),
                                format!("{attachment}&embed=true"),
                            ),
                        ])
                    };
                    regex = format!("{}(?:/([0-9]+))?/?$", regex.trim_end_matches('/'));
                    rewrite.set(regex, format!("{INDEX}?{query}&page={next}"));
                    rewrite = Rules::from([embed, trackback]).merge(rewrite);
                    if !page {
                        rewrite = rewrite.merge(sub(&format!("{base}/([^/]+)/")));
                    }
                    rewrite = sub(&format!("{base}/attachment/([^/]+)/")).merge(rewrite);
                } else {
                    rewrite.set(format!("{regex}?$"), format!("{INDEX}?{query}"));
                }
            }
            post_rewrite = rewrite.merge(post_rewrite);
        }
        post_rewrite
    }
}

/// What `WP::parse_request()` makes of a request's path.
#[derive(Debug, Default, PartialEq)]
pub(crate) struct Parsed {
    /// `matched_rule`, empty for none.
    pub(crate) rule: String,
    /// `matched_query`: the rule's query with the captures put in.
    pub(crate) query: String,
    /// The query vars the rule gives, before the request's own.
    pub(crate) vars: Vec<(String, Vec<u8>)>,
    /// No rule matched a path that needed one: `error=404`.
    pub(crate) not_found: bool,
}

/// The paths PHP sees for a request that reaches WordPress: `PATH_INFO` is
/// set for `/index.php/...`, as Apache sets it.
fn server_paths(home_path: &str, path: &str) -> (String, String) {
    let index = format!("{home_path}/index.php");
    match path
        .strip_prefix(&index)
        .filter(|rest| rest.starts_with('/'))
    {
        Some(info) => {
            // `rawurldecode()`, which leaves '+' alone.
            let info = urldecode(info.replace('+', "%2B").as_bytes());
            let info = String::from_utf8_lossy(&info).into_owned();
            (format!("{index}{info}"), info)
        }
        None => (index, String::new()),
    }
}

/// `WP::parse_request()`'s rule matching for the path of a request.
pub(crate) async fn parse_request(
    site: &Site,
    rewriter: &Rewriter,
    home_path: &str,
    path: &str,
) -> Result<Parsed, DbError> {
    let (php_self, pathinfo) = server_paths(home_path, path);
    let pathinfo = pathinfo.replace('%', "%25");
    let home = home_path.trim_matches('/');
    // `preg_replace( '|^home|i', '' )`, then trimmed of slashes.
    let strip = |s: &str| -> String {
        let s = s.trim_matches('/');
        let rest = s
            .get(..home.len())
            .filter(|start| !home.is_empty() && start.eq_ignore_ascii_case(home))
            .and_then(|_| s.get(home.len()..))
            .unwrap_or(s);
        rest.trim_matches('/').to_string()
    };
    let req_uri = if pathinfo.is_empty() {
        path.to_string()
    } else {
        path.replace(&pathinfo, "")
    };
    let (req_uri, pathinfo, php_self) = (strip(&req_uri), strip(&pathinfo), strip(&php_self));
    let index_info = Pattern::new("^.*index.php$").and_then(|p| p.captures(pathinfo.as_bytes()));
    let (requested_path, requested_file) = if !pathinfo.is_empty() && index_info.is_none() {
        (pathinfo, req_uri)
    } else {
        let req_uri = if req_uri == INDEX {
            String::new()
        } else {
            req_uri
        };
        (req_uri.clone(), req_uri)
    };
    let mut parsed = Parsed {
        not_found: true,
        ..Parsed::default()
    };
    // PHP's `empty()`: "0" is empty too.
    let empty = |s: &str| s.is_empty() || s == "0";
    if !empty(&requested_path) {
        let mut request_match = requested_path.clone();
        for (regex, pattern, query) in &rewriter.rules {
            if !requested_file.is_empty()
                && regex.starts_with(&requested_file)
                && requested_file != requested_path
            {
                request_match = format!("{requested_file}/{requested_path}");
            }
            let raw = request_match.as_bytes();
            let decoded = urldecode(raw);
            let Some((subject, captures)) = pattern
                .captures(raw)
                .map(|c| (raw, c))
                .or_else(|| pattern.captures(&decoded).map(|c| (&decoded[..], c)))
            else {
                continue;
            };
            // An unset group is an empty capture.
            let captures: Vec<&[u8]> = captures
                .iter()
                .map(|c| {
                    c.and_then(|(start, end)| subject.get(start..end))
                        .unwrap_or_default()
                })
                .collect();
            if rewriter.verbose
                && let Some(n) = page_capture(query)
                && !page_exists(site, captures.get(n).copied().unwrap_or_default()).await?
            {
                continue;
            }
            parsed.rule = regex.clone();
            // Everything up to the last '?' goes.
            let template = query
                .rsplit_once('?')
                .filter(|(head, _)| !head.is_empty())
                .map_or(query.as_str(), |(_, q)| q);
            parsed.query = substitute(template, &captures);
            parsed.vars = flat(&parse_str(parsed.query.as_bytes()));
            parsed.not_found = false;
            break;
        }
    }
    if empty(&requested_path) || requested_file == php_self {
        parsed.not_found = false;
    }
    Ok(parsed)
}

/// `WP::$public_query_vars`, with those core's `query_vars` filters add.
const PUBLIC_QUERY_VARS: [&str; 51] = [
    "m",
    "p",
    "posts",
    "w",
    "cat",
    "withcomments",
    "withoutcomments",
    "s",
    "search",
    "exact",
    "sentence",
    "calendar",
    "page",
    "paged",
    "more",
    "tb",
    "pb",
    "author",
    "order",
    "orderby",
    "year",
    "monthnum",
    "day",
    "hour",
    "minute",
    "second",
    "name",
    "category_name",
    "tag",
    "feed",
    "author_name",
    "pagename",
    "page_id",
    "error",
    "attachment",
    "attachment_id",
    "subpost",
    "subpost_id",
    "preview",
    "robots",
    "favicon",
    "taxonomy",
    "term",
    "cpage",
    "post_type",
    "embed",
    "post_format",
    "rest_route",
    "sitemap",
    "sitemap-subtype",
    "sitemap-stylesheet",
];

/// Whether `name` is a public query var, which `WP::parse_request()` reads.
pub(crate) fn is_query_var(name: &str) -> bool {
    PUBLIC_QUERY_VARS.contains(&name)
}

impl Parsed {
    /// The query vars the path gives, where the request's query does not
    /// set them: the rule's public ones, term names with their spaces back
    /// as '+', and `error=404` when no rule matched.
    pub(crate) fn query_vars(&self) -> Vec<(String, Vec<u8>)> {
        let mut vars: Vec<(String, Vec<u8>)> = self
            .vars
            .iter()
            .filter(|(name, _)| PUBLIC_QUERY_VARS.contains(&name.as_str()))
            .map(|(name, value)| {
                let value = if matches!(name.as_str(), "category_name" | "tag" | "post_format") {
                    value
                        .iter()
                        .map(|b| if *b == b' ' { b'+' } else { *b })
                        .collect()
                } else {
                    value.clone()
                };
                (name.clone(), value)
            })
            .collect();
        if self.not_found {
            vars.push(("error".into(), b"404".to_vec()));
        }
        vars
    }
}

/// The capture a verbose page rule takes its page path from:
/// `pagename=$matches[N]`.
fn page_capture(query: &str) -> Option<usize> {
    let (_, rest) = query.split_once("pagename=$matches[")?;
    let end = rest
        .find(|c: char| !c.is_ascii_digit())
        .unwrap_or(rest.len());
    let (digits, tail) = rest.split_at_checked(end).unwrap_or_default();
    if tail.starts_with(']') {
        digits.parse().ok()
    } else {
        None
    }
}

/// `WP_MatchesMapRegex::apply()`: each `$matches[N]` (N from 1) as its
/// capture, urlencoded, or nothing. The `addslashes()` after it changes
/// nothing urlencoded.
fn substitute(template: &str, captures: &[&[u8]]) -> String {
    let mut out = String::new();
    let mut rest = template;
    while let Some(at) = rest.find("$matches[") {
        out.push_str(rest.get(..at).unwrap_or_default());
        let after = rest.get(at + 9..).unwrap_or_default();
        let digits = after.bytes().take_while(u8::is_ascii_digit).count();
        let index = after
            .get(..digits)
            .filter(|d| !d.starts_with('0') && !d.is_empty());
        match index.zip(after.get(digits..).filter(|r| r.starts_with(']'))) {
            Some((index, tail)) => {
                let value = index
                    .parse()
                    .ok()
                    .and_then(|i: usize| captures.get(i))
                    .copied()
                    .unwrap_or_default();
                out.push_str(&String::from_utf8_lossy(&urlencode(value)));
                rest = tail.get(1..).unwrap_or_default();
            }
            None => {
                out.push_str("$matches[");
                rest = after;
            }
        }
    }
    out.push_str(rest);
    out
}

/// The string values of `parse_str()`'s result.
fn flat(value: &Value) -> Vec<(String, Vec<u8>)> {
    value
        .as_array()
        .unwrap_or_default()
        .iter()
        .filter_map(|(key, value)| Some((lossy(&key.to_php_string()), value.as_bytes()?.to_vec())))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every request of `reference/rewrite/request-golden.sh`.
    #[tokio::test]
    async fn requests_parse_as_in_php() {
        use hyscribe_phpser::json_decode;
        let site = crate::tests::oracle_site().await;
        let golden = include_str!("../tests/golden/parse-request.tsv");
        let mut rewriters = std::collections::HashMap::new();
        let mut cases = 0;
        for line in golden.lines() {
            let fields: Vec<&str> = line.split('\t').collect();
            let setup: Vec<&str> = fields[0].split('|').collect();
            let settings = Settings {
                structure: setup[0],
                category_base: setup[1],
                tag_base: setup[2],
                page_on_front: setup[3],
                home_path: "",
            };
            let rewriter = rewriters
                .entry(fields[0])
                .or_insert_with(|| Rewriter::new(&settings));
            let uri = String::from_utf8(hyscribe_testkit::unhex(fields[1])).unwrap();
            let (path, query) = uri.split_once('?').unwrap_or((&uri, ""));
            let parsed = parse_request(&site, rewriter, "", path).await.unwrap();
            // The request's `error` never survives parsing.
            let mut ours: Vec<(String, Vec<u8>)> = query
                .split('&')
                .filter_map(|pair| pair.split_once('='))
                .map(|(k, v)| (k.to_string(), urldecode(v.as_bytes())))
                .filter(|(k, _)| PUBLIC_QUERY_VARS.contains(&k.as_str()) && k != "error")
                .collect();
            let get: Vec<String> = ours.iter().map(|(k, _)| k.clone()).collect();
            ours.extend(
                parsed
                    .query_vars()
                    .into_iter()
                    .filter(|(k, _)| !get.contains(k)),
            );
            ours.sort();
            let php = json_decode(&hyscribe_testkit::unhex(fields[2])).unwrap();
            let field = |name: &str| php.get_str(name).unwrap().to_php_string();
            // `_post_format_request()`, which HyScribe applies later, is
            // undone: the format slug's prefix and the post types it sets.
            let mut vars: Vec<(String, Vec<u8>)> = php
                .get_str("vars")
                .unwrap()
                .as_array()
                .unwrap()
                .iter()
                .filter(|(k, _)| k.to_php_string() != b"post_type")
                .map(|(k, v)| {
                    let v = v.as_bytes().unwrap();
                    let format = k.to_php_string() == b"post_format";
                    let v = v
                        .strip_prefix(b"post-format-")
                        .filter(|_| format)
                        .unwrap_or(v);
                    (String::from_utf8(k.to_php_string()).unwrap(), v.to_vec())
                })
                .collect();
            vars.sort();
            let context = format!("{} {uri}", fields[0]);
            assert_eq!(parsed.rule.as_bytes(), field("rule"), "{context}");
            assert_eq!(parsed.query.as_bytes(), field("query"), "{context}");
            assert_eq!(ours, vars, "{context}");
            cases += 1;
        }
        assert_eq!(cases, 1876);
    }

    #[tokio::test]
    async fn odd_rules_and_queries() {
        let rules: Vec<(String, String)> = [
            ("(?=x)", "index.php?x=1"),
            ("(a)/?$", "index.php?pagename=$matches[1]"),
            (
                "a/?$",
                "q=$matches[1]&r=$matches[x&s=$matches[0]&t=$matches[2&u[]=1",
            ),
        ]
        .map(|(r, q)| (r.to_string(), q.to_string()))
        .into();
        let rewriter = compile(rules, true);
        assert_eq!(rewriter.rules.len(), 2);
        // The verbose page rule's page is not there.
        let site = crate::tests::oracle_site().await;
        let parsed = parse_request(&site, &rewriter, "", "/a/").await.unwrap();
        assert_eq!(parsed.rule, "a/?$");
        assert_eq!(
            parsed.query,
            "q=&r=$matches[x&s=$matches[0]&t=$matches[2&u[]=1"
        );
        let vars: Vec<String> = parsed.vars.iter().map(|(k, _)| k.clone()).collect();
        assert_eq!(vars, ["q", "r", "s", "t"]);
        assert_eq!(page_capture("pagename=$matches[2]"), Some(2));
        for query in ["x", "pagename=$matches[2", "pagename=$matches[]"] {
            assert_eq!(page_capture(query), None, "{query}");
        }
        let parsed = Parsed {
            vars: vec![
                ("category_name".into(), b"a b".to_vec()),
                ("name".into(), b"a b".to_vec()),
                ("secret".into(), b"x".to_vec()),
            ],
            ..Parsed::default()
        };
        assert_eq!(
            parsed.query_vars(),
            [
                ("category_name".to_string(), b"a+b".to_vec()),
                ("name".to_string(), b"a b".to_vec())
            ]
        );
    }

    /// Every setup of `reference/rewrite/golden.sh`.
    #[test]
    fn rules_match_php() {
        let golden = include_str!("../tests/golden/rewrite-rules.tsv");
        let mut setups = 0;
        for setup in golden.split("setup\t").skip(1) {
            let (head, expected) = setup.split_once('\n').unwrap();
            let fields: Vec<&str> = head.split('\t').collect();
            let settings = Settings {
                structure: fields[0],
                category_base: fields[1],
                tag_base: fields[2],
                page_on_front: fields[3],
                home_path: "",
            };
            let ours: String = rules(&settings)
                .iter()
                .map(|(regex, query)| format!("{regex}\t{query}\n"))
                .collect();
            assert_eq!(ours, expected, "{head}");
            setups += 1;
        }
        assert_eq!(setups, 16);
        assert!(rules(&Settings::default()).is_empty());
        let under = rules(&Settings {
            structure: "/%postname%/",
            home_path: "/blog",
            ..Settings::default()
        });
        assert!(!under.iter().any(|(r, _)| r == "robots\\.txt$"));
        assert_eq!(under.len(), 91);
    }
}
