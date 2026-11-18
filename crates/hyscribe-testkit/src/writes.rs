// SPDX-License-Identifier: AGPL-3.0-or-later

//! The write oracle's HyScribe side (docs/TESTING.md, "Write oracle"): runs
//! an action over the fixture database inside a transaction it rolls back,
//! takes the rows it changed with the masks `reference/writes/run.php`
//! applies, and compares them with the golden PHP WordPress recorded.

use std::collections::{BTreeMap, BTreeSet};

use hyscribe_phpser::{Key, Value};
use sqlx::mysql::{MySqlConnectOptions, MySqlRow};
use sqlx::{Column as _, Connection as _, MySqlConnection, Row as _};

use Held::{Digits, TermMap, Whole};

/// How a column holds IDs.
#[derive(Clone, Copy)]
enum Held {
    /// As its whole value.
    Whole,
    /// As runs of digits.
    Digits,
    /// As the parents and children of a `{$taxonomy}_children` option.
    TermMap,
}

/// A core table: its auto increment column, its key columns, and the columns
/// holding IDs of a table.
struct Table {
    name: &'static str,
    id: Option<&'static str>,
    key: &'static [&'static str],
    ids: &'static [(&'static str, &'static str, Held)],
}

const TABLES: [Table; 12] = [
    Table {
        name: "wp_posts",
        id: Some("ID"),
        key: &["ID"],
        ids: &[
            ("ID", "wp_posts", Whole),
            ("post_parent", "wp_posts", Whole),
            ("post_author", "wp_users", Whole),
            ("guid", "wp_posts", Digits),
            ("post_name", "wp_posts", Digits),
        ],
    },
    Table {
        name: "wp_postmeta",
        id: Some("meta_id"),
        key: &["meta_id"],
        ids: &[
            ("meta_id", "wp_postmeta", Whole),
            ("post_id", "wp_posts", Whole),
        ],
    },
    Table {
        name: "wp_comments",
        id: Some("comment_ID"),
        key: &["comment_ID"],
        ids: &[
            ("comment_ID", "wp_comments", Whole),
            ("comment_parent", "wp_comments", Whole),
            ("comment_post_ID", "wp_posts", Whole),
            ("user_id", "wp_users", Whole),
        ],
    },
    Table {
        name: "wp_commentmeta",
        id: Some("meta_id"),
        key: &["meta_id"],
        ids: &[
            ("meta_id", "wp_commentmeta", Whole),
            ("comment_id", "wp_comments", Whole),
        ],
    },
    Table {
        name: "wp_terms",
        id: Some("term_id"),
        key: &["term_id"],
        ids: &[("term_id", "wp_terms", Whole), ("slug", "wp_terms", Whole)],
    },
    Table {
        name: "wp_term_taxonomy",
        id: Some("term_taxonomy_id"),
        key: &["term_taxonomy_id"],
        ids: &[
            ("term_taxonomy_id", "wp_term_taxonomy", Whole),
            ("term_id", "wp_terms", Whole),
            ("parent", "wp_terms", Whole),
        ],
    },
    Table {
        name: "wp_term_relationships",
        id: None,
        key: &["object_id", "term_taxonomy_id"],
        ids: &[
            ("object_id", "wp_posts", Whole),
            ("term_taxonomy_id", "wp_term_taxonomy", Whole),
        ],
    },
    Table {
        name: "wp_termmeta",
        id: Some("meta_id"),
        key: &["meta_id"],
        ids: &[
            ("meta_id", "wp_termmeta", Whole),
            ("term_id", "wp_terms", Whole),
        ],
    },
    Table {
        name: "wp_options",
        id: Some("option_id"),
        key: &["option_name"],
        ids: &[
            ("option_id", "wp_options", Whole),
            ("option_value", "wp_terms", TermMap),
        ],
    },
    Table {
        name: "wp_users",
        id: Some("ID"),
        key: &["ID"],
        ids: &[("ID", "wp_users", Whole)],
    },
    Table {
        name: "wp_usermeta",
        id: Some("umeta_id"),
        key: &["umeta_id"],
        ids: &[
            ("umeta_id", "wp_usermeta", Whole),
            ("user_id", "wp_users", Whole),
            // A post ID a user option keeps, as the Quick Draft's auto-draft.
            ("meta_value", "wp_posts", Whole),
        ],
    },
    Table {
        name: "wp_links",
        id: Some("link_id"),
        key: &["link_id"],
        ids: &[
            ("link_id", "wp_links", Whole),
            ("link_owner", "wp_users", Whole),
        ],
    },
];

