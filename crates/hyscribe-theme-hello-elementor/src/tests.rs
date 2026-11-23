// SPDX-License-Identifier: AGPL-3.0-or-later

//! The port against the SDK's test host: what each callback does and what
//! each template prints, given what the host's tags answer. The pages as
//! PHP prints them are checked by the host's golden suites.

use std::collections::BTreeMap;

use hyscribe_sdk::query::Post;
use hyscribe_sdk::test::Host;

use super::*;

fn text(s: &str) -> Value {
    Value::Text(s.into())
}

/// The key the test host holds a tag's answer under for `args`.
fn key(name: &str, args: &[Value]) -> String {
    format!("{name}{args:?}")
}

fn menu_args(location: &str) -> Vec<Value> {
    let args = format!(
        r#"{{"theme_location":"{location}","fallback_cb":false,"container":false,"echo":false}}"#
    );
    vec![Value::Json(args)]
}

/// What the host's tags print on a page, by key.
fn printed() -> BTreeMap<String, Vec<u8>> {
    let entries: [(String, &str); 22] = [
        (key("language_attributes", &[]), "lang=\"en-US\""),
        (key("bloginfo", &[text("charset")]), "UTF-8"),
        (key("wp_head", &[]), "<meta name=\"head\">\n"),
        (key("body_class", &[]), "class=\"home\""),
        (key("wp_body_open", &[]), ""),
        (key("esc_url", &[text("#content")]), "#content"),
        (key("get_bloginfo", &[text("name")]), "Site & Co"),
        (
            key("get_bloginfo", &[text("description"), text("display")]),
            "It&#8217;s",
        ),
        (
            key("wp_nav_menu", &menu_args("menu-1")),
            "<ul id=\"menu-1\"></ul>\n",
        ),
        (key("wp_nav_menu", &menu_args("menu-2")), ""),
        (key("home_url", &[text("/")]), "http://x.test/"),
        (key("esc_url", &[text("http://x.test/")]), "http://x.test/"),
        (
            key("post_class", &[text("site-main")]),
            "class=\"post-1 post\"",
        ),
        (
            key(
                "the_title",
                &[text("<h1 class=\"entry-title\">"), text("</h1>")],
            ),
            "<h1 class=\"entry-title\">Hello</h1>",
        ),
        (key("the_content", &[]), "<p>Body\u{ff}</p>\n"),
        (key("wp_link_pages", &[]), ""),
        (key("wp_footer", &[]), "<script></script>\n"),
        (key("get_permalink", &[]), "http://x.test/?p=1"),
        (
            key("esc_url", &[text("http://x.test/?p=1")]),
            "http://x.test/?p=1",
        ),
        (key("get_the_title", &[]), "Hello <b>x</b>"),
        (
            key("wp_kses_post", &[text("Hello <b>x</b>")]),
            "Hello <b>x</b>",
        ),
        (key("the_excerpt", &[]), "<p>Excerpt</p>\n"),
    ];
    entries
        .into_iter()
        .map(|(key, value)| (key, value.as_bytes().to_vec()))
        .collect()
}

/// A host with Hello registered, one post in the loop, and the tags of a
/// page answered.
fn fresh() -> Host<Hello> {
    let host = Host::<Hello>::new();
    host.init().unwrap();
    host.state(|state| {
        state.printed = printed();
        state.posts = vec![Post::default()];
        state.values.insert("comments_template".into(), Value::Null);
        state.values.insert("max_num_pages".into(), Value::Int(1));
    });
    host
}

const HEADER: &str = "<!doctype html>\n<html lang=\"en-US\">\n<head>\n\t<meta charset=\"UTF-8\">\n\t<meta name=\"viewport\" content=\"width=device-width, initial-scale=1\">\n\t<link rel=\"profile\" href=\"https://gmpg.org/xfn/11\">\n\t<meta name=\"head\">\n</head>\n<body class=\"home\">\n\n\n<a class=\"skip-link screen-reader-text\" href=\"#content\">Skip to content</a>\n\n";

const HEADER_PART: &str = "\n<header id=\"site-header\" class=\"site-header\">\n\n\t<div class=\"site-branding\">\n\t\t\t\t\t<div class=\"site-title\">\n\t\t\t\t<a href=\"http://x.test/\" title=\"Home\" rel=\"home\">\n\t\t\t\t\tSite &amp; Co\t\t\t\t</a>\n\t\t\t</div>\n\t\t\t\t\t\t<p class=\"site-description\">\n\t\t\t\tIt&#8217;s\t\t\t</p>\n\t\t\t\t\t\t</div>\n\n\t\t\t<nav class=\"site-navigation\" aria-label=\"Main menu\">\n\t\t\t<ul id=\"menu-1\"></ul>\n\t\t</nav>\n\t</header>\n";

