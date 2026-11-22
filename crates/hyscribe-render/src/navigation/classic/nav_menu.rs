// SPDX-License-Identifier: AGPL-3.0-or-later

//! `wp_nav_menu()` with `Walker_Nav_Menu`: a classic menu printed for a
//! theme location or by name, over the items `wp_get_nav_menu_items()` sets
//! up and the classes of the page asked for. Its filters pass objects and
//! cross to no extension yet; `fallback_cb` is `false` or `wp_page_menu`,
//! which is not ported.

use hyscribe_kses::{esc_attr, esc_url};
use hyscribe_model::Term;
use hyscribe_phpser::{Value, is_empty_string, php_trim};

use super::{NotPorted, context, located_menu, menu_items, menu_object};
use crate::permalinks::permalink;
use crate::post::title_filters;
use crate::site::{NavMenuItem, Request, Site};

/// `wp_nav_menu( $args )`'s arguments, as `wp_parse_args()` leaves them.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct NavMenuArgs {
    /// `menu`: an ID, slug or name.
    pub menu: Value,
    /// `container`.
    pub container: Vec<u8>,
    /// `container_class`.
    pub container_class: Vec<u8>,
    /// `container_id`.
    pub container_id: Vec<u8>,
    /// `container_aria_label`.
    pub container_aria_label: Vec<u8>,
    /// `menu_class`.
    pub menu_class: Vec<u8>,
    /// `menu_id`.
    pub menu_id: Vec<u8>,
    /// `fallback_cb`: `wp_page_menu` when true, none when false.
    pub fallback: bool,
    /// `before`.
    pub before: Vec<u8>,
    /// `after`.
    pub after: Vec<u8>,
    /// `link_before`.
    pub link_before: Vec<u8>,
    /// `link_after`.
    pub link_after: Vec<u8>,
    /// `items_wrap`, a `sprintf()` format.
    pub items_wrap: Vec<u8>,
    /// `item_spacing`: `preserve` or `discard`.
    pub item_spacing: Vec<u8>,
    /// `depth`.
    pub depth: i64,
    /// `theme_location`.
    pub theme_location: Vec<u8>,
}

impl Default for NavMenuArgs {
    fn default() -> NavMenuArgs {
        NavMenuArgs {
            menu: Value::String(Vec::new()),
            container: b"div".to_vec(),
            container_class: Vec::new(),
            container_id: Vec::new(),
            container_aria_label: Vec::new(),
            menu_class: b"menu".to_vec(),
            menu_id: Vec::new(),
            fallback: true,
            before: Vec::new(),
            after: Vec::new(),
            link_before: Vec::new(),
            link_after: Vec::new(),
            items_wrap: b"<ul id=\"%1$s\" class=\"%2$s\">%3$s</ul>".to_vec(),
            item_spacing: b"preserve".to_vec(),
            depth: 0,
            theme_location: Vec::new(),
        }
    }
}

/// What `wp_nav_menu()` answers.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum NavMenu {
    /// The menu's markup.
    Printed(Vec<u8>),
    /// `false`: no menu, or no items to print.
    False,
    /// A fallback, an item or a format that is not ported.
    NotPorted,
}

/// What the request printed before, which `wp_nav_menu()`'s static list
/// and `_nav_menu_item_id_use_once()`'s keep from being printed twice.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Printed {
    /// The menus' IDs.
    menus: Vec<Vec<u8>>,
    /// The items whose `id` attribute was printed.
    items: Vec<u64>,
}

/// `wp_nav_menu( $args )` with `echo` false, after what `printed` lists.
pub fn wp_nav_menu(
    site: &dyn Site,
    request: &Request,
    args: &NavMenuArgs,
    printed: &mut Printed,
) -> NavMenu {
    match menu(site, request, args, printed) {
        Ok(Some(markup)) => NavMenu::Printed(markup),
        Ok(None) => NavMenu::False,
        Err(NotPorted) => NavMenu::NotPorted,
    }
}

