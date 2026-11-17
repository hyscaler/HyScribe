// SPDX-License-Identifier: AGPL-3.0-or-later

//! The parts of SimplePie 1.9 the RSS block reads: `Parser`'s data tree,
//! built from the XML reader's events as PHP's handlers build it (so the
//! cache holds what PHP's holds), the feed's type and items, and an item's
//! title, link, date, author and description, sanitized as WordPress's
//! `WP_SimplePie_Sanitize_KSES` does. RSS 0.9x, 1.0 and 2.0, Atom 0.3 and
//! 1.0, Dublin Core and iTunes elements are read where those getters read
//! them; Atom's `source` and SimplePie's IRI normalization are not ported.

use hyscribe_phpser::{Key, Value, php_trim};

use crate::kses::wp_kses_post;
use crate::xml::{Error, Event, NAMESPACE_XML, Name, Parser};

pub(crate) const ATOM_10: &[u8] = b"http://www.w3.org/2005/Atom";
pub(crate) const ATOM_03: &[u8] = b"http://purl.org/atom/ns#";
const RDF: &[u8] = b"http://www.w3.org/1999/02/22-rdf-syntax-ns#";
const RSS_090: &[u8] = b"http://my.netscape.com/rdf/simple/0.9/";
const RSS_10: &[u8] = b"http://purl.org/rss/1.0/";
const CONTENT: &[u8] = b"http://purl.org/rss/1.0/modules/content/";
const RSS_20: &[u8] = b"";
const DC_10: &[u8] = b"http://purl.org/dc/elements/1.0/";
const DC_11: &[u8] = b"http://purl.org/dc/elements/1.1/";
const ITUNES: &[u8] = b"http://www.itunes.com/dtds/podcast-1.0.dtd";
const XHTML: &[u8] = b"http://www.w3.org/1999/xhtml";

/// SimplePie's feed types, as bits.
const TYPE_RSS_090: i64 = 1;
const TYPE_RSS_091: i64 = 6;
const TYPE_RSS_092: i64 = 8;
const TYPE_RSS_093: i64 = 16;
const TYPE_RSS_094: i64 = 32;
const TYPE_RSS_10: i64 = 64;
const TYPE_RSS_20: i64 = 128;
const TYPE_RSS_SYNDICATION: i64 = 190;
const TYPE_RSS_ALL: i64 = 255;
const TYPE_ATOM_03: i64 = 256;
const TYPE_ATOM_10: i64 = 512;
const TYPE_ATOM_ALL: i64 = 768;
const TYPE_ALL: i64 = 1023;

/// The sanitizer's constructs.
const TEXT: u8 = 1;
const HTML: u8 = 2;
const XHTML_CONSTRUCT: u8 = 4;
const MAYBE_HTML: u8 = 16;
const IRI: u8 = 64;

/// Lists grouped by namespace, then by name.
type Grouped<T> = Vec<(Vec<u8>, Vec<(Vec<u8>, T)>)>;

/// An open element: its name, whether it opened a node of the tree, and
/// its `xml:base`, whether that was set, and `xml:lang`.
type Open = (Name, bool, Vec<u8>, bool, Vec<u8>);

/// An element of the tree, as `Parser` keeps it.
#[derive(Default)]
struct Node {
    data: Vec<u8>,
    attribs: Grouped<Vec<u8>>,
    base: Vec<u8>,
    explicit: bool,
    lang: Vec<u8>,
    child: Grouped<Vec<Value>>,
}

impl Node {
    fn into_value(self) -> Value {
        let mut entries = vec![
            (key(b"data"), Value::String(self.data)),
            (key(b"attribs"), groups(self.attribs, Value::String)),
            (key(b"xml_base"), Value::String(self.base)),
            (key(b"xml_base_explicit"), Value::Bool(self.explicit)),
            (key(b"xml_lang"), Value::String(self.lang)),
        ];
        if !self.child.is_empty() {
            entries.push((key(b"child"), children(self.child)));
        }
        Value::Array(entries)
    }

