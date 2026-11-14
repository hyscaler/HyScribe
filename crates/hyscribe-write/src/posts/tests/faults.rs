// SPDX-License-Identifier: AGPL-3.0-or-later

//! A proxy to the fixture database that cuts its connection at the n-th
//! command once armed, so every database error a post function can meet
//! is met in a test.

use std::sync::Arc;
use std::sync::atomic::{AtomicI64, Ordering};

use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};

/// `COM_QUERY` and `COM_STMT_EXECUTE`: one per statement run.
const STATEMENTS: [u8; 2] = [0x03, 0x17];

pub(super) struct Faults {
    port: u16,
    /// Commands left before the cut; negative when not armed.
    budget: Arc<AtomicI64>,
}

/// Copies the client's packets to the server, cutting both at the command
/// that finds the budget spent.
async fn forward(
    mut client: tokio::net::tcp::OwnedReadHalf,
    mut server: tokio::net::tcp::OwnedWriteHalf,
    budget: Arc<AtomicI64>,
) -> std::io::Result<()> {
    loop {
        let mut header = [0; 4];
        client.read_exact(&mut header).await?;
        let len =
            usize::from(header[0]) | usize::from(header[1]) << 8 | usize::from(header[2]) << 16;
        let mut body = vec![0; len];
        client.read_exact(&mut body).await?;
        if header[3] == 0 && body.first().is_some_and(|c| STATEMENTS.contains(c)) {
            let left = budget.load(Ordering::SeqCst);
            if left == 0 {
                return Ok(());
            }
            if left > 0 {
                budget.store(left - 1, Ordering::SeqCst);
            }
        }
        server.write_all(&[&header[..], &body].concat()).await?;
    }
}

impl Faults {
    pub(super) async fn start() -> Faults {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        let budget = Arc::new(AtomicI64::new(-1));
        let shared = Arc::clone(&budget);
        let db = hyscribe_testkit::fixture_database();
        tokio::spawn(async move {
            while let Ok((client, _)) = listener.accept().await {
                let server = TcpStream::connect((db.host.as_str(), db.port))
                    .await
                    .unwrap();
                // Packets go at once, as without the proxy.
                server.set_nodelay(true).unwrap();
                client.set_nodelay(true).unwrap();
                let (client_read, mut client_write) = client.into_split();
                let (mut server_read, server_write) = server.into_split();
                let budget = Arc::clone(&shared);
                tokio::spawn(async move {
                    let cut = forward(client_read, server_write, budget);
                    let back = tokio::io::copy(&mut server_read, &mut client_write);
                    tokio::select! {
                        _ = cut => {}
                        _ = back => {}
                    }
                });
            }
        });
        Faults { port, budget }
    }

    /// A connection through the proxy, without TLS so its packets can be
    /// read.
    pub(super) async fn connect(&self) -> hyscribe_db::Connection {
        let db = hyscribe_testkit::fixture_database();
        let options = sqlx::mysql::MySqlConnectOptions::new()
            .host("127.0.0.1")
            .port(self.port)
            .database(&db.name)
            .username(&db.user)
            .password(db.password.expose())
            .charset(&db.charset)
            .ssl_mode(sqlx::mysql::MySqlSslMode::Disabled);
        <hyscribe_db::Connection as sqlx::Connection>::connect_with(&options)
            .await
            .unwrap()
    }

    /// Lets `commands` more commands through, then cuts the connection;
    /// `None` lets every command through.
    pub(super) fn arm(&self, commands: Option<i64>) {
        self.budget.store(commands.unwrap_or(-1), Ordering::SeqCst);
    }
}
