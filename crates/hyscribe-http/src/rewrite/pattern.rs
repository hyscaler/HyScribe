// SPDX-License-Identifier: AGPL-3.0-or-later

//! The regexes rewrite rules are written in, matched as `preg_match()`
//! matches `#^$regex#` against bytes: literals, `.`, `\d`, classes, groups
//! with `|`, greedy and lazy quantifiers and the `^` and `$` anchors.
//! Matching backtracks in PCRE's order, so captures come out as in PHP.

/// PCRE2's limit on nested parentheses, past which it refuses a pattern.
const MAX_NESTING: usize = 250;

/// PCRE's largest `{}` count, past which it refuses a pattern.
const MAX_COUNT: usize = 65535;

/// How deep matching nests, one level for each quantified item or group
/// repetition still open, before it gives up as PCRE gives up past its JIT
/// stack. Rewrite rules nest a few dozen levels.
const MAX_DEPTH: usize = 1000;

/// How many steps matching takes before giving up, as PCRE gives up past
/// `pcre.backtrack_limit`.
const MAX_STEPS: usize = 1_000_000;

/// A parsed regex.
#[derive(Debug)]
pub struct Pattern {
    items: Vec<Item>,
    groups: usize,
}

#[derive(Debug)]
struct Item {
    atom: Atom,
    min: usize,
    max: Option<usize>,
    lazy: bool,
}

#[derive(Debug)]
enum Atom {
    /// One byte from the set.
    Byte(Box<[bool; 256]>),
    Node(Node),
}

#[derive(Debug)]
enum Node {
    Start,
    /// The end, or before a final newline.
    End,
    /// Alternatives, capturing into the numbered group or not.
    Group(Option<usize>, Vec<Vec<Item>>),
}

/// A group's start and end, where it took part in the match.
pub type Captures = Vec<Option<(usize, usize)>>;

impl Pattern {
    /// `None` for syntax outside what rewrite rules use.
    pub fn new(regex: &str) -> Option<Pattern> {
        let mut parser = Parser {
            bytes: regex.as_bytes(),
            at: 0,
            groups: 0,
            nesting: 0,
        };
        let alternatives = parser.alternatives()?;
        if parser.at != regex.len() {
            return None;
        }
        // The whole match is group 0.
        Some(Pattern {
            items: vec![Item::once(Atom::Node(Node::Group(Some(0), alternatives)))],
            groups: parser.groups,
        })
    }

    /// The captures of a match at the start of `input`, the whole match
    /// first.
    pub fn captures(&self, input: &[u8]) -> Option<Captures> {
        let mut run = Run {
            input,
            captures: vec![None; self.groups + 1],
            depth: 0,
            steps: 0,
        };
        run.sequence(&self.items, 0, &mut |_, _| true)
            .then_some(run.captures)
    }
}

impl Item {
    fn once(atom: Atom) -> Item {
        Item {
            atom,
            min: 1,
            max: Some(1),
            lazy: false,
        }
    }
}

struct Parser<'a> {
    bytes: &'a [u8],
    at: usize,
    groups: usize,
    /// Groups open around the one being read.
    nesting: usize,
}

