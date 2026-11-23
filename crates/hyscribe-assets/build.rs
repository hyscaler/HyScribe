// SPDX-License-Identifier: AGPL-3.0-or-later

//! Embeds the files of `wp-includes` and `wp-admin` from the pinned WordPress release:
//! downloaded, or read from `HYSCRIBE_WORDPRESS_TARBALL` for
//! offline builds, and checked against its SHA-256 before anything in it
//! is used. PHP is left out, and so is a `.js` or `.css` that has a `.min`
//! twin, which pages only request with `SCRIPT_DEBUG`. A theme the release
//! no longer bundles comes from its pinned wordpress.org archive
//! (`themes/pinned.tsv`), checked the same way before it is unpacked
//! (`HYSCRIBE_THEME_ZIPS` is a folder of them for offline builds). Each file served
//! from the release, the embedded themes' too, keeps its modification time
//! and SHA-256 for `Last-Modified` and `ETag`.

use sha2::{Digest, Sha256};
use std::io;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::UNIX_EPOCH;

const VERSION: &str = "7.1.3";
const SHA256: &str = "d2a09acb6a15e3b9c471d72557753c266d6f41a79ed19bfe04cbfe49e283b2a5";
const TARBALL_ENV: &str = "HYSCRIBE_WORDPRESS_TARBALL";
const THEME_ZIPS_ENV: &str = "HYSCRIBE_THEME_ZIPS";
const THEMES: &str = "wp-content/themes";
/// Files with a `.min` twin, so not served, that PHP reads as they are.
const READ: [&str; 1] = ["css/dist/block-editor/default-editor-styles.css"];
/// As the web server sends them; a file with no extension gets no type.
const CONTENT_TYPES: [(&str, &str); 18] = [
    ("css", "text/css"),
    ("crt", "application/x-x509-ca-cert"),
    ("eot", "application/vnd.ms-fontobject"),
    ("gif", "image/gif"),
    ("html", "text/html"),
    ("jpg", "image/jpeg"),
    ("js", "text/javascript"),
    ("json", "application/json"),
    ("md", "text/markdown"),
    ("mp4", "video/mp4"),
    ("png", "image/png"),
    // Sass sources, which Apache sends without a type.
    ("scss", ""),
    ("svg", "image/svg+xml"),
    ("ttf", "font/ttf"),
    ("txt", "text/plain"),
    ("woff", "font/woff"),
    ("woff2", "font/woff2"),
    ("", ""),
];

