// SPDX-License-Identifier: AGPL-3.0-or-later

use std::collections::HashMap;

use hyscribe_config::Database;
use sqlx::{Connection, Executor};

use super::*;
use crate::tests::classic::{page_of, site_on};

/// PHP's answers, `reference/theme-tags/golden.sh`.
const GOLDEN: &str = include_str!("../../../tests/golden/theme-tags-hello-elementor.tsv");

/// The class Hello's own `body_class` filter adds, which its port answers.
const HELLO_CLASS: &str = " hello-elementor-default";

fn text(s: &str) -> Value {
    Value::String(s.as_bytes().to_vec())
}

/// Hello Elementor's PHP files that its templates find by name.
fn hello_files() -> Vec<TemplateFile> {
    let names = [
        "index.php",
        "header.php",
        "footer.php",
        "template-parts/single.php",
        "comments.php",
    ];
    names
        .map(|file| TemplateFile {
            file: file.into(),
            name: None,
            post_types: Vec::new(),
        })
        .to_vec()
}

/// The scratch database with `reference/theme-tags/content.sql` added.
async fn with_content() -> Database {
    content_db(include_str!(
        "../../../../../reference/theme-tags/content.sql"
    ))
    .await
}

/// The scratch database with `content` added.
async fn content_db(content: &str) -> Database {
    let db = Database {
        name: "wordpress_saves".into(),
        ..hyscribe_testkit::fixture_database()
    };
    hyscribe_testkit::writes::load_default_install(&db)
        .await
        .unwrap();
    let options = sqlx::mysql::MySqlConnectOptions::new()
        .host(&db.host)
        .port(db.port)
        .database(&db.name)
        .username(&db.user)
        .password(db.password.expose());
    let mut conn = sqlx::MySqlConnection::connect_with(&options).await.unwrap();
    let statement = sqlx::AssertSqlSafe(content.to_string());
    conn.execute(sqlx::raw_sql(statement)).await.unwrap();
    db
}

/// What `call` answers, as `tags.php` records it.
fn told(theme: &Theme, call: ThemeCall) -> Vec<u8> {
    match theme.answer(&call) {
        Ok(Value::Bool(true)) => b"true".to_vec(),
        Ok(Value::Bool(false)) => b"false".to_vec(),
        Ok(Value::String(printed)) => printed,
        Ok(Value::Int(number)) => number.to_string().into_bytes(),
        other => format!("{other:?}").into_bytes(),
    }
}

fn print(name: &str, args: &[&str]) -> ThemeCall {
    ThemeCall::Print(name.into(), args.iter().map(|arg| text(arg)).collect())
}

fn get(name: &str, args: &[&str]) -> ThemeCall {
    ThemeCall::Get(name.into(), args.iter().map(|arg| text(arg)).collect())
}

fn array(pairs: &[(&str, Value)]) -> Value {
    let pairs = pairs
        .iter()
        .map(|(key, value)| (Key::String(key.as_bytes().to_vec()), value.clone()));
    Value::Array(pairs.collect())
}

/// Hello's `comments.php` as its port renders it, from the host's tags.
fn hello_comments(theme: &Theme) -> Vec<u8> {
    let is = |name: &str, args: Vec<Value>| {
        theme.answer(&ThemeCall::Conditional(name.into(), args)) == Ok(Value::Bool(true))
    };
    let post_type = told(theme, get("get_post_type", &[]));
    let supports = is(
        "post_type_supports",
        vec![Value::String(post_type), text("comments")],
    );
    // `comments_template()` sets the comments up, and on a post or page
    // shown on its own the theme renders its `comments.php`.
    assert_eq!(told(theme, print("comments_template", &[])), b"");
    let shown = is("is_singular", Vec::new()) && supports;
    if !shown || !is("have_comments", Vec::new()) && !is("comments_open", Vec::new()) {
        return Vec::new();
    }
    let mut out = b"<section id=\"comments\" class=\"comments-area\">\n\n\t".to_vec();
    if is("have_comments", Vec::new()) {
        let number = String::from_utf8(told(theme, get("get_comments_number", &[]))).unwrap();
        let title = if number == "1" {
            "One Response".to_string()
        } else {
            format!("{number} Responses")
        };
        let navigation = told(theme, print("the_comments_navigation", &[]));
        let args = array(&[
            ("style", text("ol")),
            ("short_ping", Value::Bool(true)),
            ("avatar_size", Value::Int(42)),
        ]);
        let list = told(
            theme,
            ThemeCall::Print("wp_list_comments".into(), vec![args]),
        );
        out.extend(
            format!("\t\t<h2 class=\"title-comments\">\n\t\t\t{title}\t\t</h2>\n\n\t\t").bytes(),
        );
        out.extend(&navigation);
        out.extend(b"\n\t\t<ol class=\"comment-list\">\n\t\t\t");
        out.extend(list);
        out.extend(b"\t\t</ol>\n\n\t\t");
        out.extend(&navigation);
        out.extend(b"\n\t");
    }
    out.extend(b"\n\t");
    out.extend(told(theme, comment_form()));
    out.extend(b"\n</section>\n");
    out
}

/// Hello's `comment_form()`.
fn comment_form() -> ThemeCall {
    let args = array(&[
        (
            "title_reply_before",
            text("<h2 id=\"reply-title\" class=\"comment-reply-title\">"),
        ),
        ("title_reply_after", text("</h2>")),
    ]);
    ThemeCall::Print("comment_form".into(), vec![args])
}