impl Parser<'_> {
    fn peek(&self) -> Option<u8> {
        self.bytes.get(self.at).copied()
    }

    fn eat(&mut self, byte: u8) -> bool {
        let found = self.peek() == Some(byte);
        self.at += usize::from(found);
        found
    }

    /// Up to a `)` or the end.
    fn alternatives(&mut self) -> Option<Vec<Vec<Item>>> {
        let (mut alternatives, mut current) = (Vec::new(), Vec::new());
        while let Some(byte) = self.peek().filter(|b| *b != b')') {
            self.at += 1;
            let atom = match byte {
                b'|' => {
                    alternatives.push(std::mem::take(&mut current));
                    continue;
                }
                b'(' => {
                    let index = if self.eat(b'?') {
                        self.eat(b':').then_some(None)?
                    } else {
                        self.groups += 1;
                        Some(self.groups)
                    };
                    if self.nesting >= MAX_NESTING {
                        return None;
                    }
                    self.nesting += 1;
                    let inner = self.alternatives()?;
                    self.nesting -= 1;
                    self.eat(b')')
                        .then_some(Atom::Node(Node::Group(index, inner)))?
                }
                b'[' => self.class()?,
                b'.' => Atom::Byte(Box::new(std::array::from_fn(|b| b != usize::from(b'\n')))),
                b'^' => Atom::Node(Node::Start),
                b'$' => Atom::Node(Node::End),
                b'\\' => Atom::Byte(Box::new(self.escape()?)),
                b'?' | b'*' | b'+' | b'{' => return None,
                literal => Atom::Byte(Box::new(std::array::from_fn(|b| b == usize::from(literal)))),
            };
            current.push(self.quantified(atom)?);
        }
        alternatives.push(current);
        Some(alternatives)
    }

    fn escape(&mut self) -> Option<[bool; 256]> {
        let byte = self.peek()?;
        self.at += 1;
        Some(match byte {
            b'd' => std::array::from_fn(|b| (usize::from(b'0')..=usize::from(b'9')).contains(&b)),
            b if !b.is_ascii_alphanumeric() => std::array::from_fn(|c| c == usize::from(b)),
            _ => return None,
        })
    }

    fn class(&mut self) -> Option<Atom> {
        let negated = self.eat(b'^');
        let mut set = [false; 256];
        loop {
            let byte = self.peek()?;
            self.at += 1;
            match byte {
                b']' => break,
                b'\\' => {
                    for (s, e) in set.iter_mut().zip(self.escape()?) {
                        *s |= e;
                    }
                }
                _ => {
                    // A range, or the byte alone; a '-' before ']' is literal.
                    let end = match (self.peek(), self.bytes.get(self.at + 1)) {
                        (Some(b'-'), Some(&end)) if end != b']' => {
                            self.at += 2;
                            end
                        }
                        _ => byte,
                    };
                    let count = usize::from(end.saturating_sub(byte)) + 1;
                    for s in set.iter_mut().skip(usize::from(byte)).take(count) {
                        *s = true;
                    }
                }
            }
        }
        if negated {
            for s in &mut set {
                *s = !*s;
            }
        }
        Some(Atom::Byte(Box::new(set)))
    }

    fn quantified(&mut self, atom: Atom) -> Option<Item> {
        let (min, max) = match self.peek() {
            Some(b'?') => (0, Some(1)),
            Some(b'*') => (0, None),
            Some(b'+') => (1, None),
            Some(b'{') => {
                let close = self.bytes.iter().skip(self.at).position(|b| *b == b'}')? + self.at;
                let inside = self.bytes.get(self.at + 1..close).unwrap_or_default();
                let inside = String::from_utf8_lossy(inside);
                let inside = inside.as_ref();
                let number = |s: &str| s.parse::<usize>().ok().filter(|n| *n <= MAX_COUNT);
                let bounds = match inside.split_once(',') {
                    None => (number(inside)?, number(inside)),
                    Some((low, "")) => (number(low)?, None),
                    Some((low, high)) => (number(low)?, Some(number(high)?)),
                };
                self.at = close;
                bounds
            }
            _ => return Some(Item::once(atom)),
        };
        // A group repeats at most a set number of times, so matching nests
        // only as deep as the pattern; rewrite rules use `?` alone.
        if max.is_none() && matches!(atom, Atom::Node(_)) {
            return None;
        }
        self.at += 1;
        let lazy = self.eat(b'?');
        Some(Item {
            atom,
            min,
            max,
            lazy,
        })
    }
}

/// Called with where the match so far ends; true ends the search.
type Next<'n, 'a> = &'n mut dyn FnMut(&mut Run<'a>, usize) -> bool;

struct Run<'a> {
    input: &'a [u8],
    captures: Captures,
    depth: usize,
    steps: usize,
}

impl<'a> Run<'a> {
    /// `step` one level deeper, or no match past the limits.
    fn deeper(&mut self, step: impl FnOnce(&mut Self) -> bool) -> bool {
        self.steps += 1;
        if self.depth >= MAX_DEPTH || self.steps > MAX_STEPS {
            return false;
        }
        self.depth += 1;
        let found = step(self);
        self.depth -= 1;
        found
    }

