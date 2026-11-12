// SPDX-License-Identifier: AGPL-3.0-or-later

//! `WP_HTML_Processor` for `<body>` fragments: each token's breadcrumbs and
//! the closers HTML implies, bailing where WordPress bails.

use std::cell::{Cell, RefCell};
use std::collections::VecDeque;
use std::rc::Rc;

use crate::tag_processor::{Namespace, TagProcessor};

mod stacks;

use stacks::{ActiveFormatting, OpenElements};

/// `WP_HTML_Processor::MAX_BOOKMARKS`.
const MAX_BOOKMARKS: usize = 10_000;

/// The bookmarks of tokens that died, waiting to be released.
type Released = Rc<RefCell<Vec<Vec<u8>>>>;

/// `WP_HTML_Token`: an element or other node on the parser's stacks.
struct Token {
    bookmark: Option<Vec<u8>>,
    node_name: String,
    self_closing: bool,
    namespace: Cell<Namespace>,
    /// `integration_node_type`: `html` or `math`.
    integration: Cell<Option<Namespace>>,
    /// Where to release the bookmark when the token dies: PHP's
    /// `on_destroy`.
    release: Option<Released>,
}

impl Drop for Token {
    fn drop(&mut self) {
        if let (Some(release), Some(bookmark)) = (&self.release, self.bookmark.take()) {
            release.borrow_mut().push(bookmark);
        }
    }
}

impl Token {
    fn new(bookmark: Option<Vec<u8>>, node_name: &str, self_closing: bool) -> Token {
        Token {
            bookmark,
            node_name: node_name.to_string(),
            self_closing,
            namespace: Cell::new(Namespace::Html),
            integration: Cell::new(None),
            release: None,
        }
    }

    /// The namespace content inside it is parsed in: HTML at an
    /// integration point.
    fn parsing_namespace(&self) -> Namespace {
        if self.integration.get().is_some() {
            Namespace::Html
        } else {
            self.namespace.get()
        }
    }

    /// The name with its namespace outside HTML: `svg DESC`.
    fn namespaced_name(&self) -> String {
        match self.namespace.get() {
            Namespace::Html => self.node_name.clone(),
            Namespace::Svg => format!("svg {}", self.node_name),
            Namespace::Math => format!("math {}", self.node_name),
        }
    }
}

/// The insertion modes a `<body>` fragment can reach before WordPress
/// bails: no head, frameset or after-body modes.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Mode {
    Body,
    Table,
    Caption,
    ColumnGroup,
    TableBody,
    Row,
    Cell,
    Template,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Operation {
    Push,
    Pop,
}

/// `WP_HTML_Stack_Event`.
struct StackEvent {
    token: Rc<Token>,
    operation: Operation,
    /// `real`, or `virtual` for an element HTML implies.
    real: bool,
}

/// Why processing stopped: `get_last_error()`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ProcessorError {
    /// `ERROR_UNSUPPORTED`, with the message WordPress gives.
    Unsupported(String),
    /// `ERROR_EXCEEDED_MAX_BOOKMARKS`.
    ExceededMaxBookmarks,
}

/// Processing stops: an unsupported construct, or too many bookmarks.
struct Bail;

type Step = Result<bool, Bail>;

/// A port of WordPress's `WP_HTML_Processor`.
pub struct HtmlProcessor {
    tags: TagProcessor,
    open: OpenElements,
    formatting: ActiveFormatting,
    mode: Mode,
    template_modes: Vec<Mode>,
    /// The token being processed; the context node before the first.
    current_token: Rc<Token>,
    context_node: Rc<Token>,
    form_element: Option<Rc<Token>>,
    queue: VecDeque<StackEvent>,
    current_element: Option<StackEvent>,
    breadcrumbs: Vec<String>,
    last_error: Option<ProcessorError>,
    bookmark_counter: u64,
    released: Released,
}

