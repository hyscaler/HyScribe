// SPDX-License-Identifier: AGPL-3.0-or-later

//! `wp_nav_menu()` against `reference/nav-menu/golden.sh`: the menus its
//! golden lists, as the database held them, printed for each case.

use hyscribe_phpser::{encode, json_decode};

use super::*;
use crate::navigation::classic::{NavMenu, NavMenuArgs, Printed, wp_nav_menu};

const GOLDEN: &str = include_str!("../../../../../tests/golden/nav-menu.tsv");

fn json(text: &str) -> Value {
    json_decode(text.as_bytes()).unwrap()
}

fn text(value: &Value) -> String {
    String::from_utf8(value.to_php_string()).unwrap()
}

/// The default install with the golden's menus, items and options.
fn golden_site() -> Menus {
    let mut site = site("Unused");
    site.terms.clear();
    site.options.clear();
    site.post_mut(3).status = "publish".into();
    for line in GOLDEN.lines() {
        let fields: Vec<&str> = line.split('\t').collect();
        match fields[0] {
            "menu" => site.menu(
                fields[1].parse().unwrap(),
                &text(&json(fields[2])),
                &text(&json(fields[3])),
            ),
            "item" => {
                let menu: u64 = fields[1].parse().unwrap();
                let item = json(fields[2]);
                let field = |name: &str| item.get_str(name).unwrap();
                let post = Post {
                    id: field("ID").to_int() as u64,
                    title: field("post_title").to_php_string(),
                    excerpt: field("post_excerpt").to_php_string(),
                    content: field("post_content").to_php_string(),
                    parent: field("post_parent").to_int() as u64,
                    menu_order: field("menu_order").to_int(),
                    post_type: "nav_menu_item".into(),
                    status: text(field("post_status")),
                    ..Post::default()
                };
                let meta = item
                    .as_array()
                    .unwrap()
                    .iter()
                    .filter(|(key, _)| key.to_php_string().starts_with(b"_menu_item_"))
                    .map(|(key, value)| {
                        let key: &'static str =
                            String::from_utf8(key.to_php_string()).unwrap().leak();
                        (key, text(value))
                    })
                    .collect();
                site.terms
                    .iter_mut()
                    .filter(|term| term.id == menu)
                    .for_each(|term| term.count += 1);
                site.entries.push(Entry { menu, post, meta });
            }
            "option" => {
                let name: &'static str = fields[1].to_string().leak();
                let value = json(fields[2]);
                let stored = match value {
                    Value::Array(_) => String::from_utf8(encode(&value)).unwrap(),
                    value => text(&value),
                };
                site.options.push((name, stored));
            }
            _ => {}
        }
    }
    site
}

/// The arguments of a case, as `wp_parse_args()` leaves them.
fn args_of(json_args: &str) -> NavMenuArgs {
    let given = json(json_args);
    let mut args = NavMenuArgs::default();
    for (key, value) in given.as_array().unwrap() {
        let bytes = value.to_php_string();
        match &key.to_php_string()[..] {
            b"menu" => args.menu = value.clone(),
            b"container" => args.container = bytes,
            b"container_class" => args.container_class = bytes,
            b"container_id" => args.container_id = bytes,
            b"container_aria_label" => args.container_aria_label = bytes,
            b"menu_class" => args.menu_class = bytes,
            b"menu_id" => args.menu_id = bytes,
            b"fallback_cb" => args.fallback = !value.is_empty(),
            b"before" => args.before = bytes,
            b"after" => args.after = bytes,
            b"link_before" => args.link_before = bytes,
            b"link_after" => args.link_after = bytes,
            b"items_wrap" => args.items_wrap = bytes,
            b"item_spacing" => args.item_spacing = bytes,
            b"depth" => args.depth = value.to_int(),
            b"theme_location" => args.theme_location = bytes,
            b"echo" => {}
            other => panic!("{}", String::from_utf8_lossy(other)),
        }
    }
    args
}

fn request_of(page: &str) -> Request {
    match page.split_once(':') {
        Some(("post", id)) => singular(id.parse().unwrap(), &format!("/?p={id}")),
        Some(("page", id)) => singular(id.parse().unwrap(), &format!("/?page_id={id}")),
        _ => home(),
    }
}

fn hex(text: &str) -> Vec<u8> {
    (0..text.len())
        .step_by(2)
        .map(|at| u8::from_str_radix(text.get(at..at + 2).unwrap(), 16).unwrap())
        .collect()
}

