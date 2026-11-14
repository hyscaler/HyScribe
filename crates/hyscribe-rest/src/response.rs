// SPDX-License-Identifier: AGPL-3.0-or-later

//! `WP_REST_Response` and `WP_Error` as the server sends them: errors as
//! `rest_convert_error_to_response()` shapes them, links compacted with
//! their curies, and the pagination headers collections send.

use hyscribe_phpser::{Key, Value, add_query_arg, add_query_values, urlencode_deep};

/// A `WP_Error` with one code, its message and data.
#[derive(Clone, Debug, PartialEq)]
pub struct RestError {
    /// The error code.
    pub code: &'static str,
    /// The message.
    pub message: String,
    /// The data, `Null` for none.
    pub data: Value,
}

/// The code of [`RestError::not_ported`].
pub(crate) const NOT_PORTED: &str = "hyscribe_not_ported";

/// A PHP array of string keys.
pub(crate) fn object<const N: usize>(entries: [(&str, Value); N]) -> Value {
    Value::Array(
        entries
            .into_iter()
            .map(|(key, value)| (Key::String(key.as_bytes().to_vec()), value))
            .collect(),
    )
}

/// A PHP string.
pub(crate) fn text(text: impl Into<Vec<u8>>) -> Value {
    Value::String(text.into())
}

impl RestError {
    /// An error with `array( 'status' => $status )` as its data.
    pub fn status(code: &'static str, message: &str, status: u16) -> RestError {
        RestError {
            code,
            message: message.into(),
            data: object([("status", Value::Int(i64::from(status)))]),
        }
    }

    /// What a route answers where it is not ported, which is sent as `501`.
    pub(crate) fn not_ported() -> RestError {
        RestError {
            code: NOT_PORTED,
            message: String::new(),
            data: Value::Null,
        }
    }

    /// `rest_convert_error_to_response()`: the code, message and data, sent
    /// with the data's status, else 500.
    pub(crate) fn into_response(self) -> Response {
        let status = self
            .data
            .get_str("status")
            .filter(|status| status.is_numeric())
            .map_or(500, |status| u16::try_from(status.to_int()).unwrap_or(500));
        let data = object([
            ("code", text(self.code)),
            ("message", text(self.message)),
            ("data", self.data),
        ]);
        Response::new(data, status)
    }
}

/// A response: its status, headers, data and links.
#[derive(Clone, Debug, PartialEq)]
pub struct Response {
    /// The status code.
    pub status: u16,
    /// The headers, each name once.
    pub headers: Vec<(String, String)>,
    /// The data.
    pub data: Value,
    /// The links and their relations.
    links: Vec<(String, Value)>,
}

impl Response {
    /// `new WP_REST_Response( $data, $status )`.
    pub fn new(data: Value, status: u16) -> Response {
        Response {
            status,
            headers: Vec::new(),
            data,
            links: Vec::new(),
        }
    }

    /// `header( $key, $value )`.
    pub fn header(&mut self, key: &str, value: &str) {
        self.headers.retain(|(name, _)| name != key);
        self.headers.push((key.into(), value.into()));
    }

    /// `add_link( $rel, $href )`.
    pub fn add_link(&mut self, rel: &str, href: String) {
        self.add_link_attributes(rel, object([("href", text(href))]));
    }

    /// A link to `rel` as `get_response_links()` gives it.
    pub(crate) fn add_link_attributes(&mut self, rel: &str, link: Value) {
        self.links.push((rel.into(), link));
    }

    /// `is_error()`.
    pub(crate) fn is_error(&self) -> bool {
        self.status >= 400
    }

    /// `get_compact_response_links()`: each `https://api.w.org/` relation
    /// as `wp:` and the curie after them.
    fn compact_links(&self) -> Option<Value> {
        const CURIE: &str = "https://api.w.org/";
        if self.links.is_empty() {
            return None;
        }
        // Each relation once, in the order it first appears, with its links.
        let rels = self
            .links
            .iter()
            .enumerate()
            .filter(|(i, (rel, _))| {
                self.links.iter().position(|(known, _)| known == rel) == Some(*i)
            })
            .map(|(_, (rel, _))| {
                let items = self.links.iter().filter(|(known, _)| known == rel);
                (rel, list(items.map(|(_, link)| link.clone()).collect()))
            });
        let entry = |rel: String, items| (Key::String(rel.into_bytes()), items);
        let (curied, plain): (Vec<_>, Vec<_>) =
            rels.partition(|(rel, _)| rel.strip_prefix(CURIE).is_some_and(|rest| !rest.is_empty()));
        let mut links: Vec<(Key, Value)> = plain
            .into_iter()
            .map(|(rel, items)| entry(rel.clone(), items))
            .collect();
        for (rel, items) in &curied {
            let short = rel.get(CURIE.len()..).unwrap_or_default();
            links.push(entry(format!("wp:{short}"), items.clone()));
        }
        links.extend((!curied.is_empty()).then(|| {
            let curie = object([
                ("name", text("wp")),
                ("href", text("https://api.w.org/{rel}")),
                ("templated", Value::Bool(true)),
            ]);
            (Key::String(b"curies".to_vec()), list(vec![curie]))
        }));
        Some(Value::Array(links))
    }

    /// The data with its links under `_links`, as
    /// `prepare_response_for_collection()` and `response_to_data()` give it.
    pub fn into_data(self) -> Value {
        let links = self.compact_links();
        let mut data = self.data;
        if let (Some(links), Value::Array(entries)) = (links, &mut data) {
            entries.push((Key::String(b"_links".to_vec()), links));
        }
        data
    }
}

/// A PHP list of `items`.
pub(crate) fn list(items: Vec<Value>) -> Value {
    Value::Array((0..).map(Key::Int).zip(items).collect())
}

/// The headers a page of a collection sends: its total, its pages, and
/// links to the pages either side, made from the request's query and the
/// collection's URL.
pub fn paginate(
    response: &mut Response,
    (total, pages, page): (u64, u64, u64),
    query: &[(Key, Value)],
    collection: &str,
) {
    response.header("X-WP-Total", &total.to_string());
    response.header("X-WP-TotalPages", &pages.to_string());
    let query = urlencode_deep(Value::Array(query.to_vec()));
    let base = add_query_values(query.as_array().unwrap_or_default(), collection.as_bytes());
    let links: Vec<String> = [
        ("prev", page.saturating_sub(1), page > 1),
        ("next", page + 1, page < pages),
    ]
    .into_iter()
    .filter(|(_, _, shown)| *shown)
    .map(|(rel, to, _)| {
        let to = to.to_string();
        let link = add_query_arg(&[(b"page", Some(to.as_bytes()))], &base);
        format!("<{}>; rel=\"{rel}\"", String::from_utf8_lossy(&link))
    })
    .collect();
    // `link_header()` joins a response's links in one header.
    if !links.is_empty() {
        response.header("Link", &links.join(", "));
    }
}