const SINGLE: &str = "\n<main id=\"content\" class=\"post-1 post\">\n\n\t\t\t<div class=\"page-header\">\n\t\t\t<h1 class=\"entry-title\">Hello</h1>\t\t</div>\n\t\n\t<div class=\"page-content\">\n\t\t<p>Body\u{ff}</p>\n\n\t\t\n\t\t\t</div>\n\n\t\n</main>\n\n\t";

const FOOTER: &str = "<footer id=\"site-footer\" class=\"site-footer\">\n\t</footer>\n\n<script></script>\n\n</body>\n</html>\n";

fn page(file: &str) -> String {
    String::from_utf8(Hello::render(file).unwrap()).unwrap()
}

#[test]
fn init_registers_its_hooks_and_files() {
    let host = Host::<Hello>::new();
    host.init().unwrap();
    let hooks = host.state(|state| state.hooks());
    let named: Vec<(&str, i32)> = hooks
        .iter()
        .map(|h| (h.hook.as_str(), h.priority))
        .collect();
    assert_eq!(
        &named[..2],
        &[("after_setup_theme", 0), ("after_setup_theme", 10)]
    );
    assert_eq!(hooks.len(), 10);
    assert_eq!(host.state(|state| state.templates.len()), 11);
    let host = Host::<Hello>::new();
    host.state(|state| state.fails.insert("theme-setup.templates"));
    assert!(host.init().is_err());
    assert_eq!((host.activate(), host.deactivate()), (Ok(()), Ok(())));
}

#[test]
fn setup_adds_hello_s_features() {
    let host = fresh();
    host.action("after_setup_theme", &[]);
    host.state(|state| {
        let features: Vec<&str> = state.supports.iter().map(|(f, _)| f.as_str()).collect();
        assert_eq!(features.len(), 12);
        assert_eq!(features[3], "html5");
        assert_eq!(features[11], "wc-product-gallery-slider");
        assert!(matches!(&state.supports[4].1[..], [Value::Json(logo)] if logo.contains("350")));
        let menus = [
            ("menu-1".into(), "Header".into()),
            ("menu-2".into(), "Footer".into()),
        ];
        assert_eq!(state.nav_menus, menus);
        assert_eq!(state.editor_styles, ["assets/css/editor-styles.css"]);
        assert_eq!(state.content_width, Some(800));
        assert!(state.options.is_empty(), "no write outside the admin");
    });
    // Each filter turns its part off.
    let host = Host::<Hello>::new();
    host.init().unwrap();
    host.add("hello_elementor_add_woocommerce_support", 10, |_| {
        Value::Bool(false)
    });
    host.add("hello_elementor_content_width", 10, |_| text("wide"));
    host.action("after_setup_theme", &[]);
    host.state(|state| assert_eq!((state.supports.len(), state.content_width), (8, Some(0))));
    let host = Host::<Hello>::new();
    host.init().unwrap();
    host.add("hello_elementor_register_menus", 10, |_| Value::Int(0));
    host.add("hello_elementor_add_theme_support", 10, |_| Value::Null);
    host.action("after_setup_theme", &[]);
    host.state(|state| assert!(state.supports.is_empty() && state.nav_menus.is_empty()));
}

#[test]
fn setup_fails_with_its_calls() {
    let failing = |call: &'static str| {
        let host = fresh();
        host.state(|state| state.fails.insert(call));
        setup(Vec::new())
    };
    assert!(failing("theme-setup.register-nav-menus").is_err());
    let failed = failing("theme-setup.add-support").unwrap_err();
    assert!(
        failed.contains("theme-setup.add-support failed"),
        "{failed}"
    );
    assert!(failing("theme-setup.add-editor-style").is_err());
    assert!(failing("template.conditional").is_err());
    let host = fresh();
    host.state(|state| {
        state.conditionals.insert("is_admin".into());
        state.fails.insert("options.get");
    });
    assert!(setup(Vec::new()).is_err());
    host.state(|state| state.fails = ["theme-setup.set-content-width"].into());
    assert!(content_width(Vec::new()).is_err());
}