/// The tags `tags.php` records after the first ones, in its order.
fn more_tags(theme: &Theme, out: &mut Vec<(String, Vec<u8>)>) {
    macro_rules! put {
        ($name:expr, $call:expr $(,)?) => {
            out.push(($name, told(theme, $call)))
        };
    }
    let shows = [
        "name",
        "description",
        "charset",
        "url",
        "wpurl",
        "html_type",
        "language",
        "rss2_url",
        "atom_url",
        "comments_rss2_url",
        "pingback_url",
        "nope",
    ];
    for show in shows {
        put!(format!("get_bloginfo:{show}"), get("get_bloginfo", &[show]));
        let display = get("get_bloginfo", &[show, "display"]);
        put!(format!("get_bloginfo:{show}:display"), display);
    }
    for show in ["charset", "name"] {
        put!(format!("bloginfo:{show}"), print("bloginfo", &[show]));
    }
    put!("home_url".into(), get("home_url", &[]));
    put!("home_url:/".into(), get("home_url", &["/"]));
    let samples = [
        "#content",
        "http://127.0.0.1:8081/?p=1&a=b",
        "example.test/a b",
        "javascript:alert(1)",
        "a\"b<c>&amp;'d",
        "<p onclick=\"x\">Hi <script>x</script><a href=\"https://w.test/\">w</a></p>",
    ];
    for (at, sample) in samples.iter().enumerate() {
        for name in ["esc_url", "esc_attr", "esc_html", "wp_kses_post"] {
            put!(format!("{name}:{at}"), print(name, &[sample]));
        }
    }
    let title = print(
        "the_archive_title",
        &["<h1 class=\"entry-title\">", "</h1>"],
    );
    put!("the_archive_title".into(), title);
    let description = ["<p class=\"archive-description\">", "</p>"];
    put!(
        "the_archive_description".into(),
        print("the_archive_description", &description),
    );
    put!("get_search_query".into(), get("get_search_query", &[]));
    let raw = ThemeCall::Get("get_search_query".into(), vec![Value::Bool(false)]);
    put!("get_search_query:raw".into(), raw);
    put!("max_num_pages".into(), get("max_num_pages", &[]));
    let previous = "<span class=\"meta-nav\">&larr;</span> Previous";
    put!(
        "previous_posts_link".into(),
        print("previous_posts_link", &[previous]),
    );
    let next = "Next <span class=\"meta-nav\">&rarr;</span>";
    put!("next_posts_link".into(), print("next_posts_link", &[next]));
    while theme.answer(&ThemeCall::HavePosts) == Ok(Value::Bool(true)) {
        theme.answer(&ThemeCall::ThePost).unwrap();
        let id = String::from_utf8(told(theme, get("get_the_ID", &[]))).unwrap();
        let at = |name: &str| format!("post:{id}:{name}");
        let title = print("the_title", &["<h1 class=\"entry-title\">", "</h1>"]);
        put!(at("the_title"), title);
        for name in ["get_the_title", "get_permalink"] {
            put!(at(name), get(name, &[]));
        }
        put!(at("the_permalink"), print("the_permalink", &[]));
        put!(at("post_class"), print("post_class", &["site-main"]));
        put!(at("get_post_type"), get("get_post_type", &[]));
        let post_type = String::from_utf8(told(theme, get("get_post_type", &[]))).unwrap();
        let supports = ThemeCall::Conditional(
            "post_type_supports".into(),
            vec![text(&post_type), text("comments")],
        );
        put!(at("post_type_supports:comments"), supports);
        let thumbnail = ThemeCall::Conditional("has_post_thumbnail".into(), Vec::new());
        put!(at("has_post_thumbnail"), thumbnail);
        let thumbnail = vec![Value::Null, text("large")];
        let thumbnail = ThemeCall::Get("get_the_post_thumbnail".into(), thumbnail);
        put!(at("get_the_post_thumbnail"), thumbnail);
        for name in ["the_excerpt", "the_content", "wp_link_pages"] {
            put!(at(name), print(name, &[]));
        }
        put!(
            at("has_tag"),
            ThemeCall::Conditional("has_tag".into(), Vec::new()),
        );
        let tags = ["<span class=\"tag-links\">Tagged ", ", ", "</span>"];
        put!(at("the_tags"), print("the_tags", &tags));
        let open = ThemeCall::Conditional("comments_open".into(), Vec::new());
        put!(at("comments_open"), open);
        out.push((at("comments_template"), hello_comments(theme)));
        let have = ThemeCall::Conditional("have_comments".into(), Vec::new());
        put!(at("have_comments"), have);
        put!(at("get_comments_number"), get("get_comments_number", &[]));
        let navigation = print("the_comments_navigation", &[]);
        put!(at("the_comments_navigation"), navigation);
        let args = array(&[
            ("style", text("ol")),
            ("short_ping", Value::Bool(true)),
            ("avatar_size", Value::Int(42)),
        ]);
        let list = ThemeCall::Print("wp_list_comments".into(), vec![args]);
        put!(at("wp_list_comments"), list);
        put!(at("comment_form"), comment_form());
    }
}

/// The tags of `reference/theme-tags/tags.php` over the page being
/// rendered, in its order; the loop's IDs are the request's posts it went
/// through.
fn tags(theme: &Theme, request: &Request) -> Vec<(String, Vec<u8>)> {
    let mut out = Vec::new();
    let names = [
        "is_home",
        "is_front_page",
        "is_singular",
        "is_single",
        "is_page",
        "is_attachment",
        "is_archive",
        "is_category",
        "is_tag",
        "is_tax",
        "is_author",
        "is_date",
        "is_year",
        "is_month",
        "is_day",
        "is_time",
        "is_search",
        "is_404",
        "is_paged",
        "is_preview",
        "is_feed",
        "is_embed",
        "is_privacy_policy",
        "is_page_template",
        "is_admin",
        "is_rtl",
        "in_the_loop",
    ];
    for name in names {
        let answer = told(theme, ThemeCall::Conditional(name.into(), Vec::new()));
        out.push((name.to_string(), answer));
    }
    for kind in ["page", "post"] {
        let call = ThemeCall::Conditional("is_singular".into(), vec![text(kind)]);
        out.push((format!("is_singular:{kind}"), told(theme, call)));
    }
    let mut ids = Vec::new();
    while theme.answer(&ThemeCall::HavePosts) == Ok(Value::Bool(true)) {
        theme.answer(&ThemeCall::ThePost).unwrap();
        let looping = theme.answer(&ThemeCall::InTheLoop) == Ok(Value::Bool(true));
        let id = request.posts[ids.len()].id;
        ids.push(format!("{id}{}", if looping { "" } else { "!" }));
    }
    out.push(("loop".into(), ids.join(",").into_bytes()));
    out.push((
        "in_the_loop:after".into(),
        told(theme, ThemeCall::InTheLoop),
    ));
    theme.answer(&ThemeCall::RewindPosts).unwrap();
    out.push((
        "have_posts:rewound".into(),
        told(theme, ThemeCall::HavePosts),
    ));
    theme.answer(&ThemeCall::ResetPostdata).unwrap();
    for name in ["language_attributes", "body_class", "wp_body_open"] {
        let printed = told(theme, ThemeCall::Print(name.into(), Vec::new()));
        out.push((name.into(), printed));
    }
    let logo = ThemeCall::Conditional("has_custom_logo".into(), Vec::new());
    out.push(("has_custom_logo".into(), told(theme, logo)));
    let logo = ThemeCall::Print("the_custom_logo".into(), Vec::new());
    out.push(("the_custom_logo".into(), told(theme, logo.clone())));
    for location in ["menu-1", "menu-2"] {
        let args = [
            ("theme_location", text(location)),
            ("fallback_cb", Value::Bool(false)),
            ("container", Value::Bool(false)),
            ("echo", Value::Bool(false)),
        ];
        let args = args.map(|(key, value)| (Key::String(key.as_bytes().to_vec()), value));
        let call = ThemeCall::Get("wp_nav_menu".into(), vec![Value::Array(args.to_vec())]);
        let menu = match theme.answer(&call) {
            Ok(Value::String(markup)) => [&b"markup:"[..], &markup].concat(),
            other => told_answer(other),
        };
        out.push((format!("wp_nav_menu:{location}"), menu));
    }
    for part in ["header", "footer"] {
        out.push((format!("the_custom_logo:{part}"), told(theme, logo.clone())));
    }
    more_tags(theme, &mut out);
    out
}

