// SPDX-License-Identifier: AGPL-3.0-or-later

//! Embeds the files of `wp-includes` and `wp-admin` from the pinned WordPress release
//!: downloaded, or read from `HYSCRIBE_WORDPRESS_TARBALL` for
//! offline builds, and checked against its SHA-256 before anything in it
//! is used. PHP is left out, and so is a `.js` or `.css` that has a `.min`
//! twin, which pages only request with `SCRIPT_DEBUG`. Each file served
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
const THEMES: &str = "wp-content/themes";
/// Files with a `.min` twin, so not served, that PHP reads as they are.
const READ: [&str; 1] = ["css/dist/block-editor/default-editor-styles.css"];
/// As the web server sends them; a file with no extension gets no type.
const CONTENT_TYPES: [(&str, &str); 15] = [
    ("css", "text/css"),
    ("crt", "application/x-x509-ca-cert"),
    ("eot", "application/vnd.ms-fontobject"),
    ("gif", "image/gif"),
    ("js", "text/javascript"),
    ("json", "application/json"),
    ("md", "text/markdown"),
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
    println!("cargo::rerun-if-env-changed={TARBALL_ENV}");
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
    run(Command::new("tar")
        .arg("-xzf")
        .arg(&tarball)
        .arg("-C")
        .arg(&tree)
        .arg("wordpress/wp-includes")
        .arg("wordpress/wp-admin")
        .args(
            themes
                .iter()
                .map(|slug| format!("wordpress/{THEMES}/{slug}")),
        ))?;
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
    let digest = hex(&Sha256::digest(std::fs::read(path)?));
    if digest == SHA256 {
        Ok(())
    } else {
        Err(io::Error::other(format!(
            "{} has SHA-256 {digest}, expected {SHA256}",
            path.display()
        )))
    }
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