/// A row's columns in table order, each value escaped as a golden writes
/// it. Masks find the same digits and dates in an escaped value.
type Row = Vec<(String, Vec<u8>)>;

/// New IDs' tokens by the table they are of and the ID.
type NewIds = BTreeMap<(&'static str, Vec<u8>), Vec<u8>>;

/// Every table of [`TABLES`] (each row tagged with its table's name) and the
/// clock, now and five seconds earlier, in one round trip.
const SNAPSHOT: &str = "SELECT 'clock', UNIX_TIMESTAMP() AS unix, UTC_TIMESTAMP() AS utc, \
    UNIX_TIMESTAMP() - 5 AS earlier, UTC_TIMESTAMP() - INTERVAL 5 SECOND AS earlier_utc; \
    SELECT 'wp_posts', wp_posts.* FROM wp_posts; \
    SELECT 'wp_postmeta', wp_postmeta.* FROM wp_postmeta; \
    SELECT 'wp_comments', wp_comments.* FROM wp_comments; \
    SELECT 'wp_commentmeta', wp_commentmeta.* FROM wp_commentmeta; \
    SELECT 'wp_terms', wp_terms.* FROM wp_terms; \
    SELECT 'wp_term_taxonomy', wp_term_taxonomy.* FROM wp_term_taxonomy; \
    SELECT 'wp_term_relationships', wp_term_relationships.* FROM wp_term_relationships; \
    SELECT 'wp_termmeta', wp_termmeta.* FROM wp_termmeta; \
    SELECT 'wp_options', wp_options.* FROM wp_options; \
    SELECT 'wp_users', wp_users.* FROM wp_users; \
    SELECT 'wp_usermeta', wp_usermeta.* FROM wp_usermeta; \
    SELECT 'wp_links', wp_links.* FROM wp_links";

/// The rows of every table at one time.
struct Snapshot {
    /// The database's clock as Unix seconds and as a UTC date.
    clock: (u64, Vec<u8>),
    /// The clock five seconds earlier, where a run's times start: a test reads its
    /// request's time before [`check_writes`] connects, `wp_user_settings()` a cookie's.
    earlier: (u64, Vec<u8>),
    /// The rows of each table of [`TABLES`] by raw key.
    tables: Vec<BTreeMap<Vec<u8>, Row>>,
}

impl Snapshot {
    fn of(rows: &[MySqlRow]) -> Snapshot {
        let mut snapshot = Snapshot {
            clock: (0, Vec::new()),
            earlier: (0, Vec::new()),
            tables: TABLES.iter().map(|_| BTreeMap::new()).collect(),
        };
        for row in rows {
            // The text protocol gives every value as PHP's mysqli does.
            let mut columns = row.columns().iter().enumerate().map(|(i, column)| {
                let value = row.try_get_unchecked::<Option<Vec<u8>>, _>(i);
                let value = escape(value.ok().flatten().as_deref());
                (column.name().to_string(), value)
            });
            let tag = columns.next().map(|(_, tag)| tag).unwrap_or_default();
            let values: Row = columns.collect();
            let mut tables = TABLES.iter().zip(&mut snapshot.tables);
            match tables.find(|(table, _)| table.name.as_bytes() == tag) {
                Some((table, rows)) => {
                    rows.insert(key(&values, table.key), values);
                }
                None => {
                    let time = |unix, utc| {
                        let seconds = number(value(&values, unix)).unwrap_or_default();
                        (seconds, value(&values, utc).to_vec())
                    };
                    snapshot.clock = time("unix", "utc");
                    snapshot.earlier = time("earlier", "earlier_utc");
                }
            }
        }
        snapshot
    }
}

fn sql_error(err: sqlx::Error) -> String {
    err.to_string()
}

fn value<'a>(row: &'a Row, column: &str) -> &'a [u8] {
    row.iter()
        .find(|(name, _)| name == column)
        .map_or(&[], |(_, value)| value)
}

