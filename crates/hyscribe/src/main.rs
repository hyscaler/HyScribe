// SPDX-License-Identifier: AGPL-3.0-or-later

//! Process entry point; everything else is in the library.

use std::process::ExitCode;

fn main() -> ExitCode {
    let home = std::env::current_exe()
        .ok()
        .and_then(|exe| exe.parent().map(std::path::Path::to_path_buf))
        .unwrap_or_default();
    let env = std::env::vars_os()
        .filter_map(|(k, v)| Some((k.into_string().ok()?, v.into_string().ok()?)))
        .collect();
    ExitCode::from(hyscribe::run(hyscribe::Context {
        args: std::env::args_os().collect(),
        env,
        home,
        out: &mut std::io::stdout(),
        err: &mut std::io::stderr(),
    }))
}