/// The version stored after an admin request, given the stored one.
fn version_after(stored: Option<Value>) -> Option<Value> {
    let host = fresh();
    host.state(|state| {
        state.conditionals.insert("is_admin".into());
        if let Some(stored) = stored {
            state.options.insert("hello_theme_version".into(), stored);
        }
    });
    setup(Vec::new()).unwrap();
    host.state(|state| state.options.get("hello_theme_version").cloned())
}

#[test]
fn an_admin_request_writes_the_version_when_missing_or_older() {
    let ours = Some(text(VERSION));
    let kept = [text("3.6.0"), text("3.5.2"), Value::Int(5)];
    for stored in kept {
        assert_eq!(version_after(Some(stored.clone())), Some(stored));
    }
    let written = [
        None,
        Some(text("3.4.4")),
        Some(text("")),
        Some(text("0")),
        Some(Value::Int(1)),
        Some(Value::Bool(true)),
        Some(Value::Bool(false)),
    ];
    for stored in written {
        assert_eq!(version_after(stored), ours);
    }
    // A read only site refuses the write, which PHP would not see either.
    let host = fresh();
    host.state(|state| {
        state.conditionals.insert("is_admin".into());
        state.fails.insert("options.update");
    });
    assert_eq!(setup(Vec::new()), Ok(Value::Null));
}

#[test]
fn styles_are_enqueued_as_their_filters_allow() {
    let host = fresh();
    host.action("wp_enqueue_scripts", &[]);
    host.state(|state| {
        let handles: Vec<&str> = state.enqueued.iter().map(|(_, h)| h.as_str()).collect();
        assert_eq!(
            handles,
            [
                "hello-elementor",
                "hello-elementor-theme-style",
                "hello-elementor-header-footer"
            ]
        );
        assert_eq!(state.assets[1].src, "assets/css/theme.css");
        assert_eq!(state.assets[2].version.as_deref(), Some("3.5.2"));
    });
    let host = fresh();
    host.state(|state| {
        let on = text("true");
        state
            .options
            .insert("hello_elementor_settings_hello_style".into(), on.clone());
        state
            .options
            .insert("hello_elementor_settings_header_footer".into(), on);
    });
    host.action("wp_enqueue_scripts", &[]);
    let handles = host.state(|state| state.enqueued.clone());
    assert_eq!(
        handles,
        [(AssetKind::Style, "hello-elementor-theme-style".into())]
    );
    for call in ["assets.register", "assets.enqueue"] {
        let host = fresh();
        host.state(|state| state.fails.insert(call));
        assert!(styles(Vec::new()).is_err());
    }
}

#[test]
fn the_description_meta_tag_prints_a_post_s_excerpt() {
    let field = [text("post_excerpt"), Value::Int(1)];
    let given = |excerpt: &str| {
        let host = fresh();
        host.state(|state| {
            state.conditionals.insert("is_singular".into());
            state
                .values
                .insert("get_queried_object_id".into(), Value::Int(1));
            let excerpt = excerpt.as_bytes().to_vec();
            state.printed.insert(key("get_post_field", &field), excerpt);
            let stripped = b"An \"x\" & <y>".to_vec();
            state
                .printed
                .insert(key("wp_strip_all_tags", &[text("A <b>x</b>")]), stripped);
        });
        host
    };
    let host = given("A <b>x</b>");
    let printed = host.action("wp_head", &[]);
    assert_eq!(
        printed,
        "<meta name=\"description\" content=\"An &quot;x&quot; &amp; &lt;y&gt;\">\n"
    );
    for excerpt in ["", "0"] {
        let _host = given(excerpt);
        assert_eq!(description(Vec::new()), Ok(Value::Null));
    }
    let host = given("A <b>x</b>");
    host.state(|state| state.conditionals.clear());
    assert_eq!(description(Vec::new()), Ok(Value::Null));
    host.add("hello_elementor_description_meta_tag", 10, |_| {
        Value::Bool(false)
    });
    host.state(|state| state.conditionals.insert("is_singular".into()));
    assert_eq!(description(Vec::new()), Ok(Value::Null));
    let host = given("A <b>x</b>");
    let on = text("true");
    host.state(|state| {
        state
            .options
            .insert("hello_elementor_settings_description_meta_tag".into(), on)
    });
    assert_eq!(description(Vec::new()), Ok(Value::Null));
    // Each call's failure fails the callback.
    let host = given("A <b>x</b>");
    host.state(|state| state.fails.insert("options.get"));
    assert!(description(Vec::new()).is_err());
    let host = given("A <b>x</b>");
    host.state(|state| state.refused.insert("is_singular".into()));
    assert!(description(Vec::new()).is_err());
    for gone in [
        "get_queried_object_id",
        "get_post_field",
        "wp_strip_all_tags",
    ] {
        let host = given("A <b>x</b>");
        host.state(|state| {
            state.values.remove(gone);
            state.printed.retain(|key, _| !key.starts_with(gone));
        });
        assert!(description(Vec::new()).is_err(), "{gone}");
    }
}