    fn adopt(&mut self, name: &Name, child: Value) {
        let at = match self.child.iter().position(|(ns, _)| *ns == name.namespace) {
            Some(at) => at,
            None => {
                self.child.push((name.namespace.clone(), Vec::new()));
                self.child.len() - 1
            }
        };
        if let Some((_, locals)) = self.child.get_mut(at) {
            match locals.iter_mut().find(|(local, _)| *local == name.local) {
                Some((_, list)) => list.push(child),
                None => locals.push((name.local.clone(), vec![child])),
            }
        }
    }
}

/// `array( namespace => array( name => value ) )`.
fn groups<T>(groups: Grouped<T>, value: fn(T) -> Value) -> Value {
    Value::Array(
        groups
            .into_iter()
            .map(|(ns, entries)| {
                let entries = entries
                    .into_iter()
                    .map(|(name, v)| (key(&name), value(v)))
                    .collect();
                (key(&ns), Value::Array(entries))
            })
            .collect(),
    )
}

/// A node's children, each list indexed from 0.
fn children(child: Grouped<Vec<Value>>) -> Value {
    groups(child, |list: Vec<Value>| {
        Value::Array(
            list.into_iter()
                .enumerate()
                .map(|(at, v)| (Key::Int(i64::try_from(at).unwrap_or_default()), v))
                .collect(),
        )
    })
}

/// A PHP array key: a decimal integer's string is that integer.
pub(crate) fn key(name: &[u8]) -> Key {
    let digits = name.strip_prefix(b"-").unwrap_or(name);
    let canonical = !digits.is_empty()
        && digits.iter().all(u8::is_ascii_digit)
        && (digits == b"0" || digits.first() != Some(&b'0'))
        && name != b"-0";
    match std::str::from_utf8(name).ok().and_then(|n| n.parse().ok()) {
        Some(int) if canonical => Key::Int(int),
        _ => Key::String(name.to_vec()),
    }
}

/// `htmlspecialchars( $text, ENT_QUOTES )`, or `ENT_COMPAT` without
/// `quotes`.
pub(crate) fn specialchars(text: &[u8], quotes: bool) -> Vec<u8> {
    let mut out = Vec::with_capacity(text.len());
    for &b in text {
        match b {
            b'&' => out.extend_from_slice(b"&amp;"),
            b'"' => out.extend_from_slice(b"&quot;"),
            b'\'' if quotes => out.extend_from_slice(b"&#039;"),
            b'<' => out.extend_from_slice(b"&lt;"),
            b'>' => out.extend_from_slice(b"&gt;"),
            _ => out.push(b),
        }
    }
    out
}

