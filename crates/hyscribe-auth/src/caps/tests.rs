// SPDX-License-Identifier: AGPL-3.0-or-later

use hyscribe_phpser::Value;
use hyscribe_testkit::unhex;

use super::*;

/// The data `reference/capabilities/golden.sh` inserted and rolled back.
#[derive(Default)]
struct Fixture {
    options: Vec<(String, Vec<u8>)>,
    users: Vec<(u64, Vec<u8>)>,
    posts: Vec<CapPost>,
    post_meta: Vec<(u64, String, Vec<u8>)>,
    comments: Vec<(u64, u64)>,
    terms: Vec<(u64, String)>,
    constants: Vec<String>,
}

impl CapabilityData for Fixture {
    fn option(&self, name: &str) -> Option<Vec<u8>> {
        let found = self.options.iter().rev().find(|(n, _)| n == name);
        found.map(|(_, value)| value.clone())
    }
    fn user_meta(&self, user_id: u64, key: &str) -> Option<Vec<u8>> {
        let found = self.users.iter().find(|(id, _)| *id == user_id);
        found
            .filter(|_| key == "wp_capabilities")
            .map(|(_, caps)| caps.clone())
    }
    fn user_exists(&self, user_id: u64) -> bool {
        self.users.iter().any(|(id, _)| *id == user_id) && user_id != 999_999
    }
    fn post(&self, id: u64) -> Option<CapPost> {
        self.posts.iter().find(|post| post.id == id).cloned()
    }
    fn post_meta(&self, id: u64, key: &str) -> Option<Vec<u8>> {
        let found = self.post_meta.iter().find(|(p, k, _)| *p == id && k == key);
        found.map(|(_, _, value)| value.clone())
    }
    fn comment_post(&self, id: u64) -> Option<u64> {
        self.comments
            .iter()
            .find(|(c, _)| *c == id)
            .map(|(_, post)| *post)
    }
    fn constant(&self, name: &str) -> bool {
        self.constants.iter().any(|c| c == name)
    }
    fn term_taxonomy(&self, id: u64) -> Option<String> {
        self.terms
            .iter()
            .find(|(t, _)| *t == id)
            .map(|(_, taxonomy)| taxonomy.clone())
    }
}

/// Every check of `reference/capabilities/golden.sh`. The capabilities to
/// change code or files are denied, as an intended difference.
#[test]
fn capabilities_match_php() {
    let golden = include_str!("../../tests/golden/capabilities.tsv");
    let mut data = Fixture::default();
    let mut failures = Vec::new();
    let mut checks = 0;
    for line in golden.lines() {
        let f: Vec<&str> = line.split('\t').collect();
        let number = |n: usize| f[n].parse::<u64>().unwrap();
        match f[0] {
            "option" => data.options.push((f[1].to_string(), unhex(f[2]))),
            "user" => data.users.push((number(1), unhex(f[2]))),
            "post" => data.posts.push(CapPost {
                id: number(1),
                author: number(2),
                status: f[3].to_string(),
                post_type: f[4].to_string(),
                parent: number(5),
            }),
            "postmeta" => data
                .post_meta
                .push((number(1), f[2].to_string(), unhex(f[3]))),
            "comment" => data.comments.push((number(1), number(2))),
            "term" => data.terms.push((number(1), f[2].to_string())),
            _ => {
                checks += 1;
                let args = match hyscribe_phpser::decode(&unhex(f[3])).ok() {
                    Some(Value::Array(args)) => args.into_iter().map(|(_, v)| v).collect(),
                    _ => Vec::new(),
                };
                let user = User::load(&data, "wp_", number(1));
                let ours = user.has_cap(&data, f[2], &args);
                let php = f[4] == "1" && !FILE_MODS.contains(&f[2]);
                if ours != php {
                    failures.push(format!(
                        "user {} {} {}: php {php}",
                        f[1],
                        f[2],
                        String::from_utf8_lossy(&unhex(f[3]))
                    ));
                }
            }
        }
    }
    assert!(
        failures.is_empty(),
        "{} of {checks} differ:\n{}",
        failures.len(),
        failures.join("\n")
    );
    assert_eq!(checks, 15972);
}

/// The golden's data, every option as last set.
fn fixture() -> Fixture {
    let golden = include_str!("../../tests/golden/capabilities.tsv");
    let mut data = Fixture::default();
    for line in golden.lines().filter(|line| !line.starts_with("can\t")) {
        let f: Vec<&str> = line.split('\t').collect();
        let number = |n: usize| f[n].parse::<u64>().unwrap();
        match f[0] {
            "option" => data.options.push((f[1].to_string(), unhex(f[2]))),
            "user" => data.users.push((number(1), unhex(f[2]))),
            _ => {}
        }
    }
    data
}

/// The constants WordPress reads from `wp-config.php`: unfiltered HTML and
/// uploads, and multisite, whose rules are not ported, so nothing is granted.
#[test]
fn constants_change_the_capabilities() {
    let mut data = fixture();
    let admin = User::load(&data, "wp_", 1);
    assert!(admin.has_cap(&data, "unfiltered_html", &[]));
    assert!(!admin.has_cap(&data, "unfiltered_upload", &[]));
    data.constants = vec![
        "DISALLOW_UNFILTERED_HTML".into(),
        "ALLOW_UNFILTERED_UPLOADS".into(),
    ];
    assert!(!admin.has_cap(&data, "unfiltered_html", &[]));
    assert!(!admin.has_cap(&data, "edit_css", &[]));
    assert!(admin.has_cap(&data, "unfiltered_upload", &[]));
    data.constants = vec!["MULTISITE".into()];
    assert!(!admin.has_cap(&data, "read", &[]));
}

/// Attachments that are each other's parents have no status, which PHP
/// never finds: reading one then needs `edit_others_posts`.
#[test]
fn a_loop_of_parents_has_no_status() {
    let mut data = fixture();
    for (id, parent) in [(90_001, 90_002), (90_002, 90_001)] {
        data.posts.push(CapPost {
            id,
            author: 1,
            status: "inherit".into(),
            post_type: "attachment".into(),
            parent,
        });
    }
    let subscriber = data.users.iter().map(|(id, _)| *id).find(|id| {
        let user = User::load(&data, "wp_", *id);
        user.has_cap(&data, "read", &[]) && !user.has_cap(&data, "edit_posts", &[])
    });
    let subscriber = User::load(&data, "wp_", subscriber.unwrap());
    let admin = User::load(&data, "wp_", 1);
    assert!(!subscriber.has_cap(&data, "read_post", &[Value::Int(90_001)]));
    assert!(admin.has_cap(&data, "read_post", &[Value::Int(90_001)]));
}

/// A denied capability stays denied for a user granted `do_not_allow`, or
/// any other name, as PHP unsets `do_not_allow` before checking.
#[test]
fn do_not_allow_is_never_granted() {
    let mut data = fixture();
    let caps = br#"a:3:{s:12:"do_not_allow";b:1;s:5:"xyzzy";b:1;s:0:"";b:1;}"#;
    data.users.push((90_000, caps.to_vec()));
    let user = User::load(&data, "wp_", 90_000);
    assert!(user.has_cap(&data, "xyzzy", &[]));
    assert!(!user.has_cap(&data, "delete_site", &[]));
    assert!(!user.has_cap(&data, "do_not_allow", &[]));
}