#[test]
fn the_body_class_of_the_notifications_package_is_added() {
    let host = fresh();
    let classes = host.filter("body_class", Value::Json("[\"home\"]".into()), &[]);
    assert_eq!(
        classes,
        Value::Json("[\"home\",\"hello-elementor-default\"]".into())
    );
    let none = body_class(vec![Value::Json("[]".into())]);
    assert_eq!(
        none,
        Ok(Value::Json("[\"hello-elementor-default\"]".into()))
    );
    assert_eq!(body_class(vec![text("home")]), Ok(text("home")));
    assert_eq!(
        body_class(vec![Value::Json("{}".into())]),
        Ok(Value::Json("{}".into()))
    );
    assert_eq!(body_class(Vec::new()), Ok(Value::Null));
}

#[test]
fn settings_turn_their_filters_off() {
    let host = fresh();
    let hooks = [
        ("hello_elementor_enable_skip_link", "_skip_link"),
        ("hello_elementor_header_footer", "_header_footer"),
        ("hello_elementor_page_title", "_page_title"),
        ("hello_elementor_enqueue_style", "_hello_style"),
        ("hello_elementor_enqueue_theme_style", "_hello_theme"),
    ];
    for (hook, suffix) in hooks {
        assert_eq!(host.filter(hook, Value::Int(2), &[]), Value::Int(2));
        let option = format!("hello_elementor_settings{suffix}");
        host.state(|state| state.options.insert(option, text("true")));
        assert_eq!(host.filter(hook, Value::Int(2), &[]), Value::Bool(false));
    }
    assert_eq!(page_title(Vec::new()), Ok(Value::Bool(false)));
    host.state(|state| state.options.clear());
    assert_eq!(page_title(Vec::new()), Ok(Value::Null));
    host.state(|state| state.fails.insert("options.get"));
    assert!(page_title(vec![text("x")]).is_err());
}

#[test]
fn values_are_true_as_in_php() {
    let truth = [
        (Value::Null, false),
        (Value::Bool(true), true),
        (Value::Int(0), false),
        (Value::Int(-1), true),
        (Value::Float(0.0), false),
        (Value::Float(0.5), true),
        (text(""), false),
        (text("0"), false),
        (text("00"), true),
        (Value::Json("[]".into()), false),
        (Value::Json("[1]".into()), true),
    ];
    for (value, truthy_in_php) in truth {
        assert_eq!(truthy(&value), truthy_in_php, "{value:?}");
    }
}

#[test]
fn a_single_post_prints_as_hello_s_templates() {
    let host = fresh();
    host.state(|state| {
        state.conditionals.insert("is_singular".into());
        state.templates.push(TemplateFile {
            file: "x.php".into(),
            name: None,
            post_types: Vec::new(),
        });
    });
    let expected = [HEADER, HEADER_PART, SINGLE, FOOTER].concat();
    assert_eq!(page("index.php"), expected);
    assert_eq!(host.render("x.php"), Err("no template x.php".into()));
    assert_eq!(page("sidebar.php"), "");
}

#[test]
fn hello_s_filters_and_settings_change_its_header_and_footer() {
    let host = fresh();
    host.state(|state| {
        let on = text("true");
        state
            .options
            .insert("hello_elementor_settings_skip_link".into(), on.clone());
        state
            .options
            .insert("hello_elementor_settings_header_footer".into(), on);
    });
    host.add("hello_elementor_viewport_content", 10, |_| {
        text("width=\"1\"")
    });
    let header = HEADER
        .replace("initial-scale=1", "")
        .replace("width=device-width, ", "width=&quot;1&quot;")
        .replace(
            "<a class=\"skip-link screen-reader-text\" href=\"#content\">Skip to content</a>\n",
            "",
        );
    assert_eq!(page("header.php"), header);
    assert_eq!(
        page("footer.php"),
        "\n<script></script>\n\n</body>\n</html>\n"
    );
    host.add("hello_elementor_viewport_content", 20, |_| Value::Int(2));
    assert!(page("header.php").contains("content=\"2\""));
    host.add("hello_elementor_viewport_content", 30, |_| {
        Value::Bool(true)
    });
    assert!(page("header.php").contains("content=\"1\""));
    host.add("hello_elementor_viewport_content", 40, |_| Value::Null);
    assert!(page("header.php").contains("content=\"\""));
}

