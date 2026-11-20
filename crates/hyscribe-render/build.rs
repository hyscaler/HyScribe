// SPDX-License-Identifier: AGPL-3.0-or-later

//! Embeds the theme ports of `themes/`, one folder per theme slug: the
//! files pages link to under the theme's URL, with the content type
//! the web server sends each, and the export's own data files, which are
//! not WordPress's to serve. With the `fixture-themes` feature the fixture
//! themes of `fixtures/themes/` (the child block theme of 10.3) follow them.

use std::io;
use std::path::Path;

const THEMES: &str = "../../themes";
const FIXTURE_THEMES: &str = "../../fixtures/themes";
const EXPORTED: [&str; 3] = ["patterns.json", "registered.json", "styles.json"];
const CONTENT_TYPES: [(&str, &str); 11] = [
    ("css", "text/css"),
    ("html", "text/html"),
    ("jpg", "image/jpeg"),
    ("json", "application/json"),
    ("md", "text/markdown"),
    ("mp4", "video/mp4"),
    ("png", "image/png"),
    ("ttf", "font/ttf"),
    ("txt", "text/plain"),
    ("webp", "image/webp"),
    ("woff2", "font/woff2"),
];

fn main() -> io::Result<()> {
    println!("cargo::rerun-if-changed={THEMES}");
    println!("cargo::rerun-if-changed={FIXTURE_THEMES}");
    // The shipped themes first, sorted: the first is the default theme.
    let mut folders = vec![(THEMES, slugs_in(THEMES)?)];
    if std::env::var_os("CARGO_FEATURE_FIXTURE_THEMES").is_some() {
        folders.push((FIXTURE_THEMES, slugs_in(FIXTURE_THEMES)?));
    }
    let slugs: Vec<(&str, &String)> = folders
        .iter()
        .flat_map(|(folder, slugs)| slugs.iter().map(move |slug| (*folder, slug)))
        .collect();
    let mut themes = String::new();
    for (folder, slug) in &slugs {
        let root = format!("{folder}/{slug}");
        let mut files = Vec::new();
        collect(Path::new(&root), "", &mut files)?;
        files.sort();
        let mut entries = String::new();
        for file in &files {
            let extension = file.rsplit('.').next().unwrap_or_default();
            let (_, content_type) = CONTENT_TYPES
                .iter()
                .find(|(known, _)| *known == extension)
                .ok_or_else(|| io::Error::other(format!("no content type for {slug}/{file}")))?;
            entries.push_str(&format!(
                "        ({file:?}, {content_type:?}, {}),\n",
                include(&root, file)
            ));
        }
        let [patterns, registered, styles] = EXPORTED.map(|name| include(&root, name));
        themes.push_str(&format!(
            "    Data {{\n        slug: {slug:?},\n        files: &[\n{entries}        ],\n        patterns: {patterns},\n        registered: {registered},\n        styles: {styles},\n    }},\n"
        ));
    }
    let out = std::env::var_os("OUT_DIR").ok_or_else(|| io::Error::other("no OUT_DIR"))?;
    // An array of known length, so that the first theme can be indexed.
    std::fs::write(
        Path::new(&out).join("themes.rs"),
        format!("const THEMES: &[Data; {}] = &[\n{themes}];\n", slugs.len()),
    )
}

/// The sorted slugs of the theme folders in `folder`.
fn slugs_in(folder: &str) -> io::Result<Vec<String>> {
    let mut slugs = Vec::new();
    for entry in std::fs::read_dir(folder)? {
        let entry = entry?;
        if entry.file_type()?.is_dir() {
            slugs.push(entry.file_name().to_string_lossy().into_owned());
        }
    }
    slugs.sort();
    Ok(slugs)
}

/// `include_bytes!` of a theme's file.
fn include(root: &str, file: &str) -> String {
    format!(
        "include_bytes!(concat!(env!(\"CARGO_MANIFEST_DIR\"), {:?}))",
        format!("/{root}/{file}")
    )
}

fn collect(dir: &Path, prefix: &str, files: &mut Vec<String>) -> io::Result<()> {
    for entry in std::fs::read_dir(dir)? {
        let entry = entry?;
        let name = format!("{prefix}{}", entry.file_name().to_string_lossy());
        let kind = entry.file_type()?;
        if kind.is_symlink() {
            return Err(io::Error::other(format!("{name} is a link")));
        }
        if kind.is_dir() {
            collect(&entry.path(), &format!("{name}/"), files)?;
        } else if !(prefix.is_empty() && EXPORTED.contains(&name.as_str())) {
            files.push(name);
        }
    }
    Ok(())
}