/// Every case prints what PHP printed, the IDs printed before kept from
/// case to case as PHP's static list keeps them.
#[test]
fn menus_print_as_php_prints_them() {
    let site = golden_site();
    let mut printed = Printed::default();
    let mut cases = 0;
    for line in GOLDEN.lines().filter(|line| line.starts_with("case\t")) {
        let fields: Vec<&str> = line.split('\t').collect();
        let expected = match fields[3] {
            "false" => NavMenu::False,
            markup => NavMenu::Printed(hex(markup)),
        };
        let menu = wp_nav_menu(
            &site,
            &request_of(fields[2]),
            &args_of(fields[1]),
            &mut printed,
        );
        let shown = |menu: &NavMenu| match menu {
            NavMenu::Printed(markup) => String::from_utf8_lossy(markup).into_owned(),
            other => format!("{other:?}"),
        };
        assert_eq!(
            shown(&menu),
            shown(&expected),
            "{} {}",
            fields[1],
            fields[2]
        );
        cases += 1;
    }
    assert_eq!(cases, 32);
}

/// `wp_page_menu`, the default fallback, is not ported: no menu, an empty
/// one without a location, and a format PHP refuses answer that.
#[test]
fn what_is_not_ported_says_so() {
    let site = golden_site();
    let not_ported =
        |args: NavMenuArgs| wp_nav_menu(&site, &home(), &args, &mut Printed::default());
    // A location without a menu; a missing menu without a location would
    // take the first menu with items instead.
    let missing = NavMenuArgs {
        menu: Value::String(b"Missing".to_vec()),
        theme_location: b"menu-9".to_vec(),
        ..NavMenuArgs::default()
    };
    assert_eq!(not_ported(missing), NavMenu::NotPorted);
    let empty = NavMenuArgs {
        menu: Value::String(b"Empty".to_vec()),
        ..NavMenuArgs::default()
    };
    assert_eq!(not_ported(empty.clone()), NavMenu::NotPorted);
    // With a location, an empty menu falls back to nothing.
    let located = NavMenuArgs {
        theme_location: b"menu-3".to_vec(),
        ..empty
    };
    assert_eq!(not_ported(located), NavMenu::False);
    for wrap in [
        "%d",
        "%1s",
        "%4$s",
        "%0$s",
        "%",
        "%x$s",
        "%99999999999999999999$s",
        "%18446744073709551616$s",
    ] {
        let args = NavMenuArgs {
            theme_location: b"menu-2".to_vec(),
            items_wrap: wrap.into(),
            fallback: false,
            ..NavMenuArgs::default()
        };
        assert_eq!(not_ported(args), NavMenu::NotPorted, "{wrap}");
    }
    // An item of a kind not ported.
    let mut site = golden_site();
    site.entries[0].set("_menu_item_type", "taxonomy");
    site.entries[0].set("_menu_item_object", "wp_pattern_category");
    let args = NavMenuArgs {
        theme_location: b"menu-1".to_vec(),
        ..NavMenuArgs::default()
    };
    assert_eq!(
        wp_nav_menu(&site, &home(), &args, &mut Printed::default()),
        NavMenu::NotPorted
    );
    // Also in the first menu with items, taken without a menu or location.
    let footer = site
        .entries
        .iter_mut()
        .find(|entry| entry.menu == 3)
        .unwrap();
    footer.set("_menu_item_type", "taxonomy");
    footer.set("_menu_item_object", "wp_pattern_category");
    let args = NavMenuArgs {
        fallback: false,
        ..NavMenuArgs::default()
    };
    assert_eq!(
        wp_nav_menu(&site, &home(), &args, &mut Printed::default()),
        NavMenu::NotPorted
    );
}

/// A menu ID numbered past PHP's integers, which PHP counts on in floats,
/// is not ported; up to them it counts on.
#[test]
fn ids_count_on_up_to_the_largest_integer() {
    let mut site = golden_site();
    site.terms[1].slug = b"links-9223372036854775806".to_vec();
    let args = NavMenuArgs {
        menu: Value::String(b"Footer links".to_vec()),
        container: Vec::new(),
        fallback: false,
        ..NavMenuArgs::default()
    };
    let mut printed = Printed::default();
    let mut id_of = |site: &Menus| match wp_nav_menu(site, &home(), &args, &mut printed) {
        NavMenu::Printed(menu) => String::from_utf8(menu)
            .unwrap()
            .get(8..38)
            .unwrap()
            .to_string(),
        other => format!("{other:?}"),
    };
    assert_eq!(id_of(&site), "menu-links-9223372036854775806");
    assert_eq!(id_of(&site), "menu-links-9223372036854775807");
    assert_eq!(id_of(&site), "NotPorted");
    site.terms[1].slug = b"links-99999999999999999999".to_vec();
    let mut printed = Printed::default();
    let mut once = || wp_nav_menu(&site, &home(), &args, &mut printed);
    assert!(matches!(once(), NavMenu::Printed(_)));
    assert_eq!(once(), NavMenu::NotPorted);
}