#[test]
fn the_header_part_shows_a_logo_or_the_site_s_name() {
    let host = fresh();
    host.state(|state| {
        state.conditionals.insert("has_custom_logo".into());
        state
            .printed
            .insert(key("the_custom_logo", &[]), b"<img>".to_vec());
        let menu = key("wp_nav_menu", &menu_args("menu-2"));
        state.printed.insert(menu, b"<ul></ul>\n".to_vec());
    });
    let logo = "\n<header id=\"site-header\" class=\"site-header\">\n\n\t<div class=\"site-branding\">\n\t\t<img>\t</div>\n\n";
    assert!(page("template-parts/header.php").starts_with(logo));
    let tag = host.state(|state| state.printed.remove("the_custom_logo[]"));
    assert!(Hello::render("template-parts/header.php").is_err());
    host.state(|state| {
        state
            .printed
            .insert("the_custom_logo[]".into(), tag.unwrap())
    });
    let footer = "<footer id=\"site-footer\" class=\"site-footer\">\n\t\t\t<nav class=\"site-navigation\" aria-label=\"Footer menu\">\n\t\t\t<ul></ul>\n\t\t</nav>\n\t</footer>\n";
    assert_eq!(page("template-parts/footer.php"), footer);
    host.state(|state| {
        state.conditionals.clear();
        state.printed.insert(
            key("get_bloginfo", &[text("description"), text("display")]),
            b"0".to_vec(),
        );
        state
            .printed
            .insert(key("wp_nav_menu", &menu_args("menu-1")), Vec::new());
    });
    let no_tagline = "\t\t\t</div>\n\t\t\t\t\t\t</div>\n\n\t</header>\n";
    assert!(page("template-parts/header.php").ends_with(no_tagline));
    host.state(|state| {
        state
            .printed
            .insert(key("get_bloginfo", &[text("name")]), Vec::new());
    });
    let nameless = "\t<div class=\"site-branding\">\n\t\t\t</div>\n\n\t</header>\n";
    assert!(page("template-parts/header.php").ends_with(nameless));
}

#[test]
fn a_single_post_shows_its_tags_and_comments_without_a_title() {
    let host = fresh();
    host.state(|state| {
        state
            .options
            .insert("hello_elementor_settings_page_title".into(), text("true"));
        state.conditionals.insert("has_tag".into());
        let tags = [
            text("<span class=\"tag-links\">Tagged "),
            text(", "),
            text("</span>"),
        ];
        state
            .printed
            .insert(key("the_tags", &tags), b"<span>t</span>".to_vec());
        state
            .values
            .insert("comments_template".into(), text("sidebar.php"));
    });
    let single = page("template-parts/single.php");
    assert!(single.starts_with(
        "\n<main id=\"content\" class=\"post-1 post\">\n\n\t\n\t<div class=\"page-content\">"
    ));
    assert!(single.contains("\t\t\t\t<div class=\"post-tags\">\n\t\t\t<span>t</span>\t\t</div>\n\t\t\t</div>\n\n\t\n</main>"));
    host.state(|state| {
        state.looped = 0;
        state.printed.retain(|key, _| !key.starts_with("the_tags"));
    });
    assert!(Hello::render("template-parts/single.php").is_err());
}

const ENTRY: &str = "\t\t\t<article class=\"post\">\n\t\t\t\t<h2 class=\"entry-title\"><a href=\"http://x.test/?p=1\">Hello <b>x</b></a></h2><p>Excerpt</p>\n\t\t\t</article>\n\t\t";

