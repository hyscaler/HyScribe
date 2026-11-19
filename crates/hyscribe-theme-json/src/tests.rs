// SPDX-License-Identifier: AGPL-3.0-or-later

use std::fs;
use std::path::{Path, PathBuf};

use hyscribe_phpser::{encode, json_decode};

use super::*;
use hyscribe_testkit::unhex;

/// The inputs of the default install, owned.
pub(crate) struct Fixture {
    pub(crate) core: Vec<u8>,
    pub(crate) blocks: Vec<BlockType>,
    pub(crate) styles: Vec<(String, String)>,
    pub(crate) theme: Vec<u8>,
    pub(crate) theme_styles: Vec<(String, Vec<u8>)>,
}

impl Fixture {
    pub(crate) fn sources(&self) -> Sources<'_> {
        Sources {
            core: &self.core,
            blocks: &self.blocks,
            block_styles: &self.styles,
            theme: Some(&self.theme),
            theme_styles: &self.theme_styles,
            user: None,
        }
    }
}

pub(crate) fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

/// A golden file of `reference/theme-json/golden.sh`.
pub(crate) fn golden(name: &str) -> Vec<u8> {
    fs::read(
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/golden")
            .join(name),
    )
    .unwrap()
}

fn lines(name: &str) -> Vec<String> {
    String::from_utf8(golden(name))
        .unwrap()
        .lines()
        .map(str::to_owned)
        .collect()
}

/// Twenty Twenty-Five on WordPress 7.1.3, as the oracle has it.
pub(crate) fn fixture() -> Fixture {
    let blocks = lines("registry.txt")
        .into_iter()
        .map(|name| {
            let dir = root()
                .join("wordpress/wp-includes/blocks")
                .join(name.trim_start_matches("core/"));
            let json =
                fs::read(dir.join("block.json")).unwrap_or_else(|_| golden("post-comments.json"));
            BlockType {
                name,
                metadata: json_decode(&json).unwrap(),
            }
        })
        .collect();
    let styles = lines("registered-styles.tsv")
        .iter()
        .map(|l| {
            let (block, style) = l.split_once('\t').unwrap();
            (block.to_owned(), style.to_owned())
        })
        .collect();
    let base = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/golden/theme-styles");
    let mut theme_styles = Vec::new();
    for dir in ["blocks", "sections"] {
        for file in fs::read_dir(base.join(dir)).unwrap() {
            let file = file.unwrap().path();
            let name = format!("{dir}/{}", file.file_name().unwrap().to_string_lossy());
            theme_styles.push((name, fs::read(&file).unwrap()));
        }
    }
    Fixture {
        core: fs::read(root().join("wordpress/wp-includes/theme.json")).unwrap(),
        blocks,
        styles,
        theme: fs::read(root().join("themes/twentytwentyfive/theme.json")).unwrap(),
        theme_styles,
    }
}

/// The default install, resolved once.
pub(crate) fn resolved() -> &'static GlobalStyles {
    static RESOLVED: std::sync::OnceLock<GlobalStyles> = std::sync::OnceLock::new();
    RESOLVED.get_or_init(|| GlobalStyles::resolve(&fixture().sources()))
}

/// The default install's block metadata.
pub(crate) fn blocks() -> &'static blocks::Blocks {
    &resolved().blocks
}

/// Checks `ours` against every case of `kind` in `cases.tsv`: PHP's
/// `serialize()` of its result for the same JSON arguments.
pub(crate) fn check(kind: &str, ours: impl Fn(&[Value]) -> Value) {
    check_except(kind, ours, |_, expected| expected);
}