fn main() -> io::Result<()> {
    println!("cargo::rerun-if-changed=build.rs");
    println!("cargo::rerun-if-changed=../../wordpress/VERSION");
    println!("cargo::rerun-if-changed=../../themes");
    println!("cargo::rerun-if-changed=../../wordpress/wp-includes/dependencies.json");
    println!("cargo::rerun-if-changed=../../themes/pinned.tsv");
    println!("cargo::rerun-if-env-changed={TARBALL_ENV}");
    println!("cargo::rerun-if-env-changed={THEME_ZIPS_ENV}");
    let pinned = std::fs::read_to_string("../../wordpress/VERSION")?;
    let pinned = pinned.trim();
    if pinned != VERSION {
        return Err(io::Error::other(format!(
            "wordpress/VERSION is {pinned}, the build pins {VERSION}"
        )));
    }
    let out =
        PathBuf::from(std::env::var_os("OUT_DIR").ok_or_else(|| io::Error::other("no OUT_DIR"))?);
    let tarball = tarball(&out)?;
    let tree = out.join("wordpress");
    if tree.exists() {
        std::fs::remove_dir_all(&tree)?;
    }
    std::fs::create_dir_all(&tree)?;
    // The themes of `themes/`, each bundled with the release.
    let mut themes = Vec::new();
    for entry in std::fs::read_dir("../../themes")? {
        let entry = entry?;
        if entry.file_type()?.is_dir() {
            themes.push(entry.file_name().to_string_lossy().into_owned());
        }
    }
    themes.sort();
    let pinned_themes = read_pins("../../themes/pinned.tsv")?;
    let bundled: Vec<&String> = themes
        .iter()
        .filter(|slug| !pinned_themes.iter().any(|(pinned, _, _)| pinned == *slug))
        .collect();
    run(Command::new("tar")
        .arg("-xzf")
        .arg(&tarball)
        .arg("-C")
        .arg(&tree)
        .arg("wordpress/wp-includes")
        .arg("wordpress/wp-admin")
        .args(
            bundled
                .iter()
                .map(|slug| format!("wordpress/{THEMES}/{slug}")),
        ))?;
    for (slug, version, sha) in &pinned_themes {
        if themes.contains(slug) {
            let zip = theme_zip(&out, slug, version, sha)?;
            unpack_theme(&zip, slug, &tree.join("wordpress").join(THEMES))?;
        }
    }
    // Files a registered style or script names are served even with a
    // `.min` twin: WordPress asks for them as they are.
    let dependencies = std::fs::read_to_string("../../wordpress/wp-includes/dependencies.json")?;
    let registered: Vec<&str> = dependencies
        .split("\"src\": \"")
        .skip(1)
        .filter_map(|rest| rest.split('"').next())
        .collect();
    let mut release = Vec::new();
    for dir in ["wp-includes", "wp-admin"] {
        let root = tree.join("wordpress").join(dir);
        let mut files = Vec::new();
        collect(&root, "", &mut files)?;
        files.sort();
        let mut entries = String::new();
        let named = |file: &str| registered.contains(&format!("/{dir}/{file}").as_str());
        for file in files.iter().filter(|f| named(f) || served(f, &root)) {
            release.push(format!("{dir}/{file}"));
            let name = file.rsplit('/').next().unwrap_or_default();
            let extension = name.rsplit_once('.').map_or("", |(_, e)| e);
            let (_, content_type) = CONTENT_TYPES
                .iter()
                .find(|(known, _)| *known == extension)
                .ok_or_else(|| io::Error::other(format!("no content type for {dir}/{file}")))?;
            entries.push_str(&format!(
                "    ({file:?}, {content_type:?}, include_bytes!({:?})),\n",
                root.join(file)
            ));
        }
        let name = dir.replace('-', "_");
        std::fs::write(out.join(format!("{name}.rs")), format!("&[\n{entries}]\n"))?;
    }
    let includes = tree.join("wordpress").join("wp-includes");
    let read: String = READ
        .iter()
        .map(|file| {
            format!(
                "    ({file:?}, include_bytes!({:?})),\n",
                includes.join(file)
            )
        })
        .collect();
    std::fs::write(out.join("read.rs"), format!("&[\n{read}]\n"))?;
    for slug in &themes {
        let theme = format!("{THEMES}/{slug}");
        let mut files = Vec::new();
        collect(&tree.join("wordpress").join(&theme), "", &mut files)?;
        release.extend(
            files
                .iter()
                .filter(|f| !f.ends_with(".php"))
                .map(|f| format!("{theme}/{f}")),
        );
    }
    release.sort();
    let mut dates = String::new();
    for path in &release {
        let file = tree.join("wordpress").join(path);
        let modified = std::fs::metadata(&file)?
            .modified()?
            .duration_since(UNIX_EPOCH)
            .map_err(io::Error::other)?
            .as_secs();
        let digest = hex(&Sha256::digest(std::fs::read(&file)?));
        dates.push_str(&format!("    ({path:?}, {modified}, {digest:?}),\n"));
    }
    std::fs::write(out.join("release.rs"), format!("&[\n{dates}]\n"))
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

/// The verified release: the offline copy, a verified earlier download, or
/// a fresh one.
fn tarball(out: &Path) -> io::Result<PathBuf> {
    if let Some(path) = std::env::var_os(TARBALL_ENV) {
        let path = PathBuf::from(path);
        verify(&path)?;
        return Ok(path);
    }
    let path = out.join(format!("wordpress-{VERSION}.tar.gz"));
    if verify(&path).is_ok() {
        return Ok(path);
    }
    let partial = out.join("download.part");
    run(Command::new("curl")
        .args(["-fsSL", "--proto", "=https", "--tlsv1.2", "-o"])
        .arg(&partial)
        .arg(format!("https://wordpress.org/wordpress-{VERSION}.tar.gz")))?;
    verify(&partial)?;
    std::fs::rename(&partial, &path)?;
    Ok(path)
}

fn verify(path: &Path) -> io::Result<()> {
    verify_digest(path, SHA256)
}

fn verify_digest(path: &Path, expected: &str) -> io::Result<()> {
    let digest = hex(&Sha256::digest(std::fs::read(path)?));
    if digest == expected {
        Ok(())
    } else {
        Err(io::Error::other(format!(
            "{} has SHA-256 {digest}, expected {expected}",
            path.display()
        )))
    }
}

/// The rows of `themes/pinned.tsv`: slug, version and SHA-256.
fn read_pins(path: &str) -> io::Result<Vec<(String, String, String)>> {
    let mut rows = Vec::new();
    for line in std::fs::read_to_string(path)?.lines() {
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let fields: Vec<&str> = line.split('\t').collect();
        let [slug, version, sha] = fields[..] else {
            return Err(io::Error::other(format!("{path}: bad row {line:?}")));
        };
        rows.push((slug.to_owned(), version.to_owned(), sha.to_owned()));
    }
    Ok(rows)
}

/// A pinned theme's verified archive, like the release's: the offline copy,
/// a verified earlier download, or a fresh one.
fn theme_zip(out: &Path, slug: &str, version: &str, sha: &str) -> io::Result<PathBuf> {
    let name = format!("{slug}.{version}.zip");
    if let Some(dir) = std::env::var_os(THEME_ZIPS_ENV) {
        let path = PathBuf::from(dir).join(&name);
        verify_digest(&path, sha)?;
        return Ok(path);
    }
    let path = out.join(&name);
    if verify_digest(&path, sha).is_ok() {
        return Ok(path);
    }
    let partial = out.join("theme-download.part");
    run(Command::new("curl")
        .args(["-fsSL", "--proto", "=https", "--tlsv1.2", "-o"])
        .arg(&partial)
        .arg(format!("https://downloads.wordpress.org/theme/{name}")))?;
    verify_digest(&partial, sha)?;
    std::fs::rename(&partial, &path)?;
    Ok(path)
}

/// Unpacks a verified archive into a new folder of `themes`, in UTC as
/// `reference/themes/pin.sh` does, so file times agree with the oracle's.
fn unpack_theme(zip: &Path, slug: &str, themes: &Path) -> io::Result<()> {
    let listing = Command::new("unzip").arg("-Z1").arg(zip).output()?;
    if !listing.status.success()
        || String::from_utf8_lossy(&listing.stdout)
            .lines()
            .any(|entry| !entry.starts_with(&format!("{slug}/")))
    {
        return Err(io::Error::other(format!(
            "{} has an entry outside {slug}/",
            zip.display()
        )));
    }
    std::fs::create_dir_all(themes)?;
    run(Command::new("unzip")
        .env("TZ", "UTC")
        .args(["-q", "-d"])
        .arg(themes)
        .arg(zip))
}

fn run(command: &mut Command) -> io::Result<()> {
    let status = command.status()?;
    if status.success() {
        Ok(())
    } else {
        Err(io::Error::other(format!("{command:?} failed: {status}")))
    }
}

fn served(file: &str, root: &Path) -> bool {
    if file.ends_with(".php") {
        return false;
    }
    let twin = [".js", ".css"].iter().find_map(|ext| {
        let stem = file.strip_suffix(ext)?;
        (!stem.ends_with(".min")).then(|| format!("{stem}.min{ext}"))
    });
    twin.is_none_or(|twin| !root.join(twin).is_file())
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
        } else {
            files.push(name);
        }
    }
    Ok(())
}