fn menu(
    site: &dyn Site,
    request: &Request,
    args: &NavMenuArgs,
    printed: &mut Printed,
) -> Result<Option<Vec<u8>>, NotPorted> {
    let all = site.taxonomy_terms("nav_menu");
    let location = String::from_utf8_lossy(&args.theme_location);
    let located = !is_empty_string(&args.theme_location);
    let mut menu = menu_object(&all, &args.menu);
    if menu.is_none() && located {
        menu = located_menu(site, &all, &location);
    }
    let mut items = None;
    if menu.is_none() && !located {
        for candidate in nav_menus(site, all) {
            let found = menu_items(site, &candidate)?.unwrap_or_default();
            let has_items = !found.is_empty();
            items = Some(found);
            if has_items {
                menu = Some(candidate);
                break;
            }
        }
    }
    if let (Some(menu), None) = (&menu, &items) {
        items = Some(menu_items(site, menu)?.unwrap_or_default());
    }
    let empty = items.as_ref().is_some_and(Vec::is_empty);
    if (menu.is_none() || (empty && !located)) && args.fallback {
        return Err(NotPorted);
    }
    let Some(menu) = menu else {
        return Ok(None);
    };
    let mut items = items.unwrap_or_default();
    let spacing = Spacing::of(args);
    let container =
        (args.container == b"div" || args.container == b"nav").then(|| container_tag(args, &menu));
    context::apply(site, request, &mut items);
    let mut walker = Walker {
        args,
        spacing,
        privacy_policy: privacy_policy_url(site),
        used: std::mem::take(&mut printed.items),
    };
    let walked = walker.walk(sorted(items), args.depth);
    printed.items = walker.used;
    let wrap_id = wrap_id(args, &menu, &printed.menus).ok_or(NotPorted)?;
    printed.menus.push(wrap_id.clone());
    if is_empty_string(&walked) {
        return Ok(None);
    }
    let wrap_class = if is_empty_string(&args.menu_class) {
        &[][..]
    } else {
        &args.menu_class
    };
    let wrapped = sprintf(
        &args.items_wrap,
        &[
            esc_attr(&wrap_id).as_bytes(),
            esc_attr(wrap_class).as_bytes(),
            &walked,
        ],
    )
    .ok_or(NotPorted)?;
    Ok(Some(match container {
        Some(open) => [open, wrapped, [b"</", &args.container[..], b">"].concat()].concat(),
        None => wrapped,
    }))
}

/// `wp_get_nav_menus()`: every menu, by name.
fn nav_menus(site: &dyn Site, mut all: Vec<Term>) -> Vec<Term> {
    all.sort_by_key(|menu| menu.name.to_ascii_lowercase());
    let args = [
        ("taxonomy".to_string(), Value::String(b"nav_menu".to_vec())),
        ("hide_empty".to_string(), Value::Bool(false)),
        ("orderby".to_string(), Value::String(b"name".to_vec())),
    ];
    site.hooks().wp_get_nav_menus(all, &args)
}

/// The container's opening tag.
fn container_tag(args: &NavMenuArgs, menu: &Term) -> Vec<u8> {
    let attr = |name: &str, value: &[u8]| format!(" {name}=\"{}\"", esc_attr(value)).into_bytes();
    let id = match args.container_id.as_slice() {
        b"" | b"0" => Vec::new(),
        id => attr("id", id),
    };
    let class = match args.container_class.as_slice() {
        b"" | b"0" => [b" class=\"menu-", &menu.slug[..], b"-container\""].concat(),
        class => attr("class", class),
    };
    let label = &args.container_aria_label;
    let aria = if args.container == b"nav" && !is_empty_string(label) {
        attr("aria-label", label)
    } else {
        Vec::new()
    };
    [&b"<"[..], &args.container, &id, &class, &aria, b">"].concat()
}

/// The items as `wp_nav_menu()` keys them by their menu order, which
/// `wp_get_nav_menu_items()` counted from 1: an item of its own parent is
/// at the top, and one with children gets `menu-item-has-children`.
fn sorted(mut sorted: Vec<NavMenuItem>) -> Vec<NavMenuItem> {
    for item in &mut sorted {
        if item.menu_item_parent == item.id.to_string().as_bytes() {
            item.menu_item_parent = b"0".to_vec();
        }
    }
    let parents: Vec<Vec<u8>> = sorted
        .iter()
        .map(|item| item.menu_item_parent.clone())
        .filter(|parent| !is_empty_string(parent))
        .collect();
    for item in &mut sorted {
        if parents.contains(&item.id.to_string().into_bytes()) {
            item.classes.push(b"menu-item-has-children".to_vec());
        }
    }
    sorted
}

/// The menu's ID: `menu_id`, else `menu-{slug}` made unique among those
/// printed before by a number at its end; `None` past PHP's integers,
/// where it counts in floats.
fn wrap_id(args: &NavMenuArgs, menu: &Term, printed: &[Vec<u8>]) -> Option<Vec<u8>> {
    if !is_empty_string(&args.menu_id) {
        return Some(args.menu_id.clone());
    }
    let mut id = [b"menu-", &menu.slug[..]].concat();
    while printed.contains(&id) {
        id = match number_at_end(&id) {
            Some((stem, number)) => {
                let next = number.parse::<i64>().ok()?.checked_add(1)?;
                [stem, next.to_string().as_bytes()].concat()
            }
            None => [&id[..], b"-1"].concat(),
        };
    }
    Some(id)
}