/// `Parser::parse()`'s data tree of `xml`, or the reader's error.
pub(crate) fn parse(xml: &[u8]) -> Result<Value, Error> {
    let mut parser = Parser::new(xml, true);
    let mut root = Node::default();
    // The open elements: the node each one opened (none inside a construct).
    let mut stack: Vec<Open> = Vec::new();
    let mut construct: i64 = -1;
    let mut current: Vec<Node> = Vec::new();
    loop {
        match parser.read()? {
            Event::Eof => break,
            Event::Doctype => {}
            Event::Start { name, attributes } => {
                let (parent_base, parent_explicit, parent_lang) = stack
                    .last()
                    .map(|(_, _, base, explicit, lang)| (base.clone(), *explicit, lang.clone()))
                    .unwrap_or_default();
                let mut attribs: Grouped<Vec<u8>> = Vec::new();
                for (attribute, value) in attributes {
                    match attribs
                        .iter_mut()
                        .find(|(ns, _)| *ns == attribute.namespace)
                    {
                        Some((_, list)) => list.push((attribute.local, value)),
                        None => attribs.push((attribute.namespace, vec![(attribute.local, value)])),
                    }
                }
                let xml = |local: &[u8]| {
                    attribs
                        .iter()
                        .find(|(ns, _)| ns == NAMESPACE_XML)
                        .and_then(|(_, list)| list.iter().find(|(name, _)| name == local))
                        .map(|(_, value)| value.clone())
                };
                let (base, explicit) = match xml(b"base") {
                    Some(base) => (absolutize(&base, &parent_base), true),
                    None => (parent_base, parent_explicit),
                };
                let lang = xml(b"lang").unwrap_or(parent_lang);
                if construct >= 0 {
                    construct += 1;
                    if name.namespace == XHTML
                        && let Some(node) = current.last_mut()
                    {
                        node.data.push(b'<');
                        node.data.extend_from_slice(&name.local);
                        let plain = attribs.iter().find(|(ns, _)| ns.is_empty());
                        for (attribute, value) in
                            plain.map(|(_, l)| l.as_slice()).unwrap_or_default()
                        {
                            node.data.extend_from_slice(b" ");
                            node.data.extend_from_slice(attribute);
                            node.data.extend_from_slice(b"=\"");
                            node.data.extend(specialchars(value, false));
                            node.data.extend_from_slice(b"\"");
                        }
                        node.data.push(b'>');
                    }
                    stack.push((name, false, base, explicit, lang));
                    continue;
                }
                let attribute = |local: &[u8]| {
                    attribs
                        .iter()
                        .find(|(ns, _)| ns.is_empty())
                        .and_then(|(_, list)| list.iter().find(|(name, _)| name == local))
                        .map(|(_, value)| value.as_slice())
                };
                let ns = name.namespace.as_slice();
                let local = name.local.as_slice();
                let starts = (ns == ATOM_03
                    && matches!(
                        local,
                        b"title" | b"tagline" | b"copyright" | b"info" | b"summary" | b"content"
                    )
                    && attribute(b"mode") == Some(b"xml"))
                    || (ns == ATOM_10
                        && matches!(
                            local,
                            b"rights" | b"subtitle" | b"summary" | b"info" | b"title" | b"content"
                        )
                        && attribute(b"type") == Some(b"xhtml"))
                    || (matches!(
                        ns,
                        b"" | b"http://my.netscape.com/rdf/simple/0.9/"
                            | b"http://purl.org/rss/1.0/"
                    ) && local == b"title");
                if starts {
                    construct = 0;
                }
                current.push(Node {
                    data: Vec::new(),
                    attribs,
                    base: base.clone(),
                    explicit,
                    lang: lang.clone(),
                    child: Vec::new(),
                });
                stack.push((name, true, base, explicit, lang));
            }
            Event::Text(text) => {
                let node = current.last_mut().unwrap_or(&mut root);
                if construct >= 0 {
                    node.data.extend(specialchars(&text, true));
                } else {
                    node.data.extend(text);
                }
            }
            Event::End => {
                let Some((name, opened, ..)) = stack.pop() else {
                    continue;
                };
                if construct >= 0 {
                    construct -= 1;
                    const VOID: [&[u8]; 13] = [
                        b"area",
                        b"base",
                        b"basefont",
                        b"br",
                        b"col",
                        b"frame",
                        b"hr",
                        b"img",
                        b"input",
                        b"isindex",
                        b"link",
                        b"meta",
                        b"param",
                    ];
                    if name.namespace == XHTML
                        && !VOID.contains(&name.local.as_slice())
                        && let Some(node) = current.last_mut()
                    {
                        node.data.extend([&b"</"[..], &name.local, b">"].concat());
                    }
                }
                if construct == -1
                    && opened
                    && let Some(node) = current.pop()
                {
                    let parent = current.last_mut().unwrap_or(&mut root);
                    parent.adopt(&name, node.into_value());
                }
            }
        }
    }
    Ok(match root.child.is_empty() {
        true => Value::Array(Vec::new()),
        false => Value::Array(vec![(key(b"child"), children(root.child))]),
    })
}

/// `$value[ $key ]` for a string or integer key.
pub(crate) fn at<'a>(value: &'a Value, key: &[u8]) -> Option<&'a Value> {
    value
        .as_array()?
        .iter()
        .find(|(k, _)| k.to_php_string() == key)
        .map(|(_, v)| v)
}

/// `$value[ $a ][ $b ]...`
fn path<'a>(value: &'a Value, keys: &[&[u8]]) -> Option<&'a Value> {
    keys.iter().try_fold(value, |value, key| at(value, key))
}

/// An element's `['child'][ $ns ][ $tag ]`, a list.
fn tags<'a>(element: &'a Value, ns: &[u8], tag: &[u8]) -> Option<&'a Value> {
    path(element, &[b"child", ns, tag]).filter(|list| !list.is_empty())
}