fn key(row: &Row, columns: &[&str]) -> Vec<u8> {
    let values: Vec<&[u8]> = columns.iter().map(|column| value(row, column)).collect();
    values.join(&b","[..])
}

async fn snapshot(conn: &mut MySqlConnection) -> Result<Snapshot, String> {
    sqlx::raw_sql(SNAPSHOT)
        .fetch_all(conn)
        .await
        .map(|rows| Snapshot::of(&rows))
        .map_err(sql_error)
}

fn number(digits: &[u8]) -> Option<u64> {
    std::str::from_utf8(digits).unwrap_or_default().parse().ok()
}

/// What masks values: the times the run spans and the new IDs' tokens.
struct Masks {
    start: (u64, Vec<u8>),
    end: (u64, Vec<u8>),
    new: NewIds,
}

impl Masks {
    fn is_date(value: &[u8]) -> bool {
        value.len() == 19
            && value.iter().enumerate().all(|(i, &b)| match i {
                4 | 7 => b == b'-',
                10 => b == b' ',
                13 | 16 => b == b':',
                _ => b.is_ascii_digit(),
            })
    }

    /// A `$wp` bcrypt hash: its salt is random.
    fn is_password(value: &[u8]) -> bool {
        let salt = value.strip_prefix(b"$wp$2y$12$").unwrap_or_default();
        salt.len() == 53
            && salt
                .iter()
                .all(|b| b.is_ascii_alphanumeric() || b"./".contains(b))
    }

    /// An upload's folder of the month as `%ym%`, and its sizes in bytes,
    /// which differ by image encoder (ADR 0015), as `%filesize%`.
    fn uploaded(&self, value: &[u8]) -> Vec<u8> {
        // No folder without a date to read its year and month from.
        let date = &self.start.1;
        let folder = date
            .get(..4)
            .zip(date.get(5..7))
            .map(|(year, month)| [year, b"/", month, b"/"].concat());
        let filesize = b"s:8:\"filesize\";i:";
        let mut out = Vec::new();
        let mut rest = value;
        while let Some((&first, tail)) = rest.split_first() {
            if let Some(after) = folder.as_deref().and_then(|f| rest.strip_prefix(f)) {
                out.extend_from_slice(b"%ym%/");
                rest = after;
            } else if let Some(after) = rest.strip_prefix(&filesize[..]) {
                let digits = after.iter().take_while(|b| b.is_ascii_digit()).count();
                out.extend_from_slice(filesize);
                out.extend_from_slice(b"%filesize%");
                rest = after.get(digits..).unwrap_or_default();
            } else {
                out.push(first);
                rest = tail;
            }
        }
        out
    }