/// `#-(\d+)$#`: what comes before the digits, and the digits.
fn number_at_end(id: &[u8]) -> Option<(&[u8], &str)> {
    let digits = id.iter().rev().take_while(|b| b.is_ascii_digit()).count();
    let (stem, number) = id.split_at(id.len() - digits);
    let number = std::str::from_utf8(number).ok().filter(|n| !n.is_empty())?;
    stem.ends_with(b"-").then_some((stem, number))
}

/// `sprintf()` of `%s`, `%n$s` and `%%`; `None` for any other conversion
/// or a missing argument, which PHP refuses.
fn sprintf(format: &[u8], args: &[&[u8]]) -> Option<Vec<u8>> {
    let mut out = Vec::new();
    let mut next = 0;
    let mut rest = format;
    loop {
        let mut parts = rest.splitn(2, |b| *b == b'%');
        out.extend(parts.next().unwrap_or_default());
        let Some(after) = parts.next() else {
            return Some(out);
        };
        rest = after;
        if let Some(after) = rest.strip_prefix(b"%") {
            out.push(b'%');
            rest = after;
            continue;
        }
        let digits = rest.iter().take_while(|b| b.is_ascii_digit()).count();
        let (number, after) = rest.split_at(digits);
        let (index, after) = if digits == 0 {
            next += 1;
            (next - 1, after)
        } else {
            let n = number.iter().try_fold(0usize, |n, digit| {
                n.checked_mul(10)?.checked_add(usize::from(digit - b'0'))
            })?;
            (n.checked_sub(1)?, after.strip_prefix(b"$")?)
        };
        rest = after.strip_prefix(b"s")?;
        out.extend_from_slice(args.get(index)?);
    }
}

/// `get_privacy_policy_url()`: the published privacy policy page's link.
fn privacy_policy_url(site: &dyn Site) -> Vec<u8> {
    let option = site
        .option("wp_page_for_privacy_policy")
        .unwrap_or_default();
    let id = u64::try_from(Value::String(option).to_int()).unwrap_or_default();
    let page = site.post(id).filter(|page| page.status == "publish");
    page.map(|page| permalink(site, &page)).unwrap_or_default()
}

/// The tab and line feed `item_spacing` gives the markup.
#[derive(Clone, Copy)]
struct Spacing {
    tab: &'static [u8],
    line: &'static [u8],
}

impl Spacing {
    fn of(args: &NavMenuArgs) -> Spacing {
        match args.item_spacing.as_slice() {
            b"discard" => Spacing {
                tab: b"",
                line: b"",
            },
            _ => Spacing {
                tab: b"\t",
                line: b"\n",
            },
        }
    }

    fn indent(self, depth: usize) -> Vec<u8> {
        self.tab.repeat(depth)
    }
}

/// `Walker_Nav_Menu` over one menu.
struct Walker<'a> {
    args: &'a NavMenuArgs,
    spacing: Spacing,
    privacy_policy: Vec<u8>,
    /// The items whose `id` was printed.
    used: Vec<u64>,
}

/// The items under one parent, keyed by `menu_item_parent` as PHP keys an
/// array: in the order the first of each was met.
type Children = Vec<(Vec<u8>, Vec<NavMenuItem>)>;