/// The first of a list.
fn first(list: Option<&Value>) -> Option<&Value> {
    list?.as_array()?.first().map(|(_, v)| v)
}

/// An element's `data`.
fn data(element: &Value) -> Vec<u8> {
    at(element, b"data")
        .map(Value::to_php_string)
        .unwrap_or_default()
}

/// An element's attribute without a namespace.
fn attribute<'a>(element: &'a Value, name: &[u8]) -> Option<&'a Value> {
    path(element, &[b"attribs", b"", name])
}

/// A feed: its data tree.
pub(crate) struct Feed<'a> {
    pub(crate) data: &'a Value,
}

impl Feed<'_> {
    /// `get_type()`, as the tree's `type` holds it or as it is found.
    pub(crate) fn kind(data: &Value) -> i64 {
        if let Some(kind) = at(data, b"type") {
            return kind.to_int();
        }
        let root = |ns: &[u8], tag: &[u8]| tags(data, ns, tag).is_some();
        if root(ATOM_10, b"feed") {
            return TYPE_ALL & TYPE_ATOM_10;
        }
        if root(ATOM_03, b"feed") {
            return TYPE_ALL & TYPE_ATOM_03;
        }
        if let Some(rdf) = first(tags(data, RDF, b"RDF")) {
            let mut kind = TYPE_ALL;
            let has = |ns: &[u8]| {
                [&b"channel"[..], b"image", b"item", b"textinput"]
                    .iter()
                    .any(|tag| tags(rdf, ns, tag).is_some())
            };
            if has(RSS_10) {
                kind &= TYPE_RSS_10;
            }
            if has(RSS_090) {
                kind &= TYPE_RSS_090;
            }
            return kind;
        }
        let Some(rss) = first(tags(data, RSS_20, b"rss")) else {
            return 0;
        };
        let mut kind = TYPE_ALL & TYPE_RSS_ALL;
        let version = attribute(rss, b"version").map(Value::to_php_string);
        match version.as_deref().map(php_trim) {
            Some(b"0.91") => kind &= TYPE_RSS_091,
            Some(b"0.92") => kind &= TYPE_RSS_092,
            Some(b"0.93") => kind &= TYPE_RSS_093,
            Some(b"0.94") => kind &= TYPE_RSS_094,
            Some(b"2.0") => kind &= TYPE_RSS_20,
            _ => {}
        }
        kind
    }

    /// `get_feed_tags( $ns, $tag )`.
    fn feed_tags(&self, ns: &[u8], tag: &[u8]) -> Option<&Value> {
        let kind = Feed::kind(self.data);
        let root = |root_ns: &[u8], root: &[u8]| first(tags(self.data, root_ns, root));
        let element = if kind & TYPE_ATOM_10 != 0 {
            root(ATOM_10, b"feed")
        } else if kind & TYPE_ATOM_03 != 0 {
            root(ATOM_03, b"feed")
        } else if kind & (TYPE_RSS_10 | TYPE_RSS_090) != 0 {
            root(RDF, b"RDF")
        } else if kind & TYPE_RSS_SYNDICATION != 0 {
            root(RSS_20, b"rss")
        } else {
            None
        };
        tags(element?, ns, tag)
    }

    /// `get_channel_tags( $ns, $tag )`.
    fn channel_tags(&self, ns: &[u8], tag: &[u8]) -> Option<&Value> {
        let kind = Feed::kind(self.data);
        if kind & TYPE_ATOM_ALL != 0
            && let Some(found) = self.feed_tags(ns, tag)
        {
            return Some(found);
        }
        let channel = |channel_ns: &[u8]| first(self.feed_tags(channel_ns, b"channel"));
        (kind & TYPE_RSS_10 != 0)
            .then(|| tags(channel(RSS_10)?, ns, tag))
            .flatten()
            .or_else(|| {
                (kind & TYPE_RSS_090 != 0)
                    .then(|| tags(channel(RSS_090)?, ns, tag))
                    .flatten()
            })
            .or_else(|| {
                (kind & TYPE_RSS_SYNDICATION != 0)
                    .then(|| tags(channel(RSS_20)?, ns, tag))
                    .flatten()
            })
    }

    /// `get_link()`: the feed's first alternate link.
    fn link(&self) -> Option<Vec<u8>> {
        for ns in [ATOM_10, ATOM_03] {
            for (_, link) in self
                .channel_tags(ns, b"link")
                .and_then(Value::as_array)
                .unwrap_or_default()
            {
                let rel = attribute(link, b"rel").map(Value::to_php_string);
                if let Some(href) = attribute(link, b"href")
                    && rel.as_deref().is_none_or(|rel| rel == b"alternate")
                {
                    let base = explicit_base(link).unwrap_or_default();
                    return Some(sanitize(&href.to_php_string(), IRI, &base));
                }
            }
        }
        for ns in [RSS_10, RSS_090, RSS_20] {
            if let Some(link) = first(self.channel_tags(ns, b"link")) {
                let base = explicit_base(link).unwrap_or_default();
                return Some(sanitize(&data(link), IRI, &base));
            }
        }
        None
    }

    /// `get_base( $element )`.
    fn base(&self, element: &Value) -> Vec<u8> {
        explicit_base(element)
            .or_else(|| self.link())
            .unwrap_or_default()
    }

    /// `get_authors()` of the feed.
    fn authors(&self) -> Vec<Author> {
        let mut authors = atom_authors(self.feed_tags(ATOM_10, b"author"), ATOM_10, b"uri");
        authors.extend(atom_authors(
            first(self.feed_tags(ATOM_03, b"author"))
                .map(single)
                .as_ref(),
            ATOM_03,
            b"url",
        ));
        for ns in [DC_11, DC_10, ITUNES] {
            let names = if ns == ITUNES {
                b"author" as &[u8]
            } else {
                b"creator"
            };
            authors.extend(names_of(self.channel_tags(ns, names)));
        }
        unique(authors)
    }

    /// `get_items()`, ordered by date, newest first, undated first of all.
    pub(crate) fn items(&self) -> Vec<Item<'_>> {
        let mut items: Vec<Item<'_>> = Vec::new();
        let lists = [
            self.feed_tags(ATOM_10, b"entry"),
            self.feed_tags(ATOM_03, b"entry"),
            self.feed_tags(RSS_10, b"item"),
            self.feed_tags(RSS_090, b"item"),
            self.channel_tags(RSS_20, b"item"),
        ];
        for list in lists.into_iter().flatten() {
            for (_, element) in list.as_array().unwrap_or_default() {
                items.push(Item {
                    feed: self,
                    element,
                });
            }
        }
        let mut keyed: Vec<(Option<i64>, Item<'_>)> =
            items.into_iter().map(|item| (item.date(), item)).collect();
        keyed.sort_by(|(a, _), (b, _)| match (a, b) {
            (Some(a), Some(b)) => b.cmp(a),
            (Some(_), None) => std::cmp::Ordering::Greater,
            (None, Some(_)) => std::cmp::Ordering::Less,
            (None, None) => std::cmp::Ordering::Equal,
        });
        keyed.into_iter().map(|(_, item)| item).collect()
    }
}

