// SPDX-License-Identifier: AGPL-3.0-or-later

//! The default install (Twenty Twenty-Five, no user global styles) against
//! PHP WordPress: `serialize()` of the global settings and styles, the
//! stylesheet, the CSS of every block node and the front page's inline
//! global styles, byte for byte. Goldens come from
//! `reference/theme-json/golden.sh`.

// Tests fail by panicking.
#![allow(clippy::unwrap_used, clippy::panic, clippy::indexing_slicing)]

use std::fs;
use std::path::{Path, PathBuf};

use hyscribe_theme_json::{BlockType, GlobalStyles, Sources};

fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn golden(name: &str) -> Vec<u8> {
    fs::read(
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/golden")
            .join(name),
    )
    .unwrap()
}

fn text(name: &str) -> String {
    String::from_utf8(golden(name)).unwrap()
}

fn block_types() -> Vec<BlockType> {
    text("registry.txt")
        .lines()
        .map(|name| {
            let dir = root()
                .join("wordpress/wp-includes/blocks")
                .join(name.trim_start_matches("core/"));
            let json =
                fs::read(dir.join("block.json")).unwrap_or_else(|_| golden("post-comments.json"));
            BlockType {
                name: name.to_owned(),
                metadata: hyscribe_phpser::json_decode(&json).unwrap(),
            }
        })
        .collect()
}

fn theme_styles() -> Vec<(String, Vec<u8>)> {
    let base = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/golden/theme-styles");
    let mut files = Vec::new();
    for dir in fs::read_dir(&base).unwrap() {
        let dir = dir.unwrap().path();
        for file in fs::read_dir(&dir).unwrap() {
            let file = file.unwrap().path();
            let name = file
                .strip_prefix(&base)
                .unwrap()
                .to_string_lossy()
                .into_owned();
            files.push((name, fs::read(&file).unwrap()));
        }
    }
    files
}

fn default_install() -> GlobalStyles {
    let core = fs::read(root().join("wordpress/wp-includes/theme.json")).unwrap();
    let theme = fs::read(root().join("themes/twentytwentyfive/theme.json")).unwrap();
    let styles: Vec<(String, String)> = text("registered-styles.tsv")
        .lines()
        .map(|l| {
            let (block, style) = l.split_once('\t').unwrap();
            (block.to_owned(), style.to_owned())
        })
        .collect();
    let blocks = block_types();
    let theme_styles = theme_styles();
    GlobalStyles::resolve(&Sources {
        core: &core,
        blocks: &blocks,
        block_styles: &styles,
        theme: Some(&theme),
        theme_styles: &theme_styles,
        user: None,
    })
}

/// Fails with the first difference and some context.
fn same(what: &str, ours: &[u8], php: &[u8]) {
    if ours == php {
        return;
    }
    let at = ours.iter().zip(php).take_while(|(a, b)| a == b).count();
    let context = |s: &[u8]| {
        String::from_utf8_lossy(&s[at.saturating_sub(120)..(at + 200).min(s.len())]).into_owned()
    };
    panic!(
        "{what} differs at byte {at} (ours {} bytes, php {}):\n  php:  {}\n  ours: {}",
        ours.len(),
        php.len(),
        context(php),
        context(ours)
    );
}

#[test]
fn global_settings() {
    let styles = default_install();
    same(
        "settings",
        &hyscribe_phpser::encode(styles.settings()),
        &golden("settings.ser"),
    );
}

#[test]
fn global_styles() {
    let styles = default_install();
    same(
        "styles",
        &hyscribe_phpser::encode(styles.styles()),
        &golden("styles.ser"),
    );
}

#[test]
fn stylesheet() {
    let styles = default_install();
    same(
        "stylesheet",
        styles.stylesheet().as_bytes(),
        &golden("stylesheet-all.css"),
    );
}

#[test]
fn block_css() {
    let styles = default_install();
    let ours: String = styles
        .block_css()
        .iter()
        .map(|(name, css)| format!("{name}\t{css}\n"))
        .collect();
    same("blocks", ours.as_bytes(), &golden("blocks.tsv"));
}

#[test]
fn front_page_inline_css() {
    let styles = default_install();
    // The blocks the front page renders, with their styles enqueued.
    let on_page = [
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
    let css = styles.inline_css(&|block| on_page.contains(&block));
    let page = golden("page.css");
    same("page", css.as_bytes(), page.strip_suffix(b"\n").unwrap());
    same(
        "stylesheet",
        css.lines().next().unwrap().as_bytes(),
        &golden("stylesheet.css"),
    );
}