fn told_answer(answer: Result<Value, String>) -> Vec<u8> {
    match answer {
        Ok(Value::Bool(false)) => b"false".to_vec(),
        other => format!("{other:?}").into_bytes(),
    }
}

/// Hello's template tags answer on each page of
/// `reference/pages/default.list` as PHP answers them where its template
/// loader has picked the template, over the default install and with a
/// custom logo and menus, and on the author archives of
/// `reference/theme-tags/authors.list` with author archives on.
#[tokio::test(flavor = "multi_thread")]
async fn hello_s_tags_answer_as_php() {
    let _saves = crate::admin::tests::SAVES.lock().await;
    let content = with_content().await;
    let mut sites: HashMap<String, Arc<Site>> = HashMap::new();
    let (mut failures, mut pages) = (Vec::new(), 0);
    let mut lines = GOLDEN.lines().peekable();
    while let Some(line) = lines.next() {
        let fields: Vec<&str> = line.strip_prefix("page\t").unwrap().split('\t').collect();
        let mut expected = Vec::new();
        while let Some(tag) = lines.peek().and_then(|l| l.strip_prefix("tag\t")) {
            let (name, value) = tag.split_once('\t').unwrap();
            lines.next();
            let mut value = hyscribe_testkit::unhex(value);
            if name == "body_class" {
                let class = String::from_utf8(value).unwrap();
                value = class.replace(HELLO_CLASS, "").into_bytes();
            }
            expected.push((name.to_string(), value));
        }
        let path = String::from_utf8(hyscribe_testkit::unhex(fields[1])).unwrap();
        if expected.is_empty() {
            continue;
        }
        let site = match sites.get(fields[0]) {
            Some(site) => site.clone(),
            None => {
                let config = if fields[0].starts_with("_tags=content") {
                    content.clone()
                } else if fields[0].starts_with("_tags=authors") {
                    // The scratch database again, its content's pages done.
                    content_db(include_str!("../../../../../reference/pages/authors.sql")).await
                } else {
                    hyscribe_testkit::fixture_database()
                };
                let site = site_on(&config, fields[0]).await;
                sites.insert(fields[0].into(), site.clone());
                site
            }
        };
        let Some((request, _, _)) = page_of(&site, &path).await else {
            failures.push(format!("{} {path}: no page", fields[0]));
            continue;
        };
        let got = tokio::task::spawn_blocking(move || {
            let theme = Theme::default();
            let support = ThemeCall::AddSupport("responsive-embeds".into(), Vec::new());
            assert_eq!(theme.answer(&support), Ok(Value::Null));
            assert_eq!(theme.answer(&support), Ok(Value::Null));
            let support = ThemeCall::AddSupport("post-thumbnails".into(), Vec::new());
            assert_eq!(theme.answer(&support), Ok(Value::Null));
            theme.begin((&site, &request), &hello_files(), ("", None));
            let got = tags(&theme, &request);
            assert!(theme.end(Ok(Vec::new())).is_ok());
            got
        })
        .await
        .unwrap();
        for ((name, php), (_, ours)) in expected.iter().zip(&got) {
            if php != ours {
                failures.push(format!(
                    "{} {path} {name}:\n  php:  {}\n  ours: {}",
                    fields[0],
                    String::from_utf8_lossy(php),
                    String::from_utf8_lossy(ours)
                ));
            }
        }
        assert_eq!(expected.len(), got.len(), "{path}");
        pages += 1;
    }
    assert_eq!(pages, 143);
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

/// The page `path` of the default install with Hello active.
async fn hello_page(path: &str) -> (Arc<Site>, Request) {
    let options = "template=hello-elementor&stylesheet=hello-elementor";
    let site = site_on(&hyscribe_testkit::fixture_database(), options).await;
    let (request, _, _) = page_of(&site, path).await.unwrap();
    (site, request)
}

/// `get_header()` and the other part functions find `{slug}-{name}.php`,
/// then `{slug}.php`, among the theme's files, as `locate_template()` does.
#[tokio::test(flavor = "multi_thread")]
async fn parts_are_found_as_locate_template_finds_them() {
    let (site, request) = hello_page("/").await;
    let answers = tokio::task::spawn_blocking(move || {
        let theme = Theme::default();
        let mut files = hello_files();
        files.push(TemplateFile {
            file: "header-wide.php".into(),
            name: None,
            post_types: Vec::new(),
        });
        theme.begin((&site, &request), &files, ("", None));
        let parts = [
            (Part::Header, "header", None),
            (Part::Header, "header", Some("wide")),
            (Part::Header, "header", Some("x")),
            (Part::Footer, "footer", Some("")),
            (Part::Sidebar, "sidebar", None),
            (Part::TemplatePart, "template-parts/single", Some("x")),
            (Part::TemplatePart, "template-parts/none", None),
        ];
        let (answers, fired) = hyscribe_testkit::hooks::recorded(&[], || {
            parts.map(|(kind, slug, name)| {
                let name = name.map(Into::into);
                theme.answer(&ThemeCall::Part(
                    kind,
                    slug.into(),
                    name,
                    vec![Value::Int(1)],
                ))
            })
        });
        // No menu falls back to `wp_page_menu()`, which is not ported.
        let menu = theme.answer(&ThemeCall::Get("wp_nav_menu".into(), Vec::new()));
        assert_eq!(menu, Err(UNSERVED.into()));
        let none = theme.answer(&ThemeCall::Conditional("is_none".into(), Vec::new()));
        assert_eq!(none, Err(UNSERVED.into()));
        assert!(theme.end(Ok(Vec::new())).is_ok());
        assert_eq!(theme.answer(&ThemeCall::ThePost), Err(NO_PAGE.into()));
        (answers, fired)
    })
    .await
    .unwrap();
    let (answers, fired) = answers;
    let names: Vec<&str> = fired.iter().map(|(hook, _, _)| hook.as_str()).collect();
    let single = "get_template_part_template-parts/single";
    assert_eq!(
        names,
        [
            "get_header",
            "get_header",
            "get_header",
            "get_footer",
            "get_sidebar",
            single,
            "get_template_part",
            "get_template_part_template-parts/none",
            "get_template_part",
        ]
    );
    let args = Value::Array(vec![(Key::Int(0), Value::Int(1))]);
    assert_eq!(fired[1].2, [text("wide"), args.clone()]);
    assert_eq!(fired[3].2, [Value::Null, args.clone()]);
    let templates = ["template-parts/single-x.php", "template-parts/single.php"];
    let templates = templates
        .iter()
        .enumerate()
        .map(|(at, t)| (Key::Int(at as i64), text(t)));
    let slug = text("template-parts/single");
    assert_eq!(fired[5].2, [slug.clone(), text("x"), args.clone()]);
    assert_eq!(
        fired[6].2,
        [slug, text("x"), Value::Array(templates.collect()), args]
    );
    let file = |name: &str| Ok(text(name));
    assert_eq!(
        answers,
        [
            file("header.php"),
            file("header-wide.php"),
            file("header.php"),
            file("footer.php"),
            Ok(Value::Null),
            file("template-parts/single.php"),
            Ok(Value::Null),
        ]
    );
}

/// A tag's database error fails the page, as a theme's fault does, and
/// a tag that is not served says so.
#[tokio::test(flavor = "multi_thread")]
async fn a_page_fails_with_its_first_database_error() {
    let (site, request) = hello_page("/").await;
    let mut broken = Arc::try_unwrap(site).ok().unwrap();
    let mut config = hyscribe_testkit::fixture_database();
    config.prefix = hyscribe_config::Prefix::new("missing_").unwrap();
    broken.db = hyscribe_db::Db::connect(&config).await.unwrap();
    let broken = Arc::new(broken);
    let (printed, unserved, ended) = tokio::task::spawn_blocking(move || {
        let theme = Theme::default();
        let global = hyscribe_render::theme::classic_global_styles(HELLO_THEME_JSON, None);
        theme.begin((&broken, &request), &[], ("", Some(Arc::new(global))));
        // The head reads `current_theme`, which the site has not loaded.
        let head = theme.answer(&ThemeCall::Print("wp_head".into(), Vec::new()));
        assert!(matches!(head, Ok(Value::String(_))));
        // The logo reads `site_logo`, which the site has not loaded.
        let printed = theme.answer(&ThemeCall::Print("the_custom_logo".into(), Vec::new()));
        let unserved = theme.answer(&ThemeCall::Get("bytes".into(), Vec::new()));
        (printed, unserved, theme.end(Ok(Vec::new())))
    })
    .await
    .unwrap();
    assert_eq!(printed, Ok(Value::String(Vec::new())));
    assert_eq!(unserved, Err(UNSERVED.into()));
    let failed = ended.unwrap_err();
    assert!(matches!(failed, Failed::Database(_)), "{failed:?}");
    assert!(failed.to_string().contains("missing_"), "{failed}");
    let fault = Failed::Theme(hyscribe_ext::Fault::Timeout);
    assert_eq!(fault.to_string(), "the theme failed: Timeout");
}

/// The front end's answer to `uri`, over the site as the database has it.
async fn served(site: &Arc<Site>, uri: &str) -> axum::response::Response {
    use tower::ServiceExt;
    let request = axum::http::Request::builder()
        .uri(uri)
        .header(axum::http::header::HOST, "127.0.0.1:8081")
        .body(axum::body::Body::empty())
        .unwrap();
    crate::live(site.clone()).oneshot(request).await.unwrap()
}

/// The front end renders the active classic theme's template through its
/// component, which asks the host for the page's tags; a template that
/// fails answers the error page.
#[tokio::test(flavor = "multi_thread")]
async fn the_front_end_renders_the_classic_theme_s_template() {
    let _saves = crate::admin::tests::SAVES.lock().await;
    let db = with_content().await;
    let options = sqlx::mysql::MySqlConnectOptions::new()
        .host(&db.host)
        .port(db.port)
        .database(&db.name)
        .username(&db.user)
        .password(db.password.expose());
    let mut conn = sqlx::MySqlConnection::connect_with(&options).await.unwrap();
    let active = "UPDATE wp_options SET option_value = 'hyscribe-theme' \
        WHERE option_name IN ('template', 'stylesheet')";
    conn.execute(active).await.unwrap();
    // The site's own global styles for the theme, which add nothing.
    let user = "INSERT INTO wp_posts (ID, post_author, post_date, post_date_gmt, post_content, post_title, \
        post_excerpt, post_status, comment_status, ping_status, post_password, post_name, to_ping, pinged, \
        post_modified, post_modified_gmt, post_content_filtered, post_parent, guid, menu_order, post_type, \
        post_mime_type, comment_count) VALUES (9701, 1, '2026-11-01 10:00:00', '2026-11-01 10:00:00', \
        '{\"version\": 3, \"isGlobalStylesUserThemeJSON\": true}', 'Custom Styles', '', 'publish', 'closed', \
        'closed', '', 'wp-global-styles-hyscribe-theme', '', '', '2026-11-01 10:00:00', '2026-11-01 10:00:00', \
        '', 0, '', 0, 'wp_global_styles', '', 0); \
        INSERT INTO wp_terms (term_id, name, slug) VALUES (9701, 'hyscribe-theme', 'hyscribe-theme'); \
        INSERT INTO wp_term_taxonomy (term_taxonomy_id, term_id, taxonomy, description, count) \
        VALUES (9701, 9701, 'wp_theme', '', 1); \
        INSERT INTO wp_term_relationships (object_id, term_taxonomy_id) VALUES (9701, 9701)";
    conn.execute(sqlx::raw_sql(user)).await.unwrap();
    let dir = tempfile::tempdir().unwrap();
    let (plugins, themes) = (dir.path().join("plugins"), dir.path().join("themes"));
    let folder = themes.join("hyscribe-theme");
    std::fs::create_dir_all(&folder).unwrap();
    std::fs::create_dir_all(&plugins).unwrap();
    let manifest = include_str!("../../../../hyscribe-ext/guest/theme/hyscribe.ext.toml");
    std::fs::write(folder.join(hyscribe_ext::manifest::FILE), manifest).unwrap();
    let component = crate::extensions::tests::guest::theme();
    std::fs::write(folder.join(hyscribe_ext::hyx::COMPONENT), component).unwrap();
    let found = hyscribe_ext::installed::theme(&themes, "hyscribe-theme", "hyscribe-theme");
    hyscribe_ext::installed::approve(&found.unwrap()).unwrap();
    let site = site_on(&db, "").await;
    let site = Arc::try_unwrap(site).ok().unwrap();
    let writer = hyscribe_db::Writer::open(&db, hyscribe_config::Mode::Primary).await;
    let site = site.with_writer(writer.unwrap());
    let site = Arc::new(site.with_extensions(&plugins, &themes));
    let running = crate::extensions::request(&site, false).unwrap();
    assert_eq!(format!("{running:?}"), "Running");
    let response = served(&site, "/").await;
    assert_eq!(response.status(), 200);
    let page = crate::admin::tests::page_of(response).await;
    let classes = "home blog wp-theme-hyscribe-theme";
    assert_eq!(page, format!("<body class=\"{classes}\">front</body>"));
    // A theme that faults answers PHP's critical error page.
    let fatal = GOLDEN_HEAD
        .lines()
        .last()
        .unwrap()
        .split('\t')
        .nth(4)
        .unwrap();
    let fatal = String::from_utf8(hyscribe_testkit::unhex(fatal)).unwrap();
    let response = served(&site, "/?p=1").await;
    assert_eq!(response.status(), 500);
    assert_eq!(crate::admin::tests::page_of(response).await, fatal);
    // `index.php`'s head needs the theme's `theme.json`.
    let response = served(&site, "/?page_id=2").await;
    assert_eq!(response.status(), 500);
    let public = folder.join("public");
    std::fs::create_dir_all(&public).unwrap();
    std::fs::write(public.join("theme.json"), HELLO_THEME_JSON).unwrap();
    let mods =
        "SELECT option_value FROM wp_options WHERE option_name = 'theme_mods_hyscribe-theme'";
    let named = "INSERT INTO wp_options (option_name, option_value, autoload) \
        VALUES ('current_theme', 'HyScribe Theme', 'auto')";
    for visit in 0..3 {
        // Without `current_theme`, the theme's name is its headers'.
        if visit == 1 {
            conn.execute(named).await.unwrap();
        }
        let response = served(&site, "/?page_id=2").await;
        assert_eq!(response.status(), 200);
        let page = crate::admin::tests::page_of(response).await;
        let style = "<link rel='stylesheet' id='hyscribe-theme-css' \
            href='http://127.0.0.1:8081/wp-content/themes/hyscribe-theme/style.css?ver=1.0' media='all' />\n";
        let reply = "<script async data-wp-strategy=\"async\" fetchpriority=\"low\" id=\"comment-reply-js\"";
        // The style enqueued in the footer, moved into the head.
        let late = "late.css?ver=7.1.3' media='all' />\n\n</head>";
        assert!(page.contains(late), "{page}");
        // Not the title tag; feed links from its `after_setup_theme`.
        assert!(!page.contains("<title>") && page.contains("application/rss+xml"));
        for printed in [
            style,
            "<meta name=\"theme\">\n",
            "<style id=\"global-styles-inline-css\">",
            reply,
        ] {
            assert!(page.contains(printed), "{printed} in {page}");
        }
        // The head's write, once, as PHP makes it.
        let written: Vec<String> = sqlx::query_scalar(mods).fetch_all(&mut conn).await.unwrap();
        let kept = "a:1:{s:18:\"custom_css_post_id\";i:-1;}";
        assert_eq!(written, [kept][..visit.min(1)], "{visit}");
    }
    // The site's own styles changed, the global styles are built again.
    let styled = "UPDATE wp_posts SET post_content = '{\"version\": 3, \
        \"isGlobalStylesUserThemeJSON\": true, \"styles\": {\"color\": {\"text\": \"#123456\"}}}' \
        WHERE ID = 9701";
    conn.execute(styled).await.unwrap();
    let page = crate::admin::tests::page_of(served(&site, "/?page_id=2").await).await;
    assert!(page.contains("#123456"), "{page}");
}

/// PHP's heads and footers, `reference/classic-head/golden.sh`.
const GOLDEN_HEAD: &str = include_str!("../../../tests/golden/classic-head-hello-elementor.tsv");

/// Hello Elementor 3.5.2's `theme.json`, as its package carries it.
const HELLO_THEME_JSON: &[u8] = include_bytes!("../../../tests/hello-elementor-theme.json");

/// What Hello's port does in the head: its three stylesheets enqueued in
/// `wp_enqueue_scripts`, and its description meta tag printed in `wp_head`
/// through the host's tags, for a post shown on its own with an excerpt.
struct HelloPort(Arc<Theme>);

impl HelloPort {
    fn singular(&self) -> bool {
        told(
            &self.0,
            ThemeCall::Conditional("is_singular".into(), vec![]),
        ) == b"true"
    }
}

impl hyscribe_model::hooks::Dispatch for HelloPort {
    fn attached(&self, hook: &str) -> bool {
        ["wp_enqueue_scripts", "wp_head"].contains(&hook)
    }

    fn filter(&self, _: &str, value: Value, _: &[Value]) -> Value {
        value
    }

    fn action(&self, _: &str, _: &[Value]) {}

    fn printing(
        &self,
        hook: &str,
        _: &[Value],
        core: &mut dyn FnMut(Option<i32>) -> Vec<u8>,
    ) -> Vec<u8> {
        let mut printed = core(Some(10));
        if hook == "wp_enqueue_scripts" {
            for (handle, file) in [
                ("hello-elementor", "reset"),
                ("hello-elementor-theme-style", "theme"),
                ("hello-elementor-header-footer", "header-footer"),
            ] {
                let asset = Asset {
                    kind: AssetKind::Style,
                    handle: handle.into(),
                    src: format!("assets/css/{file}.css"),
                    deps: Vec::new(),
                    version: Some("3.5.2".into()),
                    placement: "all".into(),
                };
                self.0.register_asset("hello-elementor", &asset).unwrap();
                self.0.enqueue(AssetKind::Style, handle).unwrap();
            }
        } else if self.singular() {
            let get = ThemeCall::Get("get_queried_object_id".into(), Vec::new());
            let field = vec![text("post_excerpt"), self.0.answer(&get).unwrap()];
            let excerpt = told(&self.0, ThemeCall::Print("get_post_field".into(), field));
            let strip = ThemeCall::Print("wp_strip_all_tags".into(), vec![Value::String(excerpt)]);
            let excerpt = told(&self.0, strip);
            if !excerpt.is_empty() {
                let content = hyscribe_kses::esc_attr(&excerpt);
                printed.extend(
                    format!("<meta name=\"description\" content=\"{content}\">\n").into_bytes(),
                );
            }
        }
        printed.extend(core(None));
        printed
    }
}

/// The features Hello adds in `after_setup_theme`.
const HELLO_SUPPORTS: [&str; 9] = [
    "post-thumbnails",
    "automatic-feed-links",
    "title-tag",
    "html5",
    "custom-logo",
    "align-wide",
    "responsive-embeds",
    "editor-styles",
    "woocommerce",
];

/// Hello's head and footer, over a template of the two only, print as PHP
/// prints them on each page of `reference/pages/default.list` where its
/// template loader is reached, and with a post that has an excerpt.
#[tokio::test(flavor = "multi_thread")]
async fn hello_s_head_and_footer_print_as_php() {
    let _saves = crate::admin::tests::SAVES.lock().await;
    let content = content_db(include_str!(
        "../../../../../reference/classic-head/content.sql"
    ))
    .await;
    let global = Arc::new(hyscribe_render::theme::classic_global_styles(
        HELLO_THEME_JSON,
        None,
    ));
    let mut sites: HashMap<String, Arc<Site>> = HashMap::new();
    let (mut failures, mut pages) = (Vec::new(), 0);
    for line in GOLDEN_HEAD.lines() {
        let fields: Vec<&str> = line.strip_prefix("page\t").unwrap().split('\t').collect();
        let path = String::from_utf8(hyscribe_testkit::unhex(fields[1])).unwrap();
        if !["200", "404"].contains(&fields[2]) || path.contains("author=") {
            continue;
        }
        let site = match sites.get(fields[0]) {
            Some(site) => site.clone(),
            None => {
                let config = if fields[0].starts_with("_head=content") {
                    content.clone()
                } else {
                    hyscribe_testkit::fixture_database()
                };
                let site = site_on(&config, fields[0]).await;
                sites.insert(fields[0].into(), site.clone());
                site
            }
        };
        let Some((request, _, title)) = page_of(&site, &path).await else {
            continue;
        };
        let global = global.clone();
        let ours = tokio::task::spawn_blocking(move || {
            let theme = Arc::new(Theme::default());
            for feature in HELLO_SUPPORTS {
                theme
                    .answer(&ThemeCall::AddSupport(feature.into(), Vec::new()))
                    .unwrap();
            }
            theme.begin((&site, &request), &hello_files(), (&title, Some(global)));
            let port: Arc<dyn hyscribe_model::hooks::Dispatch> = Arc::new(HelloPort(theme.clone()));
            let page = hyscribe_model::hooks::scoped(&port, || {
                let tag = |name: &str| told(&theme, ThemeCall::Print(name.into(), Vec::new()));
                [
                    &b"<head>\n"[..],
                    &tag("wp_head"),
                    b"</head>\n<body>\n",
                    &tag("wp_footer"),
                    b"</body>\n",
                ]
                .concat()
            });
            String::from_utf8(theme.end(Ok(page)).unwrap()).unwrap()
        })
        .await
        .unwrap();
        let php = String::from_utf8(hyscribe_testkit::unhex(fields[3])).unwrap();
        let php = crate::tests::as_hyscribe(php);
        if php != ours {
            failures.push(format!(
                "{} {path}:\n{}",
                fields[0],
                crate::tests::first_difference(&php, &ours)
            ));
        }
        pages += 1;
    }
    assert_eq!(pages, 68);
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

/// A stylesheet for every medium with no dependencies is served, from the
/// folder of the extension that registered it; a handle no one registered
/// prints nothing, as in PHP; only a page enqueues.
#[tokio::test(flavor = "multi_thread")]
async fn styles_are_served_as_far_as_ported() {
    let (site, request) = hello_page("/").await;
    tokio::task::spawn_blocking(move || {
        let theme = Theme::default();
        let style = |handle: &str| Asset {
            kind: AssetKind::Style,
            handle: handle.into(),
            src: "a.css".into(),
            deps: Vec::new(),
            version: None,
            placement: String::new(),
        };
        let head = ThemeCall::Print("wp_head".into(), Vec::new());
        assert_eq!(theme.enqueue(AssetKind::Style, "a"), Err(NO_PAGE.into()));
        assert_eq!(theme.answer(&head), Err(NO_PAGE.into()));
        let refused = [
            Asset {
                kind: AssetKind::Script,
                ..style("s")
            },
            Asset {
                deps: vec!["d".into()],
                ..style("d")
            },
            Asset {
                placement: "print".into(),
                ..style("p")
            },
        ];
        for asset in refused {
            assert_eq!(theme.register_asset("x", &asset), Err(UNSERVED.into()));
        }
        theme.register_asset("a-plugin", &style("a")).unwrap();
        theme.begin((&site, &request), &[], ("", None));
        // Without the theme's `theme.json`, its head is not served.
        assert_eq!(theme.answer(&head), Err(UNSERVED.into()));
        let global = hyscribe_render::theme::classic_global_styles(HELLO_THEME_JSON, None);
        theme.begin((&site, &request), &[], ("", Some(Arc::new(global))));
        theme.enqueue(AssetKind::Style, "a").unwrap();
        theme.enqueue(AssetKind::Style, "unknown").unwrap();
        let Ok(Value::String(printed)) = theme.answer(&head) else {
            panic!("no head");
        };
        let printed = String::from_utf8(printed).unwrap();
        let href = "href='http://127.0.0.1:8081/wp-content/plugins/a-plugin/a.css?ver=7.1.3'";
        assert!(printed.contains(href), "{printed}");
        assert!(!printed.contains("unknown"));
        // The title tag as the theme supports it, and no feed links.
        assert!(!printed.contains("<title>"));
        let support = ThemeCall::AddSupport("title-tag".into(), Vec::new());
        theme.answer(&support).unwrap();
        let Ok(Value::String(printed)) = theme.answer(&head) else {
            panic!("no head");
        };
        let printed = String::from_utf8(printed).unwrap();
        assert!(printed.contains("<title></title>") && !printed.contains("application/rss+xml"));
        // `wp_head()` takes no arguments, and a page that ended is gone.
        let given = ThemeCall::Print("wp_head".into(), vec![Value::Int(1)]);
        assert_eq!(theme.answer(&given), Err(UNSERVED.into()));
        assert!(theme.end(Ok(Vec::new())).is_ok());
        assert_eq!(theme.end(Ok(b"x".to_vec())).unwrap(), b"x");
    })
    .await
    .unwrap();
}

/// Hello's setup calls are taken as PHP takes them: `register_nav_menus()`
/// adds `menus` and `add_editor_style()` adds `editor-style`, once each;
/// `is_admin()` answers outside a page, true for an admin request.
#[test]
fn setup_calls_are_taken_as_php_takes_them() {
    let theme = Theme::default();
    let calls = [
        ThemeCall::NavMenus(vec![("menu-1".into(), "Header".into())]),
        ThemeCall::NavMenus(vec![("menu-2".into(), "Footer".into())]),
        ThemeCall::EditorStyle(vec!["assets/css/editor-styles.css".into()]),
        ThemeCall::ContentWidth(800),
    ];
    for call in calls {
        assert_eq!(theme.answer(&call), Ok(Value::Null));
    }
    assert_eq!(lock(&theme.0).supports, ["menus", "editor-style"]);
    let admin = ThemeCall::Conditional("is_admin".into(), Vec::new());
    assert_eq!(theme.answer(&admin), Ok(Value::Bool(false)));
    assert_eq!(Theme::new(true).answer(&admin), Ok(Value::Bool(true)));
    let given = ThemeCall::Conditional("is_admin".into(), vec![Value::Int(1)]);
    assert_eq!(Theme::new(true).answer(&given), Err(NO_PAGE.into()));
}

/// The tags Hello's description meta tag reads: the queried post's ID and
/// excerpt, the one field and post served, and `wp_strip_all_tags()`.
#[tokio::test(flavor = "multi_thread")]
async fn the_description_s_tags_answer() {
    let (site, request) = hello_page("/?p=1").await;
    let (home_site, home) = hello_page("/").await;
    tokio::task::spawn_blocking(move || {
        let theme = Theme::default();
        theme.begin((&site, &request), &[], ("", None));
        let get = |name: &str, args: Vec<Value>| theme.answer(&ThemeCall::Get(name.into(), args));
        let print =
            |name: &str, args: Vec<Value>| theme.answer(&ThemeCall::Print(name.into(), args));
        assert_eq!(get("get_queried_object_id", Vec::new()), Ok(Value::Int(1)));
        let unserved = Err(UNSERVED.to_string());
        assert_eq!(get("get_queried_object_id", vec![Value::Int(1)]), unserved);
        let excerpt = Ok(Value::String(request.posts[0].excerpt.clone()));
        let field = |name: &str, id: i64| vec![text(name), Value::Int(id)];
        assert_eq!(print("get_post_field", field("post_excerpt", 1)), excerpt);
        assert_eq!(print("get_post_field", field("post_excerpt", 2)), unserved);
        assert_eq!(print("get_post_field", field("post_title", 1)), unserved);
        assert_eq!(
            print("get_post_field", vec![text("post_excerpt")]),
            unserved
        );
        let stripped = print("wp_strip_all_tags", vec![text(" <b>a</b>\n")]);
        assert_eq!(stripped, Ok(text("a")));
        assert_eq!(print("wp_strip_all_tags", Vec::new()), unserved);
        // On a page that shows no post on its own, no post's field.
        theme.begin((&home_site, &home), &[], ("", None));
        let first = home.posts[0].id as i64;
        assert_eq!(
            print("get_post_field", field("post_excerpt", first)),
            unserved
        );
    })
    .await
    .unwrap();
}

/// What `call` answers on the page `path` of `site`, Hello's files listed
/// but for `comments.php` when `comments` is false, `post-thumbnails`
/// supported when `thumbnails`.
async fn on_page(
    site: &Arc<Site>,
    path: &str,
    flags: (bool, bool),
    calls: Vec<ThemeCall>,
) -> Vec<Vec<u8>> {
    let (request, _, _) = page_of(site, path).await.unwrap();
    on_request(site, request, flags, calls).await
}

/// What `calls` answer over `request` of `site`, as [`on_page`].
async fn on_request(
    site: &Arc<Site>,
    request: Request,
    (comments, thumbnails): (bool, bool),
    calls: Vec<ThemeCall>,
) -> Vec<Vec<u8>> {
    let site = site.clone();
    tokio::task::spawn_blocking(move || {
        let theme = Theme::default();
        if thumbnails {
            let support = ThemeCall::AddSupport("post-thumbnails".into(), Vec::new());
            theme.answer(&support).unwrap();
        }
        let mut files = hello_files();
        files.retain(|file| comments || file.file != "comments.php");
        theme.begin((&site, &request), &files, ("", None));
        let answers = calls.into_iter().map(|call| told(&theme, call)).collect();
        assert!(theme.end(Ok(Vec::new())).is_ok());
        answers
    })
    .await
    .unwrap()
}

/// `is_author()` naming an author is not ported on an author's archive,
/// and elsewhere is false.
#[tokio::test(flavor = "multi_thread")]
async fn is_author_naming_an_author_is_refused() {
    let options = "template=hello-elementor&stylesheet=hello-elementor";
    let site = site_on(&hyscribe_testkit::fixture_database(), options).await;
    let unserved = format!("{:?}", Err::<Value, String>(UNSERVED.into())).into_bytes();
    let named = || ThemeCall::Conditional("is_author".into(), vec![text("admin")]);
    let answers = on_page(&site, "/?author=1", (true, false), vec![named()]).await;
    assert_eq!(answers, [unserved]);
    let answers = on_page(&site, "/?cat=1", (true, false), vec![named()]).await;
    assert_eq!(answers, [b"false".to_vec()]);
}

/// The tags' arguments and the states the pages of `tags.php` do not
/// reach: what is not ported answers not served.
#[tokio::test(flavor = "multi_thread")]
async fn tags_refuse_what_is_not_ported() {
    let _saves = crate::admin::tests::SAVES.lock().await;
    let db = content_db(
        "UPDATE wp_term_taxonomy SET description = 'About' WHERE term_taxonomy_id = 1;
        UPDATE wp_posts SET post_password = 'x' WHERE ID = 1;",
    )
    .await;
    let options = "template=hello-elementor&stylesheet=hello-elementor&blog_charset=";
    let site = site_on(&db, options).await;
    let unserved = format!("{:?}", Err::<Value, String>(UNSERVED.into())).into_bytes();
    let conditional = |name: &str, args: Vec<Value>| ThemeCall::Conditional(name.into(), args);
    let supports = |post_type: &str| {
        conditional(
            "post_type_supports",
            vec![text(post_type), text("comments")],
        )
    };
    let raw = |name: &str, args: Vec<Value>| ThemeCall::Print(name.into(), args);
    let list = Value::Array(vec![(Key::Int(0), text("x"))]);
    let answers = on_page(
        &site,
        "/?cat=1",
        (true, false),
        vec![
            supports("attachment"),
            supports("thing"),
            conditional("post_type_supports", Vec::new()),
            conditional("has_post_thumbnail", vec![Value::Int(1)]),
            print("the_archive_description", &[]),
            print("post_class", &[]),
            raw(
                "post_class",
                vec![Value::Array(vec![(
                    Key::Int(0),
                    text("has-post-thumbnail"),
                )])],
            ),
            print("the_tags", &[]),
            raw("wp_list_comments", vec![array(&[("style", text("div"))])]),
            raw("wp_list_comments", vec![array(&[("walker", text("x"))])]),
            raw("comment_form", vec![list.clone()]),
            raw("next_posts_link", vec![Value::Null, Value::Int(1)]),
            print("the_comments_navigation", &[]),
            print("the_none", &[]),
            get("get_bloginfo", &["charset"]),
            get("get_bloginfo", &["text_direction"]),
            get("get_bloginfo", &["admin_email"]),
            ThemeCall::Get("home_url".into(), vec![Value::Int(1)]),
            ThemeCall::Get("get_the_post_thumbnail".into(), Vec::new()),
            print("comments_template", &[]),
        ],
    )
    .await;
    let expected: [&[u8]; 20] = [
        b"true",
        b"false",
        &unserved,
        &unserved,
        &unserved,
        b"class=\"post-1 post type-post status-publish format-standard post-password-required hentry category-uncategorized\"",
        b"class=\"has-post-thumbnail post-1 post type-post status-publish format-standard post-password-required hentry category-uncategorized\"",
        b"",
        &unserved,
        &unserved,
        &unserved,
        b"",
        b"",
        &unserved,
        b"UTF-8",
        b"ltr",
        &unserved,
        &unserved,
        b"",
        &unserved,
    ];
    for (at, (ours, wanted)) in answers.iter().zip(expected).enumerate() {
        assert_eq!(
            String::from_utf8_lossy(ours),
            String::from_utf8_lossy(wanted),
            "{at}"
        );
    }
    let answers = on_page(
        &site,
        "/?p=1",
        (false, true),
        vec![
            print("comments_template", &[]),
            raw("the_tags", vec![Value::Null]),
            raw("wp_list_comments", vec![array(&[("style", text("ul"))])]),
            raw("comment_form", Vec::new()),
        ],
    )
    .await;
    assert_eq!(answers[0], unserved);
    assert_eq!(answers[1], b"");
    assert_eq!(answers[2], b"");
    let answers = on_page(
        &site,
        "/?p=1",
        (true, true),
        vec![print("comments_template", &[])],
    )
    .await;
    assert_eq!(answers[0], unserved);
    let answers = on_page(
        &site,
        "/?p=3",
        (true, true),
        vec![
            get("get_the_ID", &[]),
            conditional("have_comments", Vec::new()),
        ],
    )
    .await;
    assert_eq!(answers, [b"false".to_vec(), b"false".to_vec()]);
    // Without a global post, or showing what is not served.
    let mut calls: Vec<ThemeCall> = [
        "the_title",
        "the_permalink",
        "post_class",
        "the_content",
        "the_excerpt",
        "wp_link_pages",
        "the_tags",
        "comments_template",
        "comment_form",
    ]
    .iter()
    .map(|name| print(name, &[]))
    .collect();
    for name in [
        "get_the_title",
        "get_permalink",
        "get_post_type",
        "get_comments_number",
        "get_the_post_thumbnail",
    ] {
        calls.push(get(name, &[]));
    }
    calls.push(conditional("has_post_thumbnail", Vec::new()));
    calls.push(print("bloginfo", &["admin_email"]));
    calls.push(get("get_bloginfo", &["version", "display"]));
    let count = calls.len();
    let answers = on_page(&site, "/?p=3", (true, true), calls).await;
    assert_eq!(answers, vec![unserved.clone(); count]);
    // A theme without `comments.php` gets core's compat template, not ported.
    let plain = site_on(&hyscribe_testkit::fixture_database(), options).await;
    let template = vec![print("comments_template", &[])];
    let answers = on_page(&plain, "/?p=1", (false, true), template.clone()).await;
    assert_eq!(answers, [unserved]);
    assert_eq!(
        on_page(&plain, "/?p=1", (true, true), template).await,
        [b""]
    );
    // `wp_reset_postdata()` makes the main query's first post global again.
    let content = site_on(&with_content().await, options).await;
    let id = || get("get_the_ID", &[]);
    let calls = vec![
        ThemeCall::ThePost,
        ThemeCall::ThePost,
        id(),
        ThemeCall::ResetPostdata,
        id(),
    ];
    let answers = on_page(&content, "/", (true, true), calls).await;
    assert_eq!((&answers[2][..], &answers[4][..]), (&b"964"[..], &b"1"[..]));
}

/// `get_pagenum_link()` under pretty permalinks keeps the query and drops
/// `index.php`; under `index.php` permalinks it is not ported.
#[tokio::test(flavor = "multi_thread")]
async fn page_links_follow_pretty_permalinks() {
    let config = hyscribe_testkit::fixture_database();
    let previous = || print("previous_posts_link", &["p"]);
    let request = |uri: &str| Request {
        uri: uri.as_bytes().to_vec(),
        paged: 3,
        max_pages: 5,
        ..Request::default()
    };
    let options =
        "template=hello-elementor&stylesheet=hello-elementor&permalink_structure=%2F%25postname%25";
    let site = site_on(&config, options).await;
    let answers = on_request(
        &site,
        request("/index.php/page/3?s=a&paged=3"),
        (true, true),
        vec![previous(), print("next_posts_link", &["n"])],
    )
    .await;
    assert_eq!(
        answers,
        [
            b"<a href=\"http://127.0.0.1:8081/page/2?s=a\" >p</a>".to_vec(),
            b"<a href=\"http://127.0.0.1:8081/page/4?s=a\" >n</a>".to_vec(),
        ]
    );
    let answers = on_request(
        &site,
        request("/category/x/page/3/"),
        (true, true),
        vec![previous()],
    )
    .await;
    let path = "http://127.0.0.1:8081/category/x/page/2";
    assert_eq!(answers, [format!("<a href=\"{path}\" >p</a>").into_bytes()]);
    // On the last page but one the next link is still printed, and on the
    // last page it is not (`$nextpage <= $max_page`).
    let near = |paged| Request {
        uri: format!("/index.php/page/{paged}").into_bytes(),
        paged,
        max_pages: 5,
        ..Request::default()
    };
    let next = || print("next_posts_link", &["n"]);
    let answers = on_request(&site, near(4), (true, true), vec![next()]).await;
    let href = "http://127.0.0.1:8081/page/5";
    let link = format!("<a href=\"{href}\" >n</a>").into_bytes();
    assert_eq!(answers, [link]);
    let answers = on_request(&site, near(5), (true, true), vec![next()]).await;
    assert_eq!(answers, [Vec::<u8>::new()]);
    let options = "template=hello-elementor&stylesheet=hello-elementor&permalink_structure=%2Findex.php%2F%25postname%25%2F";
    let site = site_on(&config, options).await;
    let next = print("next_posts_link", &["n"]);
    let answers = on_request(&site, request("/"), (true, true), vec![previous(), next]).await;
    let unserved = format!("{:?}", Err::<Value, String>(UNSERVED.into())).into_bytes();
    assert_eq!(answers, [unserved.clone(), unserved]);
}

/// `get_bloginfo( $show, $filter )` runs the display filters only for
/// `'display'`, and `get_search_query( $escaped )` escapes unless its
/// argument is falsy.
#[tokio::test(flavor = "multi_thread")]
async fn filters_and_escaping_follow_their_arguments() {
    let config = hyscribe_testkit::fixture_database();
    let options = "template=hello-elementor&stylesheet=hello-elementor&blogname=A%20%26%20B";
    let site = site_on(&config, options).await;
    let calls = vec![
        get("get_bloginfo", &["name"]),
        get("get_bloginfo", &["name", "display"]),
        get("get_bloginfo", &["name", "raw"]),
    ];
    let answers = on_page(&site, "/?p=1", (true, true), calls).await;
    let (raw, display) = (b"A & B".to_vec(), b"A &#038; B".to_vec());
    assert_eq!(answers, [raw.clone(), display, raw]);
    let request = Request {
        search_query: b"a\"b<c & d".to_vec(),
        ..Request::default()
    };
    let search = |escaped: Option<bool>| {
        let args = escaped.map(Value::Bool).into_iter().collect();
        ThemeCall::Get("get_search_query".into(), args)
    };
    let calls = vec![search(None), search(Some(true)), search(Some(false))];
    let answers = on_request(&site, request, (true, true), calls).await;
    let escaped = b"a&quot;b&lt;c &amp; d".to_vec();
    assert_eq!(answers, [escaped.clone(), escaped, b"a\"b<c & d".to_vec()]);
}
