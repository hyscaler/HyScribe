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
        other => format!("{other:?}").into_bytes(),
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
/// custom logo and menus; author archives, which HyScribe answers 404
///, are left out.
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
            let mut value = hyscribe_testkit::unhex(value);
            if name == "body_class" {
                let class = String::from_utf8(value).unwrap();
                value = class.replace(HELLO_CLASS, "").into_bytes();
            }
            expected.push((name.to_string(), value));
            lines.next();
        }
        let path = String::from_utf8(hyscribe_testkit::unhex(fields[1])).unwrap();
        if expected.is_empty() || path.contains("author=") {
            continue;
        }
        let site = match sites.get(fields[0]) {
            Some(site) => site.clone(),
            None => {
                let config = if fields[0].starts_with("_tags=content") {
                    content.clone()
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
    assert_eq!(pages, 132);
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
        let global = hyscribe_render::theme::classic_global_styles(hello_theme_json(), None);
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
    std::fs::write(public.join("theme.json"), hello_theme_json()).unwrap();
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
        // The test theme supports neither the title tag nor feed links.
        assert!(!page.contains("<title>") && !page.contains("application/rss+xml"));
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
fn hello_theme_json() -> &'static [u8] {
    hyscribe_assets::ported_file("hello-elementor", "theme.json").unwrap()
}

/// Hello's real component, built by `scripts/themes.sh` once per test
/// binary, installed and approved in a themes folder of its own as the
/// binary installs it, with an empty plugins folder beside it.
fn hello_installed() -> tempfile::TempDir {
    static WASM: std::sync::OnceLock<Vec<u8>> = std::sync::OnceLock::new();
    let wasm = WASM.get_or_init(|| {
        let script = concat!(env!("CARGO_MANIFEST_DIR"), "/../../scripts/themes.sh");
        let built = std::process::Command::new("sh")
            .arg(script)
            .output()
            .unwrap();
        let stderr = String::from_utf8_lossy(&built.stderr);
        assert!(built.status.success(), "scripts/themes.sh failed: {stderr}");
        let dir = String::from_utf8(built.stdout).unwrap();
        std::fs::read(format!("{}/hello-elementor.wasm", dir.trim())).unwrap()
    });
    let dir = tempfile::tempdir().unwrap();
    let themes = dir.path().join("themes");
    std::fs::create_dir_all(dir.path().join("plugins")).unwrap();
    let port = &hyscribe_assets::PORTED[0];
    let mut files = vec![
        (
            hyscribe_ext::manifest::FILE.to_string(),
            port.manifest.to_vec(),
        ),
        (hyscribe_ext::hyx::COMPONENT.to_string(), wasm.clone()),
    ];
    let public = port.public.iter();
    files.extend(public.map(|(path, bytes)| (format!("public/{path}"), bytes.to_vec())));
    hyscribe_ext::installed::embed(&themes, files).unwrap();
    let found = hyscribe_ext::installed::theme(&themes, "hello-elementor", "hello-elementor");
    hyscribe_ext::installed::approve(&found.unwrap()).unwrap();
    dir
}

/// `site` running the extensions installed in `dir`.
fn with_hello(site: Arc<Site>, dir: &tempfile::TempDir) -> Arc<Site> {
    let site = Arc::try_unwrap(site).ok().unwrap();
    let (plugins, themes) = (dir.path().join("plugins"), dir.path().join("themes"));
    Arc::new(site.with_extensions(&plugins, &themes))
}

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
        hello_theme_json(),
        None,
    ));
    let hello = hello_installed();
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
                // A primary site, whose extensions read options through its writer.
                let site = Arc::try_unwrap(site_on(&config, fields[0]).await)
                    .ok()
                    .unwrap();
                let writer = hyscribe_db::Writer::open(&config, hyscribe_config::Mode::Primary);
                let site = site.with_writer(writer.await.unwrap());
                let site = with_hello(Arc::new(site), &hello);
                sites.insert(fields[0].into(), site.clone());
                site
            }
        };
        let Some((request, _, title)) = page_of(&site, &path).await else {
            continue;
        };
        let global = global.clone();
        let ours = tokio::task::spawn_blocking(move || {
            let running = crate::extensions::request(&site, false).unwrap();
            let theme = running.theme.clone();
            let port = crate::extensions::dispatch(&running);
            let page = hyscribe_model::hooks::scoped(&port, || {
                hyscribe_model::hooks::do_action("after_setup_theme", &[]);
                theme.begin((&site, &request), &hello_files(), (&title, Some(global)));
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
            let mut faults = Vec::new();
            crate::extensions::log(&running.request.reports(), &mut faults);
            let page = String::from_utf8(theme.end(Ok(page)).unwrap()).unwrap();
            (page, String::from_utf8(faults).unwrap())
        })
        .await
        .unwrap();
        let (ours, faults) = ours;
        let php = String::from_utf8(hyscribe_testkit::unhex(fields[3])).unwrap();
        let php = crate::tests::as_hyscribe(php);
        if php != ours || !faults.is_empty() {
            failures.push(format!(
                "{} {path}:\n{faults}{}",
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
        let global = hyscribe_render::theme::classic_global_styles(hello_theme_json(), None);
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

/// The option's value and autoload the write oracle's `case` leaves.
fn hello_version_golden(case: &str) -> (String, String) {
    let path = format!(
        "{}/../hyscribe-testkit/tests/golden/writes/hello-version-{case}.tsv",
        env!("CARGO_MANIFEST_DIR")
    );
    let golden = std::fs::read_to_string(path).unwrap();
    let cell = |column: &str| {
        let line = golden
            .lines()
            .find(|line| line.contains(&format!("\t{column}\t")));
        line.unwrap().rsplit('\t').next().unwrap().to_string()
    };
    let autoload = golden.contains("\tautoload\t").then(|| cell("autoload"));
    (
        cell("option_value"),
        autoload.unwrap_or_else(|| "auto".into()),
    )
}

/// Hello's `after_setup_theme` writes `hello_theme_version` in an admin
/// request, a visitor's on the login redirect too, as PHP writes it (the
/// write oracle's `hello-version-*`), when it is missing or older, and
/// never on the front end.
#[tokio::test(flavor = "multi_thread")]
async fn hello_writes_its_version_as_php_does() {
    let _saves = crate::admin::tests::SAVES.lock().await;
    let db = content_db("SELECT 1").await;
    let hello = hello_installed();
    let site = site_on(&db, "template=hello-elementor&stylesheet=hello-elementor").await;
    let site = Arc::try_unwrap(site).ok().unwrap();
    let writer = hyscribe_db::Writer::open(&db, hyscribe_config::Mode::Primary).await;
    let site = with_hello(Arc::new(site.with_writer(writer.unwrap())), &hello);
    let options = sqlx::mysql::MySqlConnectOptions::new()
        .host(&db.host)
        .port(db.port)
        .database(&db.name)
        .username(&db.user)
        .password(db.password.expose());
    let mut conn = sqlx::MySqlConnection::connect_with(&options).await.unwrap();
    // A request reads the active theme from the database.
    let active = "UPDATE wp_options SET option_value = 'hello-elementor' \
        WHERE option_name IN ('template', 'stylesheet')";
    conn.execute(active).await.unwrap();
    let stored = "SELECT option_value, autoload FROM wp_options \
        WHERE option_name = 'hello_theme_version'";
    let set = |value: &str| {
        format!(
            "INSERT INTO wp_options (option_name, option_value, autoload) \
             VALUES ('hello_theme_version', '{value}', 'auto')"
        )
    };
    let gone = "DELETE FROM wp_options WHERE option_name = 'hello_theme_version'";
    let cases = [
        ("/", None, None),
        ("/hs-admin/", None, Some(hello_version_golden("missing"))),
        (
            "/wp-admin/",
            Some("3.0.0"),
            Some(hello_version_golden("older")),
        ),
        (
            "/hs-admin/",
            Some("3.5.2"),
            Some(("3.5.2".into(), "auto".into())),
        ),
        (
            "/hs-admin/",
            Some("3.10"),
            Some(("3.10".into(), "auto".into())),
        ),
    ];
    for (path, before, after) in cases {
        conn.execute(gone).await.unwrap();
        if let Some(before) = before {
            let insert = sqlx::raw_sql(sqlx::AssertSqlSafe(set(before)));
            conn.execute(insert).await.unwrap();
        }
        served(&site, path).await;
        let row: Option<(String, String)> = sqlx::query_as(stored)
            .fetch_optional(&mut conn)
            .await
            .unwrap();
        assert_eq!(row, after, "{path} over {before:?}");
    }
}