/// A one element list of `element`.
fn single(element: &Value) -> Value {
    Value::Array(vec![(Key::Int(0), element.clone())])
}

/// An element's own `xml:base`, when one was set.
fn explicit_base(element: &Value) -> Option<Vec<u8>> {
    let explicit = at(element, b"xml_base_explicit").is_some_and(|v| !v.is_empty());
    explicit
        .then(|| at(element, b"xml_base").map(Value::to_php_string))
        .flatten()
}

/// An author: name, link and email.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Author {
    pub(crate) name: Option<Vec<u8>>,
    link: Option<Vec<u8>>,
    email: Option<Vec<u8>>,
}

/// Atom's authors in `list`.
fn atom_authors(list: Option<&Value>, ns: &[u8], link: &[u8]) -> Vec<Author> {
    let mut authors = Vec::new();
    for (_, author) in list.and_then(Value::as_array).unwrap_or_default() {
        let part = |name: &[u8]| first(tags(author, ns, name)).map(|e| (data(e), e));
        let name = part(b"name").map(|(d, _)| sanitize(&d, TEXT, b""));
        let uri = part(link).map(|(d, e)| sanitize(&d, IRI, &explicit_base(e).unwrap_or_default()));
        let email = part(b"email").map(|(d, _)| sanitize(&d, TEXT, b""));
        if name.is_some() || uri.is_some() || email.is_some() {
            authors.push(Author {
                name,
                link: uri,
                email,
            });
        }
    }
    authors
}