impl HtmlProcessor {
    /// `create_fragment( $html )`: a processor for HTML found inside a
    /// `<body>`, as WordPress sets one up from its full parser's state there.
    pub fn create_fragment(html: &[u8]) -> HtmlProcessor {
        let mut tags = TagProcessor::with_bookmark_limit(html, MAX_BOOKMARKS);
        tags.put_bookmark(b"root-node", 0, 0);
        tags.put_bookmark(b"context-node", 0, 0);
        let context_node = Rc::new(Token::new(Some(b"context-node".to_vec()), "BODY", false));
        let mut open = OpenElements::default();
        // The root's own events would be skipped, so it goes on quietly; the
        // `<body>` context puts the fragment in body.
        open.push(Rc::new(Token::new(
            Some(b"root-node".to_vec()),
            "HTML",
            false,
        )));
        HtmlProcessor {
            tags,
            open,
            formatting: ActiveFormatting::default(),
            mode: Mode::Body,
            template_modes: Vec::new(),
            current_token: Rc::clone(&context_node),
            context_node,
            form_element: None,
            queue: VecDeque::new(),
            current_element: None,
            breadcrumbs: vec!["HTML".into(), "BODY".into()],
            last_error: None,
            bookmark_counter: 0,
            released: Rc::new(RefCell::new(Vec::new())),
        }
    }

    /// `get_last_error()`.
    pub fn get_last_error(&self) -> Option<&ProcessorError> {
        self.last_error.as_ref()
    }

    // The stacks, with the handlers WordPress attaches to them.

    fn push(&mut self, token: Rc<Token>) {
        self.open.push(Rc::clone(&token));
        let real = token.node_name == self.current_token.node_name && !self.tags.is_tag_closer();
        let namespace = token.parsing_namespace();
        self.queue.push_back(StackEvent {
            token,
            operation: Operation::Push,
            real,
        });
        self.tags.change_parsing_namespace(namespace);
    }

    fn popped(&mut self, token: Rc<Token>) {
        let real = token.node_name == self.current_token.node_name && self.tags.is_tag_closer();
        self.queue.push_back(StackEvent {
            token,
            operation: Operation::Pop,
            real,
        });
        let namespace = self.adjusted_current_node().parsing_namespace();
        self.tags.change_parsing_namespace(namespace);
    }

    fn pop(&mut self) -> bool {
        match self.open.pop() {
            Some(token) => {
                self.popped(token);
                true
            }
            None => false,
        }
    }

    /// `pop_until()`: pops through the HTML element `name`, which every
    /// caller knows is open; `ANY_HEADING` stands for any heading.
    fn pop_until(&mut self, name: &str) {
        while let Some(token) = self.open.pop() {
            let html = token.namespace.get() == Namespace::Html;
            let found = html
                && (token.node_name == name
                    || name == stacks::ANY_HEADING && stacks::is_heading(&token.node_name));
            self.popped(token);
            if found {
                break;
            }
        }
    }

    /// Pops through `node`, which every caller knows is open.
    fn pop_through(&mut self, node: &Rc<Token>) {
        while let Some(token) = self.open.pop() {
            let found = Rc::ptr_eq(&token, node);
            self.popped(token);
            if found {
                break;
            }
        }
    }

    fn remove_node(&mut self, token: &Rc<Token>) {
        if let Some(token) = self.open.remove_node(token) {
            self.popped(token);
        }
    }

    // The token loop.

    /// `next_token()`: the next token as the tree builder sees it, real or
    /// implied; `false` at the end or once processing stops.
    pub fn next_token(&mut self) -> bool {
        // Each round either steps the parser, which consumes input, or takes
        // an event off the queue.
        loop {
            self.current_element = None;
            if self.last_error.is_some() {
                return false;
            }
            if self.queue.is_empty() {
                if self.step(Process::Next) {
                    continue;
                }
                if self.last_error.is_some() {
                    return false;
                }
            }
            let Some(event) = self.queue.pop_front() else {
                while self.pop() {}
                if self.queue.is_empty() {
                    return false;
                }
                continue;
            };
            let is_pop = event.operation == Operation::Pop;
            if event.token.bookmark.as_deref() == Some(b"root-node") {
                self.current_element = Some(event);
                continue;
            }
            if is_pop {
                self.breadcrumbs.pop();
            } else {
                self.breadcrumbs.push(event.token.node_name.clone());
            }
            let skip = is_pop && !expects_closer_of(&event.token);
            self.current_element = Some(event);
            if !skip {
                return true;
            }
        }
    }

    /// `step()`, reporting whether a token was processed; a bail records the
    /// error and stops.
    fn step(&mut self, process: Process) -> bool {
        self.try_step(process).unwrap_or_default()
    }