    fn sequence(&mut self, mut items: &[Item], mut at: usize, next: Next<'_, 'a>) -> bool {
        // A byte that occurs once leaves no choice to come back to, so a
        // run of literals matches in a loop.
        while let Some((
            Item {
                atom: Atom::Byte(set),
                min: 1,
                max: Some(1),
                ..
            },
            rest,
        )) = items.split_first()
        {
            let byte = self.input.get(at).map(|b| usize::from(*b));
            if !byte.and_then(|b| set.get(b)).copied().unwrap_or_default() {
                return false;
            }
            (items, at) = (rest, at + 1);
        }
        match items.split_first() {
            None => next(self, at),
            Some((item, rest)) => {
                self.deeper(|run| run.item(item, at, &mut |run, at| run.sequence(rest, at, next)))
            }
        }
    }

    fn item(&mut self, item: &Item, at: usize, next: Next<'_, 'a>) -> bool {
        let set = match &item.atom {
            Atom::Byte(set) => set,
            Atom::Node(node) => return self.repeat(node, item, 0, at, next),
        };
        // Single bytes repeat in a loop, so long inputs never nest deep.
        let available = self
            .input
            .get(at..)
            .unwrap_or_default()
            .iter()
            .take(item.max.unwrap_or(usize::MAX))
            .take_while(|b| set.get(usize::from(**b)).copied().unwrap_or_default())
            .count();
        if available < item.min {
            return false;
        }
        let mut counts = item.min..=available;
        loop {
            let count = if item.lazy {
                counts.next()
            } else {
                counts.next_back()
            };
            match count {
                Some(count) if next(self, at + count) => return true,
                Some(_) => {}
                None => return false,
            }
        }
    }

    fn repeat(
        &mut self,
        node: &Node,
        item: &Item,
        count: usize,
        at: usize,
        next: Next<'_, 'a>,
    ) -> bool {
        // Lazy stops as soon as it can; greedy goes once more first.
        if item.lazy && count >= item.min && next(self, at) {
            return true;
        }
        self.again(node, item, count, at, next)
            || (!item.lazy && count >= item.min && next(self, at))
    }

    /// One more time through the item, unless it is at its most, which
    /// `quantified()` keeps finite for groups.
    fn again(
        &mut self,
        node: &Node,
        item: &Item,
        count: usize,
        at: usize,
        next: Next<'_, 'a>,
    ) -> bool {
        if item.max.is_some_and(|max| count >= max) {
            return false;
        }
        self.deeper(|run| {
            run.once(node, at, &mut |run, end| {
                run.repeat(node, item, count + 1, end, next)
            })
        })
    }