#[test]
fn an_archive_lists_its_posts_with_pagination() {
    let host = fresh();
    host.state(|state| {
        let title = [text("<h1 class=\"entry-title\">"), text("</h1>")];
        state
            .printed
            .insert(key("the_archive_title", &title), b"<h1>A</h1>".to_vec());
        let description = [text("<p class=\"archive-description\">"), text("</p>")];
        state
            .printed
            .insert(key("the_archive_description", &description), Vec::new());
    });
    let archive = format!(
        "<main id=\"content\" class=\"site-main\">\n\n\t\t\t<div class=\"page-header\">\n\t\t\t<h1>A</h1>\t\t</div>\n\t\n\t<div class=\"page-content\">\n\t\t{ENTRY}\t</div>\n\n\t\n</main>\n"
    );
    assert_eq!(page("template-parts/archive.php"), archive);
    host.state(|state| {
        state.looped = 0;
        state.values.insert("max_num_pages".into(), Value::Int(2));
        state.conditionals.insert("has_post_thumbnail".into());
        let thumb = [Value::Null, text("large")];
        state
            .printed
            .insert(key("get_the_post_thumbnail", &thumb), b"<img>".to_vec());
        let previous = text("<span class=\"meta-nav\">&larr;</span> Previous");
        state
            .printed
            .insert(key("previous_posts_link", &[previous]), b"P".to_vec());
        let next = text("Next <span class=\"meta-nav\">&rarr;</span>");
        state
            .printed
            .insert(key("next_posts_link", &[next]), b"N".to_vec());
        state
            .options
            .insert("hello_elementor_settings_page_title".into(), text("true"));
    });
    let paged = page("template-parts/archive.php");
    assert!(paged.contains("</a></h2><a href=\"http://x.test/?p=1\"><img></a><p>Excerpt</p>"));
    assert!(paged.ends_with("\t\t<nav class=\"pagination\">\n\t\t\t<div class=\"nav-previous\">P</div>\n\t\t\t<div class=\"nav-next\">N</div>\n\t\t</nav>\n\t\n</main>\n"));
    assert!(paged.starts_with("<main id=\"content\" class=\"site-main\">\n\n\t\n\t<div"));
    host.state(|state| {
        state.looped = 0;
        state.conditionals.insert("is_rtl".into());
        let previous = text("<span class=\"meta-nav\">&rarr;</span> Previous");
        state
            .printed
            .insert(key("previous_posts_link", &[previous]), b"R".to_vec());
        let next = text("Next <span class=\"meta-nav\">&larr;</span>");
        state
            .printed
            .insert(key("next_posts_link", &[next]), b"L".to_vec());
    });
    assert!(
        page("template-parts/archive.php")
            .contains(">R</div>\n\t\t\t<div class=\"nav-next\">L</div>")
    );
}

#[test]
fn a_search_shows_its_results_or_says_there_are_none() {
    let host = fresh();
    host.state(|state| {
        state
            .printed
            .insert(key("get_search_query", &[]), b"a &amp; b".to_vec());
    });
    let results = "<main id=\"content\" class=\"site-main\">\n\n\t\t\t<div class=\"page-header\">\n\t\t\t<h1 class=\"entry-title\">\n\t\t\t\tSearch results for: \t\t\t\t<span>a &amp; b</span>\n\t\t\t</h1>\n\t\t</div>\n\t\n\t<div class=\"page-content\">\n\t\t\t\t\t\t\t\t\t<article class=\"post\">\n\t\t\t\t\t<h2 class=\"entry-title\"><a href=\"http://x.test/?p=1\">Hello <b>x</b></a></h2><p>Excerpt</p>\n\t\t\t\t</article>\n\t\t\t\t\t\t\t</div>\n\n\t\n</main>\n";
    assert_eq!(page("template-parts/search.php"), results);
    host.state(|state| {
        state.posts.clear();
        state
            .options
            .insert("hello_elementor_settings_page_title".into(), text("true"));
    });
    let none = "<main id=\"content\" class=\"site-main\">\n\n\t\n\t<div class=\"page-content\">\n\t\t\t\t\t<p>It seems we can&#039;t find what you&#039;re looking for.</p>\n\t\t\t</div>\n\n\t\n</main>\n";
    assert_eq!(page("template-parts/search.php"), none);
}

#[test]
fn a_missing_page_says_so() {
    let _host = fresh();
    let missing = "<main id=\"content\" class=\"site-main\">\n\n\t\t\t<div class=\"page-header\">\n\t\t\t<h1 class=\"entry-title\">The page can&rsquo;t be found.</h1>\n\t\t</div>\n\t\n\t<div class=\"page-content\">\n\t\t<p>It looks like nothing was found at this location.</p>\n\t</div>\n\n</main>\n";
    assert_eq!(page("template-parts/404.php"), missing);
    assert!(page("index.php").contains("The page can&rsquo;t be found."));
}