    fn try_step(&mut self, process: Process) -> Step {
        if process != Process::Reprocess {
            // A void element, or another that cannot hold content, closes at
            // once.
            if let Some(top) = self.open.current_node()
                && !expects_closer_of(&top)
            {
                self.pop();
            }
        }
        if process == Process::Next {
            self.tags.next_token();
            if self.tags.get_token_name().as_deref() == Some(b"#text") {
                self.tags.subdivide_text_appropriately();
            }
        }
        if self.tags.get_token_name().is_none() {
            return Ok(false);
        }
        if process != Process::Reprocess {
            let bookmark = self.bookmark_token()?;
            let name = self.token_name();
            let mut token = Token::new(Some(bookmark), &name, self.tags.has_self_closing_flag());
            token.release = Some(Rc::clone(&self.released));
            self.current_token = Rc::new(token);
        }
        if !self.parses_in_current_mode() {
            return self.step_in_foreign_content();
        }
        self.step_in_current_mode()
    }

    /// The next node, or the same one again in another mode.
    fn next(&mut self) -> Step {
        self.try_step(Process::Next)
    }

    fn reprocess(&mut self) -> Step {
        self.try_step(Process::Reprocess)
    }

    /// `bookmark_token()`: names the current token by a counter.
    fn bookmark_token(&mut self) -> Result<Vec<u8>, Bail> {
        for name in self.released.borrow_mut().drain(..) {
            self.tags.release_bookmark(&name);
        }
        self.bookmark_counter += 1;
        let name = self.bookmark_counter.to_string().into_bytes();
        if !self.tags.set_bookmark(&name) {
            self.last_error = Some(ProcessorError::ExceededMaxBookmarks);
            return Err(Bail);
        }
        Ok(name)
    }

    /// `bail()`: stops on markup WordPress does not support.
    fn bail<T>(&mut self, message: &str) -> Result<T, Bail> {
        self.last_error = Some(ProcessorError::Unsupported(message.to_string()));
        Err(Bail)
    }

    /// Whether the token goes to the current insertion mode rather than the
    /// rules for foreign content.
    fn parses_in_current_mode(&self) -> bool {
        let node = self.adjusted_current_node();
        let name = self.tags.get_token_name().unwrap_or_default();
        let start_tag =
            self.tags.get_token_type().as_deref() == Some(b"#tag") && !self.tags.is_tag_closer();
        let text = name == b"#text";
        node.namespace.get() == Namespace::Html
            || node.integration.get() == Some(Namespace::Math)
                && (start_tag && name != b"MGLYPH" && name != b"MALIGNMARK" || text)
            || node.namespace.get() == Namespace::Math
                && node.node_name == "ANNOTATION-XML"
                && start_tag
                && name == b"SVG"
            || node.integration.get() == Some(Namespace::Html) && (start_tag || text)
    }

    /// The parser's token name as the tree builder reads it, through
    /// `get_tag()`: an `IMAGE` tag is an `IMG`.
    fn token_name(&self) -> String {
        match self.tags.get_token_type().as_deref() {
            Some(b"#tag") => self.get_tag().unwrap_or_default(),
            _ => String::from_utf8_lossy(&self.tags.get_token_name().unwrap_or_default())
                .into_owned(),
        }
    }

    /// `get_adjusted_current_node()`, for its namespace and integration
    /// point: the root reads the same as the `<body>` context it stands for.
    fn adjusted_current_node(&self) -> Rc<Token> {
        self.open
            .current_node()
            .unwrap_or_else(|| Rc::clone(&self.context_node))
    }

    fn current(&self) -> Rc<Token> {
        Rc::clone(&self.current_token)
    }

    fn insert_html_element(&mut self, token: Rc<Token>) {
        self.push(token);
    }

    // The accessors, for the current element.

    /// `is_virtual()`: an element HTML implies, not one in the input.
    pub fn is_virtual(&self) -> bool {
        self.current_element
            .as_ref()
            .is_some_and(|event| !event.real)
    }

    /// `is_tag_closer()`: a real element pops exactly when its tag closes.
    pub fn is_tag_closer(&self) -> bool {
        self.current_element.as_ref().map_or_else(
            || self.tags.is_tag_closer(),
            |event| event.operation == Operation::Pop,
        )
    }

    /// `get_token_type()`.
    pub fn get_token_type(&self) -> Option<String> {
        // Only elements are implied.
        if self.is_virtual() {
            return Some("#tag".into());
        }
        self.tags
            .get_token_type()
            .map(|t| String::from_utf8_lossy(&t).into_owned())
    }