/// [`check`] where `meant` turns PHP's answer for the arguments into the
/// one HyScribe means to give, for the deliberate differences.
pub(crate) fn check_except(
    kind: &str,
    ours: impl Fn(&[Value]) -> Value,
    meant: impl Fn(&[Value], Vec<u8>) -> Vec<u8>,
) {
    let golden = String::from_utf8(golden("cases.tsv")).unwrap();
    let mut count = 0;
    let mut failures = Vec::new();
    for line in golden.lines() {
        let mut fields = line.split('\t');
        if fields.next() != Some(kind) {
            continue;
        }
        let json = unhex(fields.next().unwrap());
        let expected = unhex(fields.next().unwrap());
        let args: Vec<Value> = php::entries(&json_decode(&json).unwrap())
            .iter()
            .map(|(_, v)| v.clone())
            .collect();
        let got = encode(&ours(&args));
        let expected = meant(&args, expected);
        if got != expected {
            failures.push(format!(
                "{}\n  php:  {}\n  ours: {}",
                String::from_utf8_lossy(&json),
                String::from_utf8_lossy(&expected),
                String::from_utf8_lossy(&got)
            ));
        }
        count += 1;
    }
    assert!(count > 0, "no {kind} cases");
    assert!(
        failures.is_empty(),
        "{} of {count} {kind} cases differ:\n{}",
        failures.len(),
        failures.join("\n")
    );
}

/// A string result as PHP has it.
pub(crate) fn text(s: &str) -> Value {
    php::string(s)
}

/// A string argument.
pub(crate) fn arg(args: &[Value], i: usize) -> &str {
    php::as_str(&args[i]).unwrap()
}

/// The front page's blocks.
const ON_PAGE: [&str; 13] = [
    "core/site-title",
    "core/page-list",
    "core/navigation",
    "core/group",
    "core/heading",
    "core/post-title",
    "core/paragraph",
    "core/post-content",
    "core/post-date",
    "core/post-template",
    "core/spacer",
    "core/columns",
    "core/navigation-link",
];

#[test]
fn default_install_matches_php() {
    let fixture = fixture();
    let styles = GlobalStyles::resolve(&fixture.sources());
    assert!(encode(styles.settings()) == golden("settings.ser"));
    assert!(encode(styles.styles()) == golden("styles.ser"));
    assert!(styles.stylesheet().into_bytes() == golden("stylesheet-all.css"));
    let blocks: String = styles
        .block_css()
        .iter()
        .map(|(n, c)| format!("{n}\t{c}\n"))
        .collect();
    assert!(blocks.into_bytes() == golden("blocks.tsv"));
    let mut page = styles.inline_css(&|b| ON_PAGE.contains(&b)).into_bytes();
    page.push(b'\n');
    assert!(page == golden("page.css"));
}

/// A style variation is sanitized as the theme's own data: what the schema
/// does not know goes, and a registered block style's variation stays.
#[test]
fn style_variations_are_sanitized() {
    let fixture = fixture();
    let styles = GlobalStyles::resolve(&fixture.sources());
    let raw = br#"{"version": 3, "title": "T", "nope": 1, "styles": {"blocks": {"core/group": {"variations": {"section-1": {"color": {"text": "red"}}}}}}}"#;
    let data = styles.theme_json(json_decode(raw).unwrap());
    let kept = br#"{"version":3,"title":"T","styles":{"blocks":{"core\/group":{"variations":{"section-1":{"color":{"text":"red"}}}}}}}"#;
    assert_eq!(hyscribe_phpser::json_encode(&data).unwrap(), kept);
}

/// A site's global styles post is sanitized as the user's data: its
/// presets are keyed by the `custom` origin (REST `templates-custom.tsv`).
#[test]
fn user_global_styles_are_sanitized() {
    let fixture = fixture();
    let styles = GlobalStyles::resolve(&fixture.sources());
    let raw = br#"{"version": 3, "nope": 1, "settings": {"color": {"palette": [{"slug": "b", "color": "red", "name": "B"}]}}}"#;
    let data = styles.user_theme_json(json_decode(raw).unwrap());
    let kept = br#"{"version":3,"settings":{"color":{"palette":{"custom":[{"slug":"b","color":"red","name":"B"}]}}}}"#;
    assert_eq!(hyscribe_phpser::json_encode(&data).unwrap(), kept);
}