#[test]
fn index_picks_the_part_for_the_page() {
    let host = fresh();
    host.state(|state| {
        state
            .printed
            .insert(key("get_search_query", &[]), Vec::new())
    });
    for (conditional, shown) in [
        ("is_archive", "<article"),
        ("is_home", "<article"),
        ("is_search", "Search results for: "),
    ] {
        host.state(|state| {
            state.looped = 0;
            state.conditionals = [conditional.to_string()].into();
            let title = [text("<h1 class=\"entry-title\">"), text("</h1>")];
            state
                .printed
                .insert(key("the_archive_title", &title), Vec::new());
            let description = [text("<p class=\"archive-description\">"), text("</p>")];
            state
                .printed
                .insert(key("the_archive_description", &description), Vec::new());
        });
        assert!(page("index.php").contains(shown), "{conditional}");
    }
}

/// A host whose post has comments and takes Hello's `comments.php`.
fn commented(number: &str) -> Host<Hello> {
    let host = fresh();
    host.state(|state| {
        state.values.insert("get_post_type".into(), text("post"));
        state.values.insert("get_comments_number".into(), text(number));
        for name in ["post_type_supports", "have_comments", "comments_open"] {
            state.conditionals.insert(name.into());
        }
        state.options.insert("thread_comments".into(), text("1"));
        state.printed.insert(key("the_comments_navigation", &[]), b"<nav/>".to_vec());
        let list = Value::Json(r#"{"style":"ol","short_ping":true,"avatar_size":42}"#.into());
        state.printed.insert(key("wp_list_comments", &[list]), b"<li>c</li>\n".to_vec());
        let form = r#"{"title_reply_before":"<h2 id=\"reply-title\" class=\"comment-reply-title\">","title_reply_after":"</h2>"}"#;
        state.printed.insert(key("comment_form", &[Value::Json(form.into())]), b"<form/>".to_vec());
    });
    host
}

#[test]
fn comments_print_as_hello_s_comments_template() {
    let host = commented("1");
    let expected = "<section id=\"comments\" class=\"comments-area\">\n\n\t\t\t<h2 class=\"title-comments\">\n\t\t\tOne Response\t\t</h2>\n\n\t\t<nav/>\n\t\t<ol class=\"comment-list\">\n\t\t\t<li>c</li>\n\t\t</ol>\n\n\t\t<nav/>\n\n\t\n\t<form/>\n</section>\n";
    assert_eq!(page("comments.php"), expected);
    assert_eq!(
        host.state(|state| state.enqueued.clone()),
        [(AssetKind::Script, "comment-reply".into())]
    );
    let _host = commented("1234");
    assert!(page("comments.php").contains("\t\t\t1,234 Responses\t\t</h2>"));
    let host = commented("0");
    host.state(|state| {
        state.conditionals.remove("have_comments");
        state.options.insert("thread_comments".into(), text(""));
    });
    assert_eq!(
        page("comments.php"),
        "<section id=\"comments\" class=\"comments-area\">\n\n\t\n\t<form/>\n</section>\n"
    );
    assert!(host.state(|state| state.enqueued.is_empty()));
    // Comments closed on a post that has some: the list, and no script.
    host.state(|state| {
        state.conditionals.insert("have_comments".into());
        state.conditionals.remove("comments_open");
        state.options.insert("thread_comments".into(), text("1"));
    });
    assert!(page("comments.php").contains("<ol class=\"comment-list\">"));
    assert!(host.state(|state| state.enqueued.is_empty()));
    host.state(|state| state.conditionals.remove("have_comments"));
    assert_eq!(page("comments.php"), "");
    host.state(|state| {
        state.conditionals.insert("have_comments".into());
        state.conditionals.remove("post_type_supports");
    });
    assert_eq!(page("comments.php"), "");
}

#[test]
fn numbers_are_grouped_by_thousands() {
    let cases = [
        (0, "0"),
        (999, "999"),
        (1000, "1,000"),
        (1234567, "1,234,567"),
    ];
    for (n, shown) in cases {
        assert_eq!(templates::number_format_i18n(n), shown);
    }
}

#[test]
fn a_tag_s_failure_fails_the_page() {
    let host = commented("2");
    host.state(|state| {
        state.conditionals.insert("is_singular".into());
        state
            .values
            .insert("comments_template".into(), text("comments.php"));
    });
    assert!(Hello::render("index.php").is_ok());
    let archive_only = [
        "get_permalink",
        "esc_url[Text(\"http://x.test/?p=1\")]",
        "get_the_title",
        "wp_kses_post",
        "the_excerpt",
    ];
    host.state(|state| {
        state
            .printed
            .retain(|key, _| !archive_only.iter().any(|o| key.starts_with(o)))
    });
    let printed = host.state(|state| state.printed.clone());
    for gone in printed.keys() {
        host.state(|state| {
            state.looped = 0;
            state.printed = printed.clone();
            state.printed.remove(gone);
        });
        assert!(Hello::render("index.php").is_err(), "{gone}");
    }
    host.state(|state| state.printed = printed.clone());
    for gone in ["comments_template", "get_post_type", "get_comments_number"] {
        let values = host.state(|state| state.values.clone());
        host.state(|state| {
            state.looped = 0;
            state.values.remove(gone);
        });
        assert!(Hello::render("index.php").is_err(), "{gone}");
        host.state(|state| state.values = values);
    }
    let refusals = [
        "is_singular",
        "has_custom_logo",
        "has_tag",
        "post_type_supports",
        "have_comments",
        "comments_open",
    ];
    for refused in refusals {
        host.state(|state| {
            state.looped = 0;
            state.refused = [refused.to_string()].into();
        });
        assert!(Hello::render("index.php").is_err(), "{refused}");
    }
    host.state(|state| {
        state.looped = 0;
        state.refused.clear();
        state.fails.insert("options.get");
    });
    assert!(Hello::render("comments.php").is_err());
    host.state(|state| {
        state.fails = ["assets.enqueue"].into();
    });
    assert!(Hello::render("comments.php").is_err());
}

#[test]
fn archive_and_search_fail_with_their_tags() {
    let host = fresh();
    host.state(|state| {
        state.conditionals = ["is_archive".to_string(), "has_post_thumbnail".to_string()].into();
        let title = [text("<h1 class=\"entry-title\">"), text("</h1>")];
        state
            .printed
            .insert(key("the_archive_title", &title), Vec::new());
        let description = [text("<p class=\"archive-description\">"), text("</p>")];
        state
            .printed
            .insert(key("the_archive_description", &description), Vec::new());
        let thumb = [Value::Null, text("large")];
        state
            .printed
            .insert(key("get_the_post_thumbnail", &thumb), Vec::new());
        state
            .printed
            .insert(key("get_search_query", &[]), Vec::new());
        state.values.insert("max_num_pages".into(), Value::Int(3));
        let previous = text("<span class=\"meta-nav\">&larr;</span> Previous");
        state
            .printed
            .insert(key("previous_posts_link", &[previous]), Vec::new());
        let next = text("Next <span class=\"meta-nav\">&rarr;</span>");
        state
            .printed
            .insert(key("next_posts_link", &[next]), Vec::new());
    });
    let printed = host.state(|state| state.printed.clone());
    let entry = [
        "get_permalink",
        "esc_url",
        "get_the_title",
        "wp_kses_post",
        "get_the_post_thumbnail",
        "the_excerpt",
        "previous_posts_link",
        "next_posts_link",
    ];
    for (file, own) in [
        (
            "template-parts/archive.php",
            ["the_archive_title", "the_archive_description"].as_slice(),
        ),
        ("template-parts/search.php", ["get_search_query"].as_slice()),
    ] {
        host.state(|state| state.looped = 0);
        assert!(Hello::render(file).is_ok());
        for gone in own.iter().chain(entry.iter()) {
            host.state(|state| {
                state.looped = 0;
                state.printed = printed.clone();
                state.printed.retain(|key, _| !key.starts_with(gone));
            });
            assert!(Hello::render(file).is_err(), "{file} {gone}");
        }
        host.state(|state| state.printed = printed.clone());
        for refused in ["has_post_thumbnail", "is_rtl", "is_archive"] {
            host.state(|state| {
                state.looped = 0;
                state.refused = [refused.to_string()].into();
            });
            if refused != "is_archive" {
                assert!(Hello::render(file).is_err(), "{file} {refused}");
            }
        }
        host.state(|state| state.refused.clear());
        host.state(|state| state.values.remove("max_num_pages"));
        assert!(Hello::render(file).is_err());
        host.state(|state| state.values.insert("max_num_pages".into(), Value::Int(3)));
    }
    host.state(|state| {
        state.looped = 0;
        state.refused = ["is_archive".to_string()].into();
    });
    assert!(Hello::render("index.php").is_err());
    for refused in ["is_home", "is_search"] {
        host.state(|state| {
            state.conditionals.clear();
            state.refused = [refused.to_string()].into();
        });
        assert!(Hello::render("index.php").is_err(), "{refused}");
    }
}

#[test]
fn the_port_prints_no_markup_unescaped() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
    assert_eq!(hyscribe_sdk::page::check_port(root), Ok(()));
}