    fn mask(&self, table: &Table, column: &str, value: &[u8]) -> Vec<u8> {
        let upload = matches!(
            (table.name, column),
            ("wp_posts", "guid") | ("wp_postmeta", "meta_value")
        );
        let uploaded = upload.then(|| self.uploaded(value));
        let value = uploaded.as_deref().unwrap_or(value);
        if Self::is_password(value) {
            return b"%password%".to_vec();
        }
        let value = &fast_hashes(value)[..];
        // Such dates sort as text in time order.
        if Self::is_date(value) && value >= &self.start.1[..] && value <= &self.end.1[..] {
            return b"%now%".to_vec();
        }
        let value = digit_runs(value, |run| {
            let unix = number(run).filter(|_| run.len() == 10)?;
            (self.start.0..=self.end.0)
                .contains(&unix)
                .then(|| b"%unix%".to_vec())
        });
        let Some((_, of, held)) = table.ids.iter().find(|(name, ..)| *name == column) else {
            return value;
        };
        let token = |id: &[u8]| self.new.get(&(*of, id.to_vec())).cloned();
        match held {
            Whole => token(&value).unwrap_or(value),
            Digits => id_runs(&value, token),
            TermMap => term_map(&value, token).unwrap_or(value),
        }
    }

    /// The `cron` option with the times `wp_reschedule_event()` moves an
    /// event due during the run to, one of the core intervals on, masked.
    fn rescheduled(&self, value: &[u8]) -> Vec<u8> {
        digit_runs(value, |run| {
            let unix = number(run).filter(|_| run.len() == 10)?;
            let during = self.start.0..=self.end.0;
            let interval = [3_600, 43_200, 86_400, 604_800]
                .into_iter()
                .find(|interval| {
                    unix.checked_sub(*interval)
                        .is_some_and(|at| during.contains(&at))
                })?;
            Some(format!("%unix+{interval}%").into_bytes())
        })
    }
}

/// `value` with the hashes of random password reset keys (`$generic$`
/// and up to 40 bytes) masked.
fn fast_hashes(value: &[u8]) -> Vec<u8> {
    let mut out = Vec::new();
    let mut rest = value;
    while let Some(at) = rest.windows(9).position(|w| w == b"$generic$") {
        let (head, _) = rest.split_at(at);
        out.extend_from_slice(head);
        out.extend_from_slice(b"%fasthash%");
        rest = rest.get(at + 49..).unwrap_or_default();
    }
    out.extend_from_slice(rest);
    out
}

/// A `{$taxonomy}_children` map, `a:n:{i:<parent>;a:m:{i:<n>;i:<child>;…}…}`,
/// with its IDs replaced as `replace` says; `None` for any other value.
fn term_map(value: &[u8], replace: impl Fn(&[u8]) -> Option<Vec<u8>>) -> Option<Vec<u8>> {
    let Ok(Value::Array(parents)) = hyscribe_phpser::decode(value) else {
        return None;
    };
    let id = |id: i64| {
        let id = id.to_string().into_bytes();
        replace(&id).unwrap_or(id)
    };
    let mut out = format!("a:{}:{{", parents.len()).into_bytes();
    for (parent, children) in parents {
        let (Key::Int(parent), Value::Array(children)) = (parent, children) else {
            return None;
        };
        out.extend(b"i:");
        out.extend(id(parent));
        out.extend(format!(";a:{}:{{", children.len()).bytes());
        for (n, child) in children {
            let (Key::Int(n), Value::Int(child)) = (n, child) else {
                return None;
            };
            out.extend(format!("i:{n};i:").bytes());
            out.extend(id(child));
            out.push(b';');
        }
        out.push(b'}');
    }
    out.push(b'}');
    Some(out)
}

/// `value` with each maximal run of ASCII digits replaced as `replace` says.
fn digit_runs(value: &[u8], replace: impl Fn(&[u8]) -> Option<Vec<u8>>) -> Vec<u8> {
    let mut out = Vec::new();
    let mut rest = value;
    while let Some(at) = rest.iter().position(u8::is_ascii_digit) {
        let (head, tail) = rest.split_at(at);
        out.extend_from_slice(head);
        let len = tail.iter().take_while(|b| b.is_ascii_digit()).count();
        let (run, tail) = tail.split_at(len);
        out.extend(replace(run).unwrap_or_else(|| run.to_vec()));
        rest = tail;
    }
    out.extend_from_slice(rest);
    out
}