    fn once(&mut self, node: &Node, at: usize, next: Next<'_, 'a>) -> bool {
        match node {
            Node::Start => at == 0 && next(self, at),
            Node::End => {
                let len = self.input.len();
                (at == len || (at + 1 == len && self.input.last() == Some(&b'\n')))
                    && next(self, at)
            }
            Node::Group(index, alternatives) => alternatives.iter().any(|alternative| {
                self.sequence(alternative, at, &mut |run, end| {
                    let Some(i) = *index else {
                        return next(run, end);
                    };
                    let slot = |run: &mut Run, value| {
                        let slot = run.captures.iter_mut().skip(i).take(1);
                        slot.fold(None, |_, slot| std::mem::replace(slot, value))
                    };
                    let old = slot(run, Some((at, end)));
                    next(run, end) || {
                        slot(run, old);
                        false
                    }
                })
            }),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn groups(regex: &str, input: &str) -> Option<Vec<Option<&'static str>>> {
        let input: &'static str = Box::leak(input.to_string().into_boxed_str());
        let captures = Pattern::new(regex).unwrap().captures(input.as_bytes())?;
        Some(
            captures
                .into_iter()
                .map(|c| c.map(|(start, end)| input.get(start..end).unwrap()))
                .collect(),
        )
    }

    #[test]
    fn syntax_outside_rewrite_rules_is_refused() {
        for regex in [
            "(?=x)", "a)", "*", "a|*", "[ab", "\\w", "\\", "[\\w]", "a{2", "a{x}", "a{1,x}", "(a",
            "((?=x))", "a{x,}", "a{x,2}", "(?:ab)*", "(a)+", "(a){2,}",
        ] {
            assert!(Pattern::new(regex).is_none(), "{regex}");
        }
    }

    #[test]
    fn matches_as_pcre() {
        assert_eq!(
            groups("(a|ab)(c|bcd)(d*)", "abcd"),
            Some(vec![Some("abcd"), Some("a"), Some("bcd"), Some("")])
        );
        assert_eq!(
            groups("x(?:/([0-9]+))?/?$", "x/"),
            Some(vec![Some("x/"), None])
        );
        assert_eq!(groups("(.+?)/", "a/b/"), Some(vec![Some("a/"), Some("a")]));
        assert_eq!(
            groups("(.+)/", "a/b/"),
            Some(vec![Some("a/b/"), Some("a/b")])
        );
        assert_eq!(
            groups("(?:ab)??(ab)", "ab"),
            Some(vec![Some("ab"), Some("ab")])
        );
        assert_eq!(groups("(?:b|)?a", "a"), Some(vec![Some("a")]));
        assert_eq!(
            groups("a{2}b{1,}c{1,2}", "aabbcc"),
            Some(vec![Some("aabbcc")])
        );
        assert_eq!(groups("^a$", "a\n"), Some(vec![Some("a")]));
        assert_eq!(groups("a.", "a\n"), None);
        assert_eq!(groups("[^/]+", "/a"), None);
        assert_eq!(groups("[a-z\\d_-]+", "x_9-Y"), Some(vec![Some("x_9-")]));
        assert_eq!(groups("[a-]+", "-a-b"), Some(vec![Some("-a-")]));
        assert_eq!(groups("x^", "x"), None);
        assert_eq!(groups("b", "ab"), None);
    }

    #[test]
    fn what_pcre_refuses_to_compile_is_refused() {
        assert!(Pattern::new("a{65535}").is_some());
        assert!(Pattern::new("^{65536}").is_none());
        assert!(Pattern::new("a{1,65536}").is_none());
        let nested = |n| format!("{}a{}", "(".repeat(n), ")".repeat(n));
        assert!(Pattern::new(&nested(250)).is_some());
        assert!(Pattern::new(&nested(251)).is_none());
        // Groups side by side do not nest.
        assert!(Pattern::new(&"(a)".repeat(251)).is_some());
    }

    /// `x?.*?(?:b)` takes two steps for each byte before the `b` and six
    /// more, and every step but the last returns to the top.
    #[test]
    fn matching_stops_at_the_step_limit() {
        let pattern = Pattern::new("x?.*?(?:b)").unwrap();
        let input = |before| format!("{}b", "a".repeat(before));
        let last = (MAX_STEPS - 6) / 2;
        assert!(pattern.captures(input(last).as_bytes()).is_some());
        assert_eq!(pattern.captures(input(last + 1).as_bytes()), None);
    }

    /// PCRE gives up past its limits, and `preg_match()` returns false.
    #[test]
    fn matching_gives_up_past_the_limits() {
        let run = || {
            let deep = Pattern::new("(?:(?:^){65535}){65535}").unwrap();
            assert_eq!(deep.captures(b""), None);
            let literal = Pattern::new(&"a".repeat(20_000)).unwrap();
            assert!(literal.captures("a".repeat(20_000).as_bytes()).is_some());
            assert_eq!(literal.captures("a".repeat(19_999).as_bytes()), None);
            let long = Pattern::new(&"a?".repeat(20_000)).unwrap();
            assert_eq!(long.captures("a".repeat(20_000).as_bytes()), None);
            let nested = format!("{}a{}", "(?:".repeat(250), ")".repeat(250));
            assert!(Pattern::new(&nested).unwrap().captures(b"a").is_some());
            let backtracking =
                Pattern::new(&format!("{}{}", "a?".repeat(40), "a".repeat(40))).unwrap();
            assert_eq!(backtracking.captures("a".repeat(40).as_bytes()), None);
        };
        // A Tokio worker's stack.
        let worker = std::thread::Builder::new().stack_size(2 << 20).spawn(run);
        worker.unwrap().join().unwrap();
    }
}