/// Authors named by the elements of `list`.
fn names_of(list: Option<&Value>) -> Vec<Author> {
    list.and_then(Value::as_array)
        .unwrap_or_default()
        .iter()
        .map(|(_, element)| Author {
            name: Some(sanitize(&data(element), TEXT, b"")),
            link: None,
            email: None,
        })
        .collect()
}

/// `array_unique()` of authors.
fn unique(authors: Vec<Author>) -> Vec<Author> {
    let mut kept: Vec<Author> = Vec::new();
    for author in authors {
        if !kept.contains(&author) {
            kept.push(author);
        }
    }
    kept
}

/// An item of a feed.
pub(crate) struct Item<'a> {
    feed: &'a Feed<'a>,
    element: &'a Value,
}

impl Item<'_> {
    fn tags(&self, ns: &[u8], tag: &[u8]) -> Option<&Value> {
        first(tags(self.element, ns, tag))
    }

    /// `get_base( $element )`.
    fn base(&self, element: &Value) -> Vec<u8> {
        explicit_base(element)
            .or_else(|| self.link())
            .unwrap_or_else(|| self.feed.base(element))
    }

    /// `get_title()`.
    pub(crate) fn title(&self) -> Option<Vec<u8>> {
        let construct = [
            (ATOM_10, None),
            (ATOM_03, None),
            (RSS_10, Some(MAYBE_HTML)),
            (RSS_090, Some(MAYBE_HTML)),
            (RSS_20, Some(MAYBE_HTML)),
            (DC_11, Some(TEXT)),
            (DC_10, Some(TEXT)),
        ];
        construct.into_iter().find_map(|(ns, kind)| {
            let element = self.tags(ns, b"title")?;
            let kind = kind.unwrap_or_else(|| construct_type(element, ns, false));
            let base = if matches!(kind, TEXT) && (ns == DC_11 || ns == DC_10) {
                Vec::new()
            } else {
                self.base(element)
            };
            Some(sanitize(&data(element), kind, &base))
        })
    }

    /// `get_description()`, falling back on `get_content( true )`.
    pub(crate) fn description(&self) -> Option<Vec<u8>> {
        let summary: [(&[u8], &[u8], Option<u8>); 9] = [
            (ATOM_10, b"summary", None),
            (ATOM_03, b"summary", None),
            (RSS_10, b"description", Some(MAYBE_HTML)),
            (RSS_20, b"description", Some(HTML)),
            (DC_11, b"description", Some(TEXT)),
            (DC_10, b"description", Some(TEXT)),
            (ITUNES, b"summary", Some(HTML)),
            (ITUNES, b"subtitle", Some(TEXT)),
            (RSS_090, b"description", Some(HTML)),
        ];
        let content: [(&[u8], &[u8], Option<u8>); 3] = [
            (ATOM_10, b"content", None),
            (ATOM_03, b"content", None),
            (CONTENT, b"encoded", Some(HTML)),
        ];
        summary
            .iter()
            .chain(content.iter())
            .find_map(|&(ns, tag, kind)| {
                let element = self.tags(ns, tag)?;
                let kind = kind.unwrap_or_else(|| construct_type(element, ns, tag == b"content"));
                let sanitized = sanitize(&data(element), kind, &self.base(element));
                (!hyscribe_phpser::is_empty_string(&sanitized)).then_some(sanitized)
            })
    }

    /// `get_link()`: the first alternate link.
    pub(crate) fn link(&self) -> Option<Vec<u8>> {
        let mut links = Vec::new();
        for ns in [ATOM_10, ATOM_03] {
            for (_, link) in tags(self.element, ns, b"link")
                .and_then(Value::as_array)
                .unwrap_or_default()
            {
                let rel = attribute(link, b"rel").map(Value::to_php_string);
                if let Some(href) = attribute(link, b"href")
                    && rel.as_deref().is_none_or(|rel| rel == b"alternate")
                {
                    links.push(sanitize(&href.to_php_string(), IRI, &self.own_base(link)));
                }
            }
        }
        for ns in [RSS_10, RSS_090, RSS_20] {
            if let Some(link) = self.tags(ns, b"link") {
                links.push(sanitize(&data(link), IRI, &self.own_base(link)));
            }
        }
        if let Some(guid) = self.tags(RSS_20, b"guid") {
            let permalink = attribute(guid, b"isPermaLink").map(Value::to_php_string);
            if permalink.is_none_or(|p| php_trim(&p).eq_ignore_ascii_case(b"true")) {
                links.push(sanitize(&data(guid), IRI, &self.own_base(guid)));
            }
        }
        links.into_iter().next()
    }

    /// `get_own_base()`.
    fn own_base(&self, element: &Value) -> Vec<u8> {
        explicit_base(element)
            .or_else(|| self.feed.link())
            .unwrap_or_default()
    }

    /// `get_date( 'U' )`.
    pub(crate) fn date(&self) -> Option<i64> {
        let sources: [(&[u8], &[u8]); 8] = [
            (ATOM_10, b"published"),
            (RSS_20, b"pubDate"),
            (DC_11, b"date"),
            (DC_10, b"date"),
            (ATOM_10, b"updated"),
            (ATOM_03, b"issued"),
            (ATOM_03, b"created"),
            (ATOM_03, b"modified"),
        ];
        let raw = sources
            .iter()
            .find_map(|(ns, tag)| self.tags(ns, tag).map(data))?;
        super::dates::parse(&raw).filter(|&date| date != 0)
    }

    /// `get_author()`: the first of the item's authors, else the feed's.
    pub(crate) fn author(&self) -> Option<Author> {
        let mut authors = atom_authors(tags(self.element, ATOM_10, b"author"), ATOM_10, b"uri");
        authors.extend(atom_authors(
            self.tags(ATOM_03, b"author").map(single).as_ref(),
            ATOM_03,
            b"url",
        ));
        if let Some(author) = self.tags(RSS_20, b"author") {
            authors.push(Author {
                name: None,
                link: None,
                email: Some(sanitize(&data(author), TEXT, b"")),
            });
        }
        authors.extend(names_of(tags(self.element, DC_11, b"creator")));
        authors.extend(names_of(tags(self.element, DC_10, b"creator")));
        authors.extend(names_of(tags(self.element, ITUNES, b"author")));
        let authors = match authors.is_empty() {
            true => self.feed.authors(),
            false => unique(authors),
        };
        authors.into_iter().next()
    }
}