/// `value` with each whole number replaced as `replace` says: a run of
/// digits in a dotted number, such as an IP address's, is left as it is.
fn id_runs(value: &[u8], replace: impl Fn(&[u8]) -> Option<Vec<u8>>) -> Vec<u8> {
    let digit = |at: Option<usize>| {
        at.and_then(|at| value.get(at))
            .is_some_and(u8::is_ascii_digit)
    };
    let dot = |at: Option<usize>| at.and_then(|at| value.get(at)) == Some(&b'.');
    let mut out = Vec::new();
    let mut at = 0;
    while let Some(&b) = value.get(at) {
        let tail = value.get(at..).unwrap_or_default();
        let len = tail.iter().take_while(|b| b.is_ascii_digit()).count();
        if len == 0 {
            out.push(b);
            at += 1;
            continue;
        }
        let run = tail.get(..len).unwrap_or_default();
        let end = at + len;
        let before = dot(at.checked_sub(1)) && digit(at.checked_sub(2));
        let after = dot(Some(end)) && digit(Some(end + 1));
        let token = replace(run).filter(|_| !before && !after);
        out.extend(token.unwrap_or_else(|| run.to_vec()));
        at = end;
    }
    out
}

/// A value as a golden writes it: `\N` for NULL, and backslash, tab, line
/// feed and carriage return escaped, so a value stays on its line.
fn escape(value: Option<&[u8]>) -> Vec<u8> {
    let Some(value) = value else {
        return b"\\N".to_vec();
    };
    let mut out = Vec::new();
    for &byte in value {
        match byte {
            b'\\' => out.extend_from_slice(b"\\\\"),
            b'\t' => out.extend_from_slice(b"\\t"),
            b'\n' => out.extend_from_slice(b"\\n"),
            b'\r' => out.extend_from_slice(b"\\r"),
            _ => out.push(byte),
        }
    }
    out
}

/// IDs in `before` or `after` that `fresh` lacks, in the order of their
/// values, as `%table:n%`.
fn new_ids(fresh: &Snapshot, before: &Snapshot, after: &Snapshot) -> NewIds {
    let mut new = NewIds::new();
    let tables = TABLES
        .iter()
        .zip(&fresh.tables)
        .zip(&before.tables)
        .zip(&after.tables);
    for (((table, fresh), before), after) in tables {
        let Some(id) = table.id else { continue };
        let ids = |rows: &BTreeMap<Vec<u8>, Row>| -> BTreeSet<u64> {
            rows.values()
                .filter_map(|row| number(value(row, id)))
                .collect()
        };
        let added: BTreeSet<u64> = ids(before).union(&ids(after)).copied().collect();
        for (n, id) in added.difference(&ids(fresh)).enumerate() {
            let token = format!("%{}:{}%", table.name, n + 1);
            new.insert(
                (table.name, id.to_string().into_bytes()),
                token.into_bytes(),
            );
        }
    }
    new
}

/// Whether `row` of `table` is a transient's timeout, whose value is the
/// time plus the transient's lifetime.
fn is_timeout(table: &Table, row: &Row) -> bool {
    let name = value(row, "option_name");
    table.name == "wp_options"
        && (name.starts_with(b"_transient_timeout_")
            || name.starts_with(b"_site_transient_timeout_"))
}

/// `value` with each serialized string of 32 hexadecimal digits masked as
/// `%hash%`: the random key of the `adminhash` option or `_new_email` meta.
fn hash_masked(value: &[u8]) -> Vec<u8> {
    let mut out = Vec::new();
    let mut rest = value;
    while let Some((&first, tail)) = rest.split_first() {
        let hash = rest
            .strip_prefix(b"s:32:\"")
            .and_then(|after| after.split_at_checked(32))
            .filter(|(hash, after)| {
                after.starts_with(b"\"") && hash.iter().all(|b| b"0123456789abcdef".contains(b))
            });
        match hash {
            Some((_, after)) => {
                out.extend_from_slice(b"s:32:\"%hash%");
                rest = after;
            }
            None => {
                out.push(first);
                rest = tail;
            }
        }
    }
    out
}

