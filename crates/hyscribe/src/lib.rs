// SPDX-License-Identifier: AGPL-3.0-or-later

//! The `hyscribe` command line. `main.rs` only gathers the process
//! arguments, environment and folder and calls [`run`], so every command is
//! testable in process.

use std::collections::HashMap;
use std::ffi::OsString;
use std::io::Write;
use std::path::PathBuf;

use clap::{Parser, Subcommand};
use hyscribe_config::{Config, Mode};

/// Source code location, offered to every user as AGPL section 13 requires.
pub const SOURCE_URL: &str = "https://github.com/hyscaler/HyScribe";

#[derive(Debug, Parser)]
#[command(name = "hyscribe", about = "Rust drop-in replacement for WordPress")]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Debug, Subcommand)]
enum Command {
    /// Print the version, license and source location.
    Version,
    /// Inspect configuration.
    Config {
        #[command(subcommand)]
        command: ConfigCommand,
    },
}

#[derive(Debug, Subcommand)]
enum ConfigCommand {
    /// Load .env and the environment, and report problems without starting.
    Check {
        /// Folder holding .env. Defaults to the binary's folder.
        #[arg(long)]
        dir: Option<PathBuf>,
    },
}

/// What the process needs from the outside world.
pub struct Context<'a> {
    /// Command line arguments, including the program name.
    pub args: Vec<OsString>,
    /// Environment variables that are valid UTF-8.
    pub env: HashMap<String, String>,
    /// The folder holding the binary.
    pub home: PathBuf,
    /// Standard output.
    pub out: &'a mut dyn Write,
    /// Standard error.
    pub err: &'a mut dyn Write,
}

/// Runs one command and returns the process exit code.
pub fn run(ctx: Context<'_>) -> u8 {
    let cli = match Cli::try_parse_from(&ctx.args) {
        Ok(cli) => cli,
        Err(err) => {
            let stream = if err.use_stderr() { ctx.err } else { ctx.out };
            // Nothing useful can be done if the terminal is gone.
            let _ = write!(stream, "{}", err.render());
            return u8::try_from(err.exit_code()).unwrap_or(2);
        }
    };
    let result = match cli.command {
        Command::Version => version(ctx.out),
        Command::Config {
            command: ConfigCommand::Check { dir },
        } => config_check(&dir.unwrap_or(ctx.home), &ctx.env, ctx.out),
    };
    match result {
        Ok(()) => 0,
        Err(message) => {
            let _ = writeln!(ctx.err, "error: {message}");
            1
        }
    }
}

fn version(out: &mut dyn Write) -> Result<(), String> {
    writeln!(
        out,
        "hyscribe {}\nLicense: AGPL-3.0-or-later\nSource: {SOURCE_URL}",
        env!("CARGO_PKG_VERSION")
    )
    .map_err(|err| err.to_string())
}

fn config_check(
    dir: &std::path::Path,
    env: &HashMap<String, String>,
    out: &mut dyn Write,
) -> Result<(), String> {
    let config = hyscribe_config::load(dir, env).map_err(|err| err.to_string())?;
    let text = match config {
        None => format!(
            "No configuration in {}: HyScribe will start in setup mode.\n",
            dir.display()
        ),
        Some(config) => describe(&config),
    };
    out.write_all(text.as_bytes())
        .map_err(|err| err.to_string())
}

/// A summary that is safe to show: no passwords, keys or salts.
fn describe(config: &Config) -> String {
    let db = &config.database;
    let mode = match config.mode {
        Mode::Shadow => "shadow (read only)",
        Mode::Primary => "primary",
    };
    let password = if db.password.expose().is_empty() {
        "not set"
    } else {
        "set"
    };
    format!(
        "Configuration OK\n\
         Database: {user}@{host}:{port}/{name} (password {password}, prefix {prefix}, charset {charset})\n\
         Listen: {bind}:{http_port}{proxy}\n\
         Mode: {mode}\n\
         Uploads: {uploads}\n\
         Keys and salts: {salts} of 8 set\n",
        user = db.user,
        host = db.host,
        port = db.port,
        name = db.name,
        prefix = db.prefix,
        charset = db.charset,
        bind = config.server.bind_host,
        http_port = config.server.port,
        proxy = if config.server.behind_proxy {
            " behind a proxy"
        } else {
            ""
        },
        uploads = config
            .uploads_dir
            .as_ref()
            .map_or("hyscribe/uploads".into(), |p| p.display().to_string()),
        salts = config.salts.len(),
    )
}

#[cfg(test)]
mod tests;