/// `Misc::atom_10_construct_type()` and its Atom 0.3 and content kin.
fn construct_type(element: &Value, ns: &[u8], content: bool) -> u8 {
    let attribute = |name: &[u8]| {
        attribute(element, name).map(|v| php_trim(&v.to_php_string()).to_ascii_lowercase())
    };
    if ns == ATOM_03 {
        let kind = attribute(b"type").unwrap_or_else(|| b"text/plain".to_vec());
        return match kind.as_slice() {
            b"text/plain" => TEXT,
            b"text/html" => HTML,
            b"application/xhtml+xml" => XHTML_CONSTRUCT,
            _ => 0,
        };
    }
    match attribute(b"type").as_deref() {
        None | Some(b"text") => TEXT,
        Some(b"html") => HTML,
        Some(b"xhtml") => XHTML_CONSTRUCT,
        _ if content => 0,
        _ => 0,
    }
}

/// `WP_SimplePie_Sanitize_KSES::sanitize()`: HTML through
/// `wp_kses_post()`, text and IRIs escaped, IRIs made absolute first.
fn sanitize(data: &[u8], kind: u8, base: &[u8]) -> Vec<u8> {
    let data = php_trim(data);
    let mut kind = kind;
    if kind & MAYBE_HTML != 0 {
        kind |= if looks_like_html(data) { HTML } else { TEXT };
    }
    if kind & (HTML | XHTML_CONSTRUCT) != 0 {
        return wp_kses_post(data, b"");
    }
    if data.is_empty() && kind & IRI == 0 {
        return Vec::new();
    }
    let data = match kind & IRI != 0 {
        true => absolutize(data, base),
        false => data.to_vec(),
    };
    match kind & (TEXT | IRI) != 0 {
        true => specialchars(&data, false),
        false => data,
    }
}