/// Rows of `table` masked, by masked key.
fn masked(masks: &Masks, table: &Table, rows: &BTreeMap<Vec<u8>, Row>) -> BTreeMap<Vec<u8>, Row> {
    rows.values()
        .map(|row| {
            let timeout = is_timeout(table, row);
            let option = (table.name == "wp_options").then(|| value(row, "option_name"));
            let new_email = table.name == "wp_usermeta" && value(row, "meta_key") == b"_new_email";
            let row: Row = row
                .iter()
                .map(|(column, value)| {
                    let masked = masks.mask(table, column, value);
                    let masked = match (column.as_str(), option) {
                        ("option_value", _) if timeout => b"%expiry%".to_vec(),
                        ("option_value", Some(b"adminhash")) => hash_masked(&masked),
                        ("option_value", Some(b"cron")) => masks.rescheduled(&masked),
                        // `send_confirmation_on_profile_email()`'s random key.
                        ("meta_value", _) if new_email => hash_masked(&masked),
                        // A new privacy policy page's ID.
                        ("option_value", Some(b"wp_page_for_privacy_policy")) => {
                            let id = masks.new.get(&("wp_posts", masked.clone()));
                            id.cloned().unwrap_or(masked)
                        }
                        _ => masked,
                    };
                    (column.clone(), masked)
                })
                .collect();
            (key(&row, table.key), row)
        })
        .collect()
}

/// The golden lines for the change from `before` to `after`.
fn lines(masks: &Masks, before: &Snapshot, after: &Snapshot) -> Vec<Vec<u8>> {
    let mut lines = Vec::new();
    for ((table, before), after) in TABLES.iter().zip(&before.tables).zip(&after.tables) {
        let old = masked(masks, table, before);
        let now = masked(masks, table, after);
        let line = |key: &[u8], op: &str, column: &str, values: &[&[u8]]| {
            let mut line = format!("{}\t", table.name).into_bytes();
            line.extend_from_slice(key);
            line.extend(format!("\t{op}\t{column}").bytes());
            for value in values {
                line.push(b'\t');
                line.extend_from_slice(value);
            }
            line
        };
        for (key, row) in &now {
            let Some(was) = old.get(key) else {
                lines.extend(
                    row.iter()
                        .map(|(column, value)| line(key, "+", column, &[value])),
                );
                continue;
            };
            for ((column, value), (_, was)) in row.iter().zip(was) {
                if was != value {
                    lines.push(line(key, "~", column, &[was, value]));
                }
            }
        }
        for (key, row) in old.iter().filter(|(key, _)| !now.contains_key(*key)) {
            for (column, value) in row {
                lines.push(line(key, "-", column, &[value]));
            }
        }
    }
    lines
}

/// Runs `setup` and then `action` on `conn` inside a transaction that is
/// rolled back, and gives the golden lines of the rows `action` changed.
async fn changed_rows<T>(
    conn: &mut MySqlConnection,
    setup: impl AsyncFnOnce(&mut MySqlConnection) -> Result<T, String>,
    action: impl AsyncFnOnce(&mut MySqlConnection, T) -> Result<(), String>,
) -> Result<Vec<Vec<u8>>, String> {
    // Dropping `tx` rolls it back. Errors are combined at the end rather than
    // returned early, so no early return is left untested.
    let mut tx = conn.begin().await.map_err(sql_error)?;
    let fresh = snapshot(&mut tx).await;
    let state = setup(&mut tx).await?;
    let before = snapshot(&mut tx).await;
    action(&mut tx, state).await?;
    let after = snapshot(&mut tx).await;
    fresh.and_then(|fresh| {
        before.and_then(|before| {
            after.map(|after| {
                let new = new_ids(&fresh, &before, &after);
                let (start, end) = (fresh.earlier, after.clock.clone());
                lines(&Masks { start, end, new }, &before, &after)
            })
        })
    })
}

