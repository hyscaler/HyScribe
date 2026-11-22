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
    let content = include_str!("../../../../../reference/theme-tags/content.sql");
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
        let Some((request, _)) = page_of(&site, &path).await else {
            failures.push(format!("{} {path}: no page", fields[0]));
            continue;
        };
        let got = tokio::task::spawn_blocking(move || {
            let theme = Theme::default();
            let support = ThemeCall::AddSupport("responsive-embeds".into(), Vec::new());
            assert_eq!(theme.answer(&support), Ok(Value::Null));
            assert_eq!(theme.answer(&support), Ok(Value::Null));
            theme.begin((&site, &request), &hello_files());
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
    let (request, _) = page_of(&site, path).await.unwrap();
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
        theme.begin((&site, &request), &files);
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
        theme.begin((&broken, &request), &[]);
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
    let site = Arc::new(site.with_extensions(&plugins, &themes));
    let running = crate::extensions::request(&site).unwrap();
    assert_eq!(format!("{running:?}"), "Running");
    let response = served(&site, "/").await;
    assert_eq!(response.status(), 200);
    let page = crate::admin::tests::page_of(response).await;
    let classes = "home blog wp-theme-hyscribe-theme";
    assert_eq!(page, format!("<body class=\"{classes}\">front</body>"));
    // `index.php` asks for `wp_head()`, which is not served yet.
    assert_eq!(served(&site, "/?p=1").await.status(), 500);
}