impl Walker<'_> {
    /// `Walker::walk( $elements, $max_depth )`.
    fn walk(&mut self, items: Vec<NavMenuItem>, max_depth: i64) -> Vec<u8> {
        let mut out = Vec::new();
        if max_depth < -1 || items.is_empty() {
            return out;
        }
        if max_depth == -1 {
            for item in &items {
                self.element(item, &mut Vec::new(), 1, 0, &mut out);
            }
            return out;
        }
        let top_parent = if items
            .iter()
            .any(|item| is_empty_string(&item.menu_item_parent))
        {
            None
        } else {
            items.first().map(|root| root.menu_item_parent.clone())
        };
        let mut top = Vec::new();
        let mut children: Children = Vec::new();
        for item in items {
            let at_top = match &top_parent {
                None => is_empty_string(&item.menu_item_parent),
                Some(parent) => item.menu_item_parent == *parent,
            };
            if at_top {
                top.push(item);
                continue;
            }
            match children
                .iter_mut()
                .find(|(key, _)| *key == item.menu_item_parent)
            {
                Some((_, under)) => under.push(item),
                None => children.push((item.menu_item_parent.clone(), vec![item])),
            }
        }
        for item in &top {
            self.element(item, &mut children, max_depth, 0, &mut out);
        }
        if max_depth == 0 {
            for (_, orphans) in std::mem::take(&mut children) {
                for orphan in &orphans {
                    self.element(orphan, &mut Vec::new(), 1, 0, &mut out);
                }
            }
        }
        out
    }

    /// `Walker::display_element()`.
    fn element(
        &mut self,
        item: &NavMenuItem,
        children: &mut Children,
        max_depth: i64,
        depth: usize,
        out: &mut Vec<u8>,
    ) {
        self.start_el(item, depth, out);
        let id = item.id.to_string().into_bytes();
        let deeper = max_depth == 0 || max_depth > i64::try_from(depth).unwrap_or(i64::MAX) + 1;
        let at = children.iter().position(|(key, _)| *key == id);
        if let Some(at) = at.filter(|_| deeper) {
            let (_, under) = children.remove(at);
            let indent = self.spacing.indent(depth);
            let line = self.spacing.line;
            out.extend([line, &indent, b"<ul class=\"sub-menu\">", line].concat());
            for child in &under {
                self.element(child, children, max_depth, depth + 1, out);
            }
            out.extend([&indent[..], b"</ul>", line].concat());
        }
        out.extend([&b"</li>"[..], self.spacing.line].concat());
    }

    /// `Walker_Nav_Menu::start_el()`.
    fn start_el(&mut self, item: &NavMenuItem, depth: usize, out: &mut Vec<u8>) {
        let args = self.args;
        let mut classes: Vec<&[u8]> = item.classes.iter().map(Vec::as_slice).collect();
        let own = format!("menu-item-{}", item.id).into_bytes();
        classes.push(&own);
        classes.retain(|class| !is_empty_string(class));
        // `wp_nav_menu_remove_menu_item_has_children_class()`: none at the
        // depth printed last, which is every depth for -1.
        let last = i64::try_from(depth).is_ok_and(|depth| depth + 1 >= args.depth);
        if args.depth != 0 && last {
            classes.retain(|class| *class != b"menu-item-has-children");
        }
        // `_nav_menu_item_id_use_once()`.
        let id = match self.used.contains(&item.id) {
            true => Vec::new(),
            false => own.clone(),
        };
        self.used.push(item.id);
        let li = [("id", id), ("class", classes.join(&b' '))];
        out.extend(self.spacing.indent(depth));
        out.extend([&b"<li"[..], &build_atts(&li), b">"].concat());
        let title = title_filters(&item.title);
        let not_empty = |value: &[u8]| (!is_empty_string(value)).then(|| value.to_vec());
        let mut rel = not_empty(&item.xfn).unwrap_or_default();
        let url = item.url.as_deref().and_then(not_empty);
        if url.as_deref() == Some(&self.privacy_policy[..]) {
            rel = match rel.is_empty() {
                true => b"privacy-policy".to_vec(),
                false => [&rel[..], b" privacy-policy"].concat(),
            };
        }
        let lower = |text: &[u8]| php_trim(&text.to_ascii_lowercase()).to_vec();
        let attr_title = lower(&item.attr_title);
        let distinct = attr_title != lower(&item.title) && attr_title != lower(&title);
        let a = [
            ("target", not_empty(&item.target).unwrap_or_default()),
            ("rel", rel),
            ("href", url.unwrap_or_default()),
            (
                "aria-current",
                if item.current {
                    b"page".to_vec()
                } else {
                    Vec::new()
                },
            ),
            (
                "title",
                not_empty(&item.attr_title)
                    .filter(|_| distinct)
                    .unwrap_or_default(),
            ),
        ];
        out.extend(
            [
                &args.before[..],
                b"<a",
                &build_atts(&a),
                b">",
                &args.link_before,
                &title,
                &args.link_after,
                b"</a>",
                &args.after,
            ]
            .concat(),
        );
    }
}

/// `Walker_Nav_Menu::build_atts()`: the attributes that are not empty,
/// `href` through `esc_url()` and the rest through `esc_attr()`.
fn build_atts(atts: &[(&str, Vec<u8>)]) -> Vec<u8> {
    let mut out = Vec::new();
    for (name, value) in atts.iter().filter(|(_, value)| !value.is_empty()) {
        let value = match *name {
            "href" => esc_url(value),
            _ => esc_attr(value).into_bytes(),
        };
        out.extend([b" ", name.as_bytes(), b"=\"", &value, b"\""].concat());
    }
    out
}