/// SimplePie's test for HTML in a "maybe" construct: an entity or a
/// character reference, or an end tag.
fn looks_like_html(data: &[u8]) -> bool {
    data.iter().enumerate().any(|(at, &b)| {
        let rest = data.get(at + 1..).unwrap_or_default();
        match b {
            b'&' => match rest.first() {
                Some(b'#') => {
                    rest.get(1).is_some_and(|c| c.is_ascii_digit())
                        || (rest.get(1) == Some(&b'x')
                            && rest.get(2).is_some_and(u8::is_ascii_hexdigit))
                }
                Some(c) => c.is_ascii_alphanumeric(),
                None => false,
            },
            b'<' => {
                rest.first() == Some(&b'/')
                    && rest.get(1).is_some_and(u8::is_ascii_alphabetic)
                    && rest.get(2..).unwrap_or_default().contains(&b'>')
            }
            _ => false,
        }
    })
}

/// `Misc::absolutize_url( $relative, $base )`: RFC 3986's resolution,
/// `relative` as is when it has a scheme or there is no base.
pub(crate) fn absolutize(relative: &[u8], base: &[u8]) -> Vec<u8> {
    let scheme = |url: &[u8]| {
        let colon = url.iter().position(|&b| b == b':')?;
        let name = url.get(..colon)?;
        (name.first().is_some_and(u8::is_ascii_alphabetic)
            && name
                .iter()
                .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'+' | b'-' | b'.')))
        .then_some(colon + 1)
    };
    let Some(after_scheme) = scheme(base).filter(|_| scheme(relative).is_none()) else {
        return relative.to_vec();
    };
    // The base without its fragment, and where its query starts.
    let base = base.split(|&b| b == b'#').next().unwrap_or_default();
    let query = base.iter().position(|&b| b == b'?').unwrap_or(base.len());
    let rest = base.get(after_scheme..query).unwrap_or_default();
    let authority = match rest.strip_prefix(b"//") {
        Some(host) => 2 + host.iter().position(|&b| b == b'/').unwrap_or(host.len()),
        None => 0,
    };
    let origin = base.get(..after_scheme + authority).unwrap_or_default();
    let path = rest.get(authority..).unwrap_or_default();
    match relative.first() {
        None => base.to_vec(),
        Some(b'#') => [base, relative].concat(),
        Some(b'?') => [base.get(..query).unwrap_or_default(), relative].concat(),
        Some(b'/') if relative.starts_with(b"//") => {
            [base.get(..after_scheme).unwrap_or_default(), relative].concat()
        }
        _ => {
            let target = match relative.first() {
                Some(b'/') => relative.to_vec(),
                _ => {
                    let slash = path.iter().rposition(|&b| b == b'/');
                    let directory: &[u8] = match slash {
                        Some(slash) => path.get(..=slash).unwrap_or_default(),
                        None if authority > 0 => b"/",
                        None => b"",
                    };
                    [directory, relative].concat()
                }
            };
            let end = target
                .iter()
                .position(|&b| b == b'?' || b == b'#')
                .unwrap_or(target.len());
            let (path, tail) = target.split_at(end);
            [origin, &remove_dot_segments(path), tail].concat()
        }
    }
}

/// RFC 3986's `remove_dot_segments`.
fn remove_dot_segments(path: &[u8]) -> Vec<u8> {
    let mut output: Vec<&[u8]> = Vec::new();
    let segments: Vec<&[u8]> = path.split(|&b| b == b'/').collect();
    let last = segments.len().saturating_sub(1);
    for (index, segment) in segments.iter().enumerate() {
        match *segment {
            b"." if index == last => output.push(b""),
            b"." => {}
            b".." => {
                if output.len() > 1 {
                    output.pop();
                }
                if index == last {
                    output.push(b"");
                }
            }
            other => output.push(other),
        }
    }
    output.join(&b'/')
}

#[cfg(test)]
mod tests;
