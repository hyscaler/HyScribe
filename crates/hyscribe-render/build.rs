// SPDX-License-Identifier: AGPL-3.0-or-later

//! Embeds the default theme's files, which pages link to under the theme's
//! URL, with the content type the web server sends each. The
//! export's own data files are not WordPress's to serve.

use std::io;
use std::path::Path;

const THEME: &str = "../../themes/twentytwentyfive";
const EXPORTED: [&str; 3] = ["block-styles.tsv", "patterns.json", "styles.json"];
const CONTENT_TYPES: [(&str, &str); 7] = [
    ("css", "text/css"),
    ("html", "text/html"),
    ("json", "application/json"),
    ("png", "image/png"),
    ("txt", "text/plain"),
    ("webp", "image/webp"),
    ("woff2", "font/woff2"),
];

fn main() -> io::Result<()> {
    println!("cargo::rerun-if-changed={THEME}");
    let mut files = Vec::new();
    collect(Path::new(THEME), "", &mut files)?;
    files.sort();
    let mut entries = String::new();
    for file in &files {
        let extension = file.rsplit('.').next().unwrap_or_default();
        let (_, content_type) = CONTENT_TYPES
            .iter()
            .find(|(known, _)| *known == extension)
            .ok_or_else(|| io::Error::other(format!("no content type for {file}")))?;
        entries.push_str(&format!(
            "    ({file:?}, {content_type:?}, include_bytes!(concat!(env!(\"CARGO_MANIFEST_DIR\"), {:?}))),\n",
            format!("/{THEME}/{file}")
        ));
    }
    let out = std::env::var_os("OUT_DIR").ok_or_else(|| io::Error::other("no OUT_DIR"))?;
    std::fs::write(
        Path::new(&out).join("theme_files.rs"),
        format!("&[\n{entries}]\n"),
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
        } else if !EXPORTED.contains(&name.as_str()) {
            files.push(name);
        }
    }
    Ok(())
}
