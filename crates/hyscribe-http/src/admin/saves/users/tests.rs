// SPDX-License-Identifier: AGPL-3.0-or-later

//! The users list's deletion and password reset links against the write
//! oracle.

use hyscribe_db::{Connection, UserFields};
use hyscribe_testkit::fixture_database;
use hyscribe_testkit::writes::check_writes;
use hyscribe_write::posts::{PostArgs, wp_insert_post};
use hyscribe_write::users::{UserData, wp_insert_user};

use super::super::tests::{Request, acted, new_user, php_location, redirected, writer};
use super::*;

fn dodelete(post: &str) -> Request<'_> {
    Request {
        screen: "users",
        action: b"dodelete",
        get: "",
        post,
        user: 1,
    }
}

/// `users.php`'s deletion writes PHP's rows and answers as PHP: a user
/// deleted with their post, another whose post passes to user 1, and user
/// 1 kept; a request PHP sends back to its form, or that passes content to
/// no user, is not ported, and an option PHP knows not deletes no one.
#[tokio::test]
async fn deletions_are_php_s() {
    let (writer, _turn) = writer().await;
    let setup = async |conn: &mut Connection| {
        let deleted = new_user(&writer, conn, "oracle-deleted", "author").await;
        let heir = new_user(&writer, conn, "oracle-heir", "author").await;
        let mut write = writer.begin_on(conn).await.unwrap();
        let ctx = Context {
            user: 1,
            can_publish: true,
            home: b"http://127.0.0.1:8081".to_vec(),
            upload_host: b"127.0.0.1:8081".to_vec(),
            now: crate::now(),
        };
        for (title, author) in [("Deleted post", deleted), ("Passed post", heir)] {
            let post = PostArgs {
                author: Some(author),
                title: title.into(),
                content: b"Text.".to_vec(),
                status: "publish".into(),
                ..PostArgs::default()
            };
            wp_insert_post(&mut write, &ctx, post).await.unwrap();
        }
        write.commit().await.unwrap();
        Ok::<_, String>((deleted, heir))
    };
    let action = async |conn: &mut Connection, (deleted, heir): (u64, u64)| {
        for post in [
            format!("users[]={deleted}"),
            format!("users[]={deleted}&delete_option=delete"),
            format!("users[]={heir}&delete_option[{heir}]=reassign&reassign_user[{heir}]=-1"),
        ] {
            let unported = acted(&writer, conn, &dodelete(&post), None).await;
            assert!(matches!(unported, Err(Error::Unported(_))), "{post}");
        }
        let kept = format!("users[]={deleted}&delete_option[{deleted}]=keep");
        let to = redirected(&writer, conn, &dodelete(&kept)).await;
        assert_eq!(to, "users?delete_count=1&update=del");
        let post = format!(
            "users[]={deleted}&users[]={heir}&users[]=1&delete_option[{deleted}]=delete\
             &delete_option[{heir}]=reassign&reassign_user[{heir}]=1&delete_option[1]=delete"
        );
        let to = redirected(&writer, conn, &dodelete(&post)).await;
        let php = php_location("users-dodelete-delete");
        assert_eq!(to, php.replace("delete_count=1", "delete_count=2"));
        Ok(())
    };
    let report = check_writes(&fixture_database(), "admin-users-dodelete", setup, action).await;
    report.unwrap();
}

/// `users.php`'s password reset links write PHP's rows and answer as PHP:
/// a key for each user but user 1, one found by its email, which its
/// login is, and none for a login not found once sanitized.
#[tokio::test]
async fn password_reset_links_are_php_s() {
    let (writer, _turn) = writer().await;
    let setup = async |conn: &mut Connection| {
        let mut write = writer.begin_on(conn).await.unwrap();
        let odd = UserFields {
            pass: String::new(),
            nicename: Vec::new(),
            email: b"odd@example.test".to_vec(),
            url: Vec::new(),
            registered: "2026-01-01 00:00:00".into(),
            activation_key: String::new(),
            display_name: b"odd".to_vec(),
        };
        let odd = write.insert_user(b"oracle <i>", &odd).await.unwrap();
        let some = |text: &str| Some(text.as_bytes().to_vec());
        let at = UserData {
            user_login: some("oracle@reset.test"),
            user_pass: some("A pass phrase"),
            user_email: some("oracle@reset.test"),
            role: some("author"),
            ..UserData::default()
        };
        let at = wp_insert_user(&mut write, &at).await.unwrap();
        write.commit().await.unwrap();
        let reset = new_user(&writer, conn, "oracle-reset", "author").await;
        Ok::<_, String>([reset, at, odd])
    };
    let action = async |conn: &mut Connection, [reset, at, odd]: [u64; 3]| {
        let get = format!("users[]={reset}&users[]={at}&users[]={odd}&users[]=1");
        let request = Request {
            screen: "users",
            action: b"resetpassword",
            get: &get,
            post: "",
            user: 1,
        };
        let to = redirected(&writer, conn, &request).await;
        let php = php_location("users-resetpassword");
        assert_eq!(to, php.replace("reset_count=1", "reset_count=2"));
        Ok(())
    };
    let golden = "admin-users-resetpassword";
    let report = check_writes(&fixture_database(), golden, setup, action).await;
    report.unwrap();
}