#[test]
fn lookups() {
    let fixture = fixture();
    let styles = GlobalStyles::resolve(&fixture.sources());
    assert!(styles.use_root_padding_aware_alignments());
    assert!(styles.has_block_gap_support());
    assert_eq!(
        styles.style(&["spacing", "blockGap"]),
        Some(&php::string("1.2rem"))
    );
    assert_eq!(
        styles.block_style("core/columns", &["spacing", "blockGap"]),
        Some(&php::string("var(--wp--preset--spacing--50)"))
    );
    assert_eq!(styles.block_style("core/nothing", &["x"]), None);
    let sizes = styles.presets(&["typography", "fontSizes"]);
    let origins: Vec<_> = sizes.iter().map(|(o, list)| (*o, list.len())).collect();
    assert_eq!(origins, [("default", 4), ("theme", 5)]);
    assert_eq!(styles.presets(&["nothing"]), []);
    let size = |size: &str| json_decode(format!(r#"{{"size":{size}}}"#).as_bytes()).unwrap();
    // Twenty Twenty-Five turns fluid typography on.
    assert_eq!(
        styles.font_size_value(&size(r#""2rem""#)),
        Some(text(
            "clamp(1.25rem, 1.25rem + ((1vw - 0.2rem) * 1.176), 2rem)"
        ))
    );
    assert_eq!(styles.font_size_value(&size("null")), None);
    let odd = GlobalStyles {
        data: json_decode(br#"{"settings": {"color": {"palette": {"theme": "x", "custom": []}}}}"#)
            .unwrap(),
        blocks: blocks::Blocks::default(),
    };
    assert_eq!(odd.presets(&["color", "palette"]), [("custom", &[][..])]);
}

#[test]
fn empty_sources() {
    let styles = GlobalStyles::resolve(&Sources {
        core: b"",
        blocks: &[],
        block_styles: &[],
        theme: None,
        theme_styles: &[],
        user: None,
    });
    assert!(!styles.use_root_padding_aware_alignments());
    assert!(!styles.has_block_gap_support());
    // Theme supports give settings, but nothing to print.
    assert_eq!(
        styles.setting(&["color", "custom"]),
        Some(&Value::Bool(true))
    );
    assert_eq!(styles.styles(), &Value::Null);
    assert_eq!(styles.inline_css(&|_| true), "");
    assert_eq!(styles.block_css(), []);
    let empty = GlobalStyles {
        data: php::array(),
        blocks: blocks::Blocks::default(),
    };
    assert_eq!(empty.settings(), &Value::Null);
}

#[test]
fn inline_css_picks_blocks() {
    let fixture = fixture();
    let styles = GlobalStyles::resolve(&fixture.sources());
    let none = styles.inline_css(&|_| false);
    assert_eq!(none.lines().count(), 1);
    let all = styles.inline_css(&|_| true);
    assert!(all.lines().count() > 13);
}

#[test]
fn block_names_from_paths() {
    let path = |p: &[&str]| p.iter().map(|s| (*s).to_owned()).collect::<Vec<_>>();
    assert_eq!(
        block_name_from_path(&path(&["styles", "blocks", "a/b", "x"])),
        "a/b"
    );
    assert_eq!(block_name_from_path(&path(&["styles", "blocks", "ab"])), "");
    assert_eq!(
        block_name_from_path(&path(&["x", "core/quote"])),
        "core/quote"
    );
    assert_eq!(
        block_name_from_path(&path(&["styles", "elements", "a/b"])),
        ""
    );
    assert_eq!(block_name_from_path(&path(&["styles", "x", "a/b"])), "");
}

/// `wp_get_global_stylesheet()` of one type at a time, as the block
/// editor's settings take them: together the whole stylesheet.
#[test]
fn stylesheets_by_type() {
    let fixture = fixture();
    let styles = GlobalStyles::resolve(&fixture.sources());
    let parts = ["variables", "styles", "presets"].map(|kind| styles.stylesheet_of(kind));
    assert!(parts.iter().all(|part| !part.is_empty()));
    assert_eq!(parts.concat(), styles.stylesheet());
    assert_eq!(styles.stylesheet_of("custom-css"), "");
    let user =
        br#"{"version":3,"isGlobalStylesUserThemeJSON":true,"styles":{"css":"p{color:red}"}}"#;
    let sources = Sources {
        user: Some(user),
        ..fixture.sources()
    };
    let custom = GlobalStyles::resolve(&sources).stylesheet_of("custom-css");
    assert_eq!(custom, "p{color:red}");
}