/// The differences between a golden and the lines HyScribe wrote, one a
/// line by table, column and key; empty when they match.
fn differences(golden: &[u8], ours: &[Vec<u8>]) -> Vec<String> {
    let theirs: Vec<&[u8]> = golden
        .split(|&b| b == b'\n')
        .filter(|line| !line.is_empty() && !line.starts_with(b"#"))
        .collect();
    let (php, ours) = (
        cells(theirs),
        cells(ours.iter().map(Vec::as_slice).collect()),
    );
    let all: BTreeSet<&Cell> = php.keys().chain(ours.keys()).collect();
    let show = |values: Option<&String>| {
        values.map_or("nothing".to_string(), |v| {
            format!("{:?}", v.replace('\t', " -> "))
        })
    };
    all.into_iter()
        .filter(|cell| php.get(*cell) != ours.get(*cell))
        .map(|(table, column, key, op)| {
            let cell = (table.clone(), column.clone(), key.clone(), op.clone());
            let (php, ours) = (show(php.get(&cell)), show(ours.get(&cell)));
            format!("{table} {column} ({op} {key}): PHP {php}, HyScribe {ours}")
        })
        .collect()
}

/// A golden line's table, column, key and change.
type Cell = (String, String, String, String);

/// Golden lines' values by their cells.
fn cells(lines: Vec<&[u8]>) -> BTreeMap<Cell, String> {
    lines
        .into_iter()
        .map(|line| {
            let text = String::from_utf8_lossy(line);
            let mut fields = text.splitn(5, '\t');
            let mut field = || fields.next().unwrap_or_default().to_string();
            let (table, key, op, column, values) = (field(), field(), field(), field(), field());
            ((table, column, key, op), values)
        })
        .collect()
}

/// Runs an action as [`changed_rows`] does, over a new connection to `db`
/// (normally [`crate::fixture_database`]), and compares its rows with the
/// golden of the action `golden` that `reference/writes/golden.sh`
/// recorded. The error lists the differences.
pub async fn check_writes<T>(
    db: &hyscribe_config::Database,
    golden: &str,
    setup: impl AsyncFnOnce(&mut MySqlConnection) -> Result<T, String>,
    action: impl AsyncFnOnce(&mut MySqlConnection, T) -> Result<(), String>,
) -> Result<(), String> {
    let path = format!(
        "{}/tests/golden/writes/{golden}.tsv",
        env!("CARGO_MANIFEST_DIR")
    );
    let php = std::fs::read(&path).map_err(|err| format!("{path}: {err}"))?;
    let mut conn = MySqlConnection::connect_with(&options(db))
        .await
        .map_err(|err| format!("{}: {err}", crate::NO_FIXTURE_DATABASE))?;
    let ours = changed_rows(&mut conn, setup, action).await?;
    let differences = differences(&php, &ours);
    if differences.is_empty() {
        Ok(())
    } else {
        Err(format!("{golden}:\n{}", differences.join("\n")))
    }
}

fn options(db: &hyscribe_config::Database) -> MySqlConnectOptions {
    MySqlConnectOptions::new()
        .host(&db.host)
        .port(db.port)
        .database(&db.name)
        .username(&db.user)
        .password(db.password.expose())
        .charset(&db.charset)
}

/// Loads the default install fixture afresh into `db`, as
/// `wordpress_saves`, the scratch database of the tests whose requests
/// really write (`reference/default-install/db.sh` makes it).
pub async fn load_default_install(db: &hyscribe_config::Database) -> Result<(), sqlx::Error> {
    let mut conn = MySqlConnection::connect_with(&options(db)).await?;
    let dump = include_str!("../../../fixtures/default-install/7.1.3/database.sql");
    sqlx::raw_sql(sqlx::AssertSqlSafe(dump))
        .execute(&mut conn)
        .await?;
    Ok(())
}

#[cfg(test)]
mod tests;
