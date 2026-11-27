// SPDX-License-Identifier: AGPL-3.0-or-later

#[cfg(unix)]
use std::os::unix::fs::PermissionsExt;

use super::*;

fn fault(at: u64, kind: Kind, hook: &str) -> Counted {
    Counted {
        at,
        kind,
        hook: hook.into(),
    }
}

const ADR: &str = "fault 1795161600 timeout the_content\nfault 1795161642 trap wpcf7_form_elements\nfault 1795161700 memory the_content\ndisabled 1795161700\n";

#[test]
fn reads_the_adr_example_and_writes_it_back() {
    let record = Record::parse(ADR.as_bytes());
    let expected = Record {
        faults: vec![
            fault(1_795_161_600, Kind::Timeout, "the_content"),
            fault(1_795_161_642, Kind::Trap, "wpcf7_form_elements"),
            fault(1_795_161_700, Kind::Memory, "the_content"),
        ],
        disabled: Some(1_795_161_700),
    };
    assert_eq!(record.as_ref(), Some(&expected));
    assert_eq!(expected.text(), ADR);
    assert_eq!(expected.disabled(), Some(1_795_161_700));
    assert_eq!(expected.faults().len(), 3);
}

#[test]
fn counts_three_within_ten_minutes() {
    let mut record = Record::default();
    assert!(!record.count(fault(1_000, Kind::Trap, "a")));
    assert!(!record.count(fault(1_300, Kind::Trap, "b")));
    assert!(!record.count(fault(1_601, Kind::Timeout, "c")));
    assert_eq!(record.disabled(), None);
    assert_eq!(
        record.text(),
        "fault 1000 trap a\nfault 1300 trap b\nfault 1601 timeout c\n"
    );
    assert!(record.count(fault(1_900, Kind::Memory, "d")));
    assert_eq!(record.faults().len(), 3);
    assert_eq!(record.disabled(), Some(1_900));
    assert_eq!(
        Record::parse(record.text().as_bytes()).as_ref(),
        Some(&record)
    );
    let mut back = Record::default();
    back.count(fault(500, Kind::Trap, "a"));
    back.count(fault(400, Kind::Trap, "a"));
    assert_eq!(back.text(), "fault 500 trap a\nfault 500 trap a\n");
}

#[test]
fn refuses_what_it_did_not_write() {
    let refused = [
        "",
        "\n",
        "fault 1 trap a",
        "fault 1 trap a\n\n",
        "fault 1 trap a\r\n",
        "fault 1 trap  a\n",
        "fault 1 trap a b\n",
        "fault 1 trap\n",
        "fault 1\n",
        "fault\n",
        "fault 1 Trap a\n",
        "fault 1 crash a\n",
        "fault -1 trap a\n",
        "fault +1 trap a\n",
        "fault 1.0 trap a\n",
        "fault  trap a\n",
        "fault 123456789012345678901 trap a\n",
        "fault 99999999999999999999 trap a\n",
        "fault 2 trap a\nfault 1 trap a\n",
        "fault 1 trap a\nfault 2 trap a\nfault 3 trap a\n",
        "fault 1 trap a\nfault 2 trap a\nfault 3 trap a\nfault 4 trap a\ndisabled 4\n",
        "fault 1 trap a\nfault 2 trap a\nfault 900 trap a\nfault 901 trap a\n",
        "fault 1 trap a\nfault 2 trap a\nfault 900 trap a\ndisabled 900\n",
        "fault 1 trap a\nfault 2 trap a\nfault 3 trap a\ndisabled 2\n",
        "fault 1 trap a\nfault 2 trap a\nfault 3 trap a\ndisabled 3\ndisabled 3\n",
        "fault 1 trap a\nfault 2 trap a\nfault 3 trap a\ndisabled 3 x\n",
        "fault 1 trap a\nfault 2 trap a\nfault 3 trap a\ndisabled\n",
        "fault 1 trap a\nfault 2 trap a\nfault 3 trap a\ndisabled x\n",
        "disabled 3\n",
        "fault 1 trap a\ndisabled 1\n",
        "fault 1 trap a\nnote 1\n",
        "fault 1 trap caf\u{e9}\n",
        "fault 1 trap a\tb\n",
    ];
    for text in refused {
        assert_eq!(Record::parse(text.as_bytes()), None, "{text:?}");
    }
    assert_eq!(Record::parse(b"fault 1 trap \xff\n"), None);
    let long = format!("fault 1 trap {}\n", "h".repeat(256));
    assert_eq!(Record::parse(long.as_bytes()), None);
}

#[test]
fn reads_the_edges_it_allows() {
    let accepted = [
        "fault 0 trap a\n",
        "fault 18446744073709551615 trap a\n",
        "fault 00000000000000000001 trap a\n",
        "fault 5 trap a\nfault 5 timeout b\n",
        "fault 1 trap a\nfault 2 trap a\nfault 601 trap a\ndisabled 601\n",
        "fault 1 trap a\nfault 2 trap a\nfault 602 trap a\n",
        "fault 1 trap !~\n",
    ];
    for text in accepted {
        let record = Record::parse(text.as_bytes());
        assert!(record.is_some(), "{text:?}");
    }
    let long = format!("fault 1 trap {}\n", "h".repeat(255));
    assert!(Record::parse(long.as_bytes()).is_some());
}

#[test]
fn reads_writes_and_clears_its_file() -> std::io::Result<()> {
    let folder = tempfile::tempdir()?;
    assert_eq!(read(folder.path()), Some(Record::default()));
    let mut record = Record::default();
    record.count(fault(7, Kind::Trap, "the_content"));
    write(folder.path(), &record)?;
    assert_eq!(read(folder.path()), Some(record.clone()));
    #[cfg(unix)]
    let mode = fs::metadata(folder.path().join(FILE))?.permissions().mode();
    #[cfg(unix)]
    assert_eq!(mode & 0o777, 0o600);
    assert!(!folder.path().join(NEW).exists());
    record.count(fault(8, Kind::Memory, "the_content"));
    write(folder.path(), &record)?;
    assert_eq!(read(folder.path()), Some(record));
    clear(folder.path())?;
    assert!(!folder.path().join(FILE).exists());
    clear(folder.path())?;
    Ok(())
}

#[test]
fn fails_closed_on_what_it_cannot_read_or_write() -> std::io::Result<()> {
    let folder = tempfile::tempdir()?;
    fs::write(folder.path().join(FILE), "garbage\n")?;
    assert_eq!(read(folder.path()), None);
    let padded = format!("{ADR}{}", " ".repeat(5000));
    fs::write(folder.path().join(FILE), padded)?;
    assert_eq!(read(folder.path()), None);
    fs::remove_file(folder.path().join(FILE))?;
    fs::create_dir(folder.path().join(FILE))?;
    assert_eq!(read(folder.path()), None);
    assert!(write(folder.path(), &Record::default()).is_err());
    fs::create_dir(folder.path().join(FILE).join("x"))?;
    assert!(clear(folder.path()).is_err());
    let missing = folder.path().join("missing");
    assert!(write(&missing, &Record::default()).is_err());
    let plain = folder.path().join("plain");
    fs::write(&plain, "")?;
    // Windows reports a path through a file as missing, not as an error.
    #[cfg(unix)]
    assert_eq!(read(&plain), None);
    Ok(())
}