    /// `get_token_name()`: a tag's name through `get_tag()`.
    pub fn get_token_name(&self) -> Option<String> {
        if let Some(event) = self.current_element.as_ref().filter(|e| !e.real) {
            return Some(event.token.node_name.clone());
        }
        Some(self.token_name())
    }

    /// `get_tag()`, an implied element's too; an `IMAGE` tag is an `IMG`.
    pub fn get_tag(&self) -> Option<String> {
        if self.last_error.is_some() {
            return None;
        }
        if let Some(event) = self.current_element.as_ref().filter(|e| !e.real) {
            return Some(event.token.node_name.clone());
        }
        let tag = self
            .tags
            .get_tag()
            .map(|t| String::from_utf8_lossy(&t).into_owned());
        let image = tag.as_deref() == Some("IMAGE")
            && self.get_namespace() == Namespace::Html
            && self.tags.get_token_type().as_deref() == Some(b"#tag");
        if image { Some("IMG".into()) } else { tag }
    }

    /// `next_tag()`: the next opener, of `tag_name` in any case if given.
    pub fn next_tag(&mut self, tag_name: Option<&str>) -> bool {
        let wanted = tag_name.map(str::to_ascii_uppercase);
        while self.next_token() {
            let found = self.get_token_type().as_deref() == Some("#tag")
                && wanted
                    .as_deref()
                    .is_none_or(|name| self.get_token_name().as_deref() == Some(name))
                && !self.is_tag_closer();
            if found {
                return true;
            }
        }
        false
    }

    /// `expects_closer()` of the current token.
    pub fn expects_closer(&self) -> Option<bool> {
        self.current_element
            .as_ref()
            .map(|event| expects_closer_of(&event.token))
    }

    /// The start and length `set_bookmark()` would record, if it can.
    pub fn token_span(&self) -> Option<(usize, usize)> {
        if self.is_virtual() {
            return None;
        }
        self.tags.current_span()
    }

    /// `get_namespace()`: the current element's.
    pub fn get_namespace(&self) -> Namespace {
        match &self.current_element {
            Some(event) => event.token.namespace.get(),
            None => self.tags.get_namespace(),
        }
    }

    /// `get_breadcrumbs()`: the names of the open elements down to this one.
    pub fn get_breadcrumbs(&self) -> &[String] {
        &self.breadcrumbs
    }

    /// `get_current_depth()`.
    pub fn get_current_depth(&self) -> usize {
        self.breadcrumbs.len()
    }

    /// `get_modifiable_text()`: none on an implied element.
    pub fn get_modifiable_text(&self) -> Vec<u8> {
        if self.is_virtual() {
            return Vec::new();
        }
        self.tags.get_modifiable_text()
    }
}

/// How `step()` reads its token.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Process {
    /// `PROCESS_NEXT_NODE`.
    Next,
    /// `REPROCESS_CURRENT_NODE`.
    Reprocess,
}

/// `expects_closer()`: a node can hold content and so has a closer.
fn expects_closer_of(token: &Token) -> bool {
    let name = token.node_name.as_str();
    let html = token.namespace.get() == Namespace::Html;
    !(name.starts_with('#')
        || name == "html"
        || html && is_void(name.as_bytes())
        || html
            && matches!(
                name,
                "IFRAME"
                    | "NOEMBED"
                    | "NOFRAMES"
                    | "SCRIPT"
                    | "STYLE"
                    | "TEXTAREA"
                    | "TITLE"
                    | "XMP"
            )
        || !html && token.self_closing)
}

/// `is_void()`: an HTML element without content or a closer.
pub fn is_void(name: &[u8]) -> bool {
    matches!(
        name.to_ascii_uppercase().as_slice(),
        b"AREA"
            | b"BASE"
            | b"BASEFONT"
            | b"BGSOUND"
            | b"BR"
            | b"COL"
            | b"EMBED"
            | b"FRAME"
            | b"HR"
            | b"IMG"
            | b"INPUT"
            | b"KEYGEN"
            | b"LINK"
            | b"META"
            | b"PARAM"
            | b"SOURCE"
            | b"TRACK"
            | b"WBR"
    )
}

mod body;
mod foreign;
mod tables;
