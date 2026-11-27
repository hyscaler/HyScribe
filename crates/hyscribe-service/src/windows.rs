// SPDX-License-Identifier: AGPL-3.0-or-later

//! Windows's stand-in for signals. Before it takes the lock, the server
//! makes a named pipe of its own, `\\.\pipe\hyscribe-<pid>`, and acts on a
//! byte written to it: `service stop` writes [`STOP`], which the server
//! takes as SIGTERM, then [`KILL`] if it has not stopped in time, on which
//! it exits at once. The pipe's default access list lets only the server's
//! own user, administrators and the system write to it; anyone else may
//! only connect and read, which asks for nothing. Ctrl-C at the server's
//! console counts as SIGINT.

use std::fs::OpenOptions;
use std::io::{self, Write as _};
use std::sync::mpsc;
use std::time::{Duration, Instant};

use tokio::io::AsyncReadExt as _;
use tokio::net::windows::named_pipe::{NamedPipeServer, ServerOptions};

/// Asks the server to stop, as SIGTERM does.
pub(crate) const STOP: u8 = b's';
/// Tells the server to exit at once, as SIGKILL does.
pub(crate) const KILL: u8 = b'k';
/// The numbers the log's last line names, as on Unix.
const SIGTERM: i32 = 15;
const SIGINT: i32 = 2;
/// How long a client that connected may take to write its byte.
const PATIENCE: Duration = Duration::from_secs(1);
/// How long `service stop` tries while the pipe is busy with such a client.
const BUSY_WAIT: Duration = Duration::from_secs(5);
/// `ERROR_PIPE_BUSY`: the pipe's instance is connected to someone else.
const PIPE_BUSY: i32 = 231;

/// The pipe of the server whose pid is `pid`.
fn pipe(pid: u32) -> String {
    format!(r"\\.\pipe\hyscribe-{pid}")
}

/// Makes this process's pipe and takes requests on a thread of its own,
/// which sends the signal number each stands for.
pub(crate) fn listen() -> io::Result<mpsc::Receiver<i32>> {
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()?;
    let name = pipe(std::process::id());
    // Made here, so a failure is reported and the pipe is there before the
    // pid is; a name another process holds already is refused.
    let server = {
        let _context = runtime.enter();
        instance(&name, true)?
    };
    let (sender, receiver) = mpsc::channel();
    std::thread::spawn(move || runtime.block_on(serve(&name, server, &sender)));
    Ok(receiver)
}

/// An instance of the pipe `name`, for one client.
fn instance(name: &str, first: bool) -> io::Result<NamedPipeServer> {
    ServerOptions::new()
        .first_pipe_instance(first)
        .reject_remote_clients(true)
        .create(name)
}

async fn serve(name: &str, mut server: NamedPipeServer, sender: &mpsc::Sender<i32>) {
    let mut ctrl_c = tokio::signal::windows::ctrl_c().ok();
    loop {
        let number = tokio::select! {
            connected = server.connect() => {
                // An instance serves one client: the next waits on a new one.
                let client = match instance(name, false) {
                    Ok(next) => Some(std::mem::replace(&mut server, next)),
                    Err(_) => {
                        let _ = server.disconnect();
                        tokio::time::sleep(crate::POLL).await;
                        None
                    }
                };
                match (connected, client) {
                    (Ok(()), Some(client)) => match request_in(client).await {
                        Some(STOP) => Some(SIGTERM),
                        Some(KILL) => std::process::exit(1),
                        _ => None,
                    },
                    _ => None,
                }
            },
            Some(()) = async {
                match ctrl_c.as_mut() {
                    Some(ctrl_c) => ctrl_c.recv().await,
                    None => std::future::pending().await,
                }
            } => Some(SIGINT),
        };
        if let Some(number) = number {
            // Nobody listens any more once the server stops.
            let _ = sender.send(number);
        }
    }
}

/// The byte a connected client writes; none when it writes nothing in time.
async fn request_in(mut client: NamedPipeServer) -> Option<u8> {
    let mut byte = [0];
    let read = tokio::time::timeout(PATIENCE, client.read_exact(&mut byte)).await;
    let [byte] = byte;
    read.is_ok_and(|read| read.is_ok()).then_some(byte)
}

/// Writes `byte` to the pipe of the server whose pid is `pid`. A server
/// with no pipe, or one closing it, is gone already.
pub(crate) fn request(pid: u32, byte: u8) -> io::Result<()> {
    let deadline = Instant::now() + BUSY_WAIT;
    loop {
        match OpenOptions::new().write(true).open(pipe(pid)) {
            Ok(mut pipe) => {
                return match pipe.write_all(&[byte]) {
                    Err(err) if err.kind() == io::ErrorKind::BrokenPipe => Ok(()),
                    written => written,
                };
            }
            Err(err) if err.kind() == io::ErrorKind::NotFound => return Ok(()),
            Err(err) if err.raw_os_error() == Some(PIPE_BUSY) && Instant::now() < deadline => {
                std::thread::sleep(crate::POLL);
            }
            Err(err) => return Err(err),
        }
    }
}
