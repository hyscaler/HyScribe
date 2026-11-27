// SPDX-License-Identifier: AGPL-3.0-or-later

//! Process entry point; everything else is in the library.

use std::process::ExitCode;

fn main() -> ExitCode {
    let exe = std::env::current_exe()
        .and_then(std::fs::canonicalize)
        .unwrap_or_default();
    let home = exe
        .parent()
        .map(std::path::Path::to_path_buf)
        .unwrap_or_default();
    let env = std::env::vars_os()
        .filter_map(|(k, v)| Some((k.into_string().ok()?, v.into_string().ok()?)))
        .collect();
    let stdin = std::io::stdin();
    #[cfg(unix)]
    let terminal = std::os::fd::AsFd::as_fd(&stdin);
    #[cfg(windows)]
    let terminal = std::os::windows::io::AsHandle::as_handle(&stdin);
    ExitCode::from(hyscribe::run(hyscribe::Context {
        args: std::env::args_os().collect(),
        env,
        home,
        euid: hyscribe_service::effective_uid(),
        server: (exe, vec!["start".into()]),
        input: &mut stdin.lock(),
        terminal: Some(terminal),
        screen: std::io::IsTerminal::is_terminal(&std::io::stdout()),
        out: &mut std::io::stdout(),
        err: &mut std::io::stderr(),
    }))
}
