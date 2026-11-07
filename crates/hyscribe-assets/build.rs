// SPDX-License-Identifier: AGPL-3.0-or-later

//! Embeds the files of `wp-includes` from the pinned WordPress release
//!: downloaded, or read from `HYSCRIBE_WORDPRESS_TARBALL` for
//! offline builds, and checked against its SHA-256 before anything in it
//! is used. PHP is left out, and so is a `.js` or `.css` that has a `.min`
//! twin, which pages only request with `SCRIPT_DEBUG`.

use sha2::{Digest, Sha256};
use std::io;
use std::path::{Path, PathBuf};
use std::process::Command;

const VERSION: &str = "7.1.3";
const SHA256: &str = "d2a09acb6a15e3b9c471d72557753c266d6f41a79ed19bfe04cbfe49e283b2a5";
const TARBALL_ENV: &str = "HYSCRIBE_WORDPRESS_TARBALL";
/// As the web server sends them; a file with no extension gets no type.
const CONTENT_TYPES: [(&str, &str); 14] = [
    ("css", "text/css"),
    ("crt", "application/x-x509-ca-cert"),
    ("eot", "application/vnd.ms-fontobject"),
    ("gif", "image/gif"),
    ("js", "text/javascript"),
    ("json", "application/json"),
    ("md", "text/markdown"),
    ("png", "image/png"),
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
    run(Command::new("tar")
        .arg("-xzf")
        .arg(&tarball)
        .arg("-C")
        .arg(&tree)
        .arg("wordpress/wp-includes"))?;
    let root = tree.join("wordpress/wp-includes");
    let mut files = Vec::new();
    collect(&root, "", &mut files)?;
    files.sort();
    let mut entries = String::new();
    for file in files.iter().filter(|f| served(f, &root)) {
        let name = file.rsplit('/').next().unwrap_or_default();
        let extension = name.rsplit_once('.').map_or("", |(_, e)| e);
        let (_, content_type) = CONTENT_TYPES
            .iter()
            .find(|(known, _)| *known == extension)
            .ok_or_else(|| io::Error::other(format!("no content type for {file}")))?;
        entries.push_str(&format!(
            "    ({file:?}, {content_type:?}, include_bytes!({:?})),\n",
            root.join(file)
        ));
    }
    std::fs::write(out.join("wp_includes.rs"), format!("&[\n{entries}]\n"))
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
    let digest: String = Sha256::digest(std::fs::read(path)?)
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect();
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
