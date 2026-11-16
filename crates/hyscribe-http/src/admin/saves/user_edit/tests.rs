// SPDX-License-Identifier: AGPL-3.0-or-later

//! Add User, Edit User, the profile and the user's Ajax writes against the
//! write oracle, answering as `reference/admin-saves/golden.sh` recorded.

use hyscribe_db::Connection;
use hyscribe_testkit::fixture_database;
use hyscribe_testkit::writes::check_writes;

use super::super::tests::{
    Request, acted, answered, new_user, password_is, php_answer, php_location, redirected, turn,
    writer,
};
use super::*;

fn request<'q>(screen: &'q str, action: &'q [u8], post: &'q str) -> Request<'q> {
    Request {
        screen,
        action,
        get: "",
        post,
        user: 1,
    }
}

/// What PHP cannot go on with, or shows again: not ported.
async fn unported(writer: &hyscribe_db::Writer, conn: &mut Connection, request: &Request<'_>) {
    let answer = acted(writer, conn, request, None).await;
    assert!(
        matches!(answer, Err(Error::Unported(_))),
        "{}",
        request.post
    );
}

/// `user-new.php` makes PHP's users, told of their account or not, and
/// sends one who may list users to the list; a login PHP refuses, or a
/// form with errors, is not ported.
#[tokio::test]
async fn new_users_are_php_s() {
    let _turn = turn().await;
    let writer = writer().await;
    let action = async |conn: &mut Connection, ()| {
        let long = format!(
            "user_login={}&email=long%40example.test&pass1=a&pass2=a",
            "a".repeat(61)
        );
        for post in [
            long.as_str(),
            "user_login[]=x&email=x%40example.test&pass1=a&pass2=a",
            "user_login=&email=x%40example.test&pass1=a&pass2=a",
            "email=x%40example.test&pass1=a&pass2=a",
            "user_login=bad%21login&email=x%40example.test&pass1=a&pass2=a",
            "user_login=x&email=x%40example.test&pass1=a&pass2=b",
            "user_login=x&email=bad&pass1=a&pass2=a",
            "user_login=x&pass1=a&pass2=a",
            "user_login=x&email=admin%40default-install.test&pass1=a&pass2=a",
            "user_login=admin&email=x%40example.test&pass1=a&pass2=a",
        ] {
            unported(&writer, conn, &request("user-new", b"createuser", post)).await;
        }
        let php = php_location("user-new-create");
        let php = php.rsplit_once('=').unwrap().0;
        // The password is hashed trimmed and slashed, as PHP hashes it.
        for (post, password) in [
            (
                "user_login=oracle-new&email=oracle-new%40example.test&first_name=O%27New\
                 &last_name=User&url=%2Fhome&pass1=+It%27s+a+pass+&pass2=+It%27s+a+pass+\
                 &role=editor&send_user_notification=1&createuser=Add+User",
                &b"It\\'s a pass"[..],
            ),
            (
                "user_login=oracle-quiet&email=oracle-quiet%40example.test&url=example.test%2Fhome\
                 &pass1=Quiet&pass2=Quiet&role=&createuser=Add+User",
                b"Quiet",
            ),
        ] {
            let to = redirected(&writer, conn, &request("user-new", b"createuser", post)).await;
            let (to, id) = to.rsplit_once('=').unwrap();
            assert_eq!(to, php);
            assert!(password_is(&writer, conn, id.parse().unwrap(), password).await);
        }
        Ok(())
    };
    let nothing = async |_: &mut Connection| Ok::<_, String>(());
    let report = check_writes(&fixture_database(), "admin-user-new", nothing, action).await;
    report.unwrap();
}

/// One who may create users but not list them, nor give roles, makes a
/// user of the default role and is sent back to the form.
#[tokio::test]
async fn new_users_of_one_who_cannot_list_them_are_php_s() {
    let _turn = turn().await;
    let writer = writer().await;
    let setup = async |conn: &mut Connection| {
        let id = new_user(&writer, conn, "oracle-creator", "subscriber").await;
        let mut write = writer.begin_on(conn).await.unwrap();
        let caps = hyscribe_phpser::unserialize(
            b"a:2:{s:10:\"subscriber\";b:1;s:12:\"create_users\";b:1;}",
        );
        let none = Value::Null;
        update_metadata(
            &mut write,
            Meta::User,
            id,
            "wp_capabilities",
            &caps.unwrap(),
            &none,
        )
        .await
        .unwrap();
        write.commit().await.unwrap();
        Ok::<_, String>(id)
    };
    let action = async |conn: &mut Connection, id: u64| {
        let post = "user_login=oracle-created&email=oracle-created%40example.test\
                    &pass1=Created&pass2=Created&role=editor";
        let request = Request {
            user: id,
            ..request("user-new", b"createuser", post)
        };
        assert_eq!(
            redirected(&writer, conn, &request).await,
            "user-new?update=add"
        );
        Ok(())
    };
    let golden = "admin-user-new-unlisted";
    let report = check_writes(&fixture_database(), golden, setup, action).await;
    report.unwrap();
}

/// `user-edit.php` saves every field of another user as PHP does, and
/// sends them back to the user's screen with the referer; a role no one
/// may give dies, and a form with errors is not ported.
#[tokio::test]
async fn edited_users_are_php_s() {
    let _turn = turn().await;
    let writer = writer().await;
    let setup = async |conn: &mut Connection| {
        Ok::<_, String>(new_user(&writer, conn, "oracle-edited", "author").await)
    };
    let action = async |conn: &mut Connection, id: u64| {
        let user = format!("user_id={id}&email=e%40example.test&nickname=Nick&pass1=&pass2=");
        for post in [
            format!("user_id={id}&email=e%40example.test&nickname=&pass1=&pass2="),
            format!("{user}&first_name[]=x"),
            format!("{user}&wp_http_referer[]=x"),
            format!("{user}&pass1=a"),
        ] {
            unported(&writer, conn, &request("user-edit", b"update", &post)).await;
        }
        let died = acted(
            &writer,
            conn,
            &request("user-edit", b"update", &format!("{user}&role=bogus")),
            None,
        )
        .await;
        let message = "Sorry, you are not allowed to give users that role.";
        assert!(matches!(died, Ok(Answer::Denied(died)) if died == message));
        let post = format!(
            "user_id={id}&wp_http_referer=%2Fhs-admin%2Fusers%3Frole%3Dauthor%26update%3Dx\
             &email=oracle-edited-2%40example.test&url=https%3A%2F%2Fexample.test%2F%22me%22\
             &first_name=+First+%3Cb%3EName%3C%2Fb%3E+&last_name=Last&nickname=Nick&display_name=Nick\
             &description=+About+%3Cscript%3Ex%3C%2Fscript%3E+me+%0A&role=editor&locale=site-default\
             &rich_editing=false&syntax_highlighting=false&admin_color=ocean&comment_shortcuts=true\
             &use_ssl=1&pass1=A+new+pass&pass2=A+new+pass"
        );
        let to = redirected(&writer, conn, &request("user-edit", b"update", &post)).await;
        let php = php_location("user-edit-update").replace("801", &id.to_string());
        assert_eq!(to, php);
        assert!(password_is(&writer, conn, id, b"A new pass").await);
        Ok(())
    };
    let report = check_writes(&fixture_database(), "admin-user-edit", setup, action).await;
    report.unwrap();
}

/// The profile saves user 1 as PHP does: unchanged from each of its forms,
/// an address of their own or not an address left as it is, and then a new
/// password and an address that waits for its confirmation; the role asked
/// for is not given, as it cannot promote users.
#[tokio::test]
async fn profiles_are_php_s() {
    let _turn = turn().await;
    let writer = writer().await;
    let action = async |conn: &mut Connection, ()| {
        for case in [
            "profile-no-id",
            "user-edit-self",
            "profile-update",
            "profile-email-taken",
        ] {
            let line = include_str!("../../../../../../reference/admin-saves/cases.list")
                .lines()
                .find(|line| line.starts_with(&format!("{case}\t")))
                .unwrap();
            let fields: Vec<&str> = line.split('\t').collect();
            let screen = fields[2].trim_end_matches(".php");
            let post = fields[3].replace("other%40example.test", "ADMIN%40default-install.test");
            let to = redirected(&writer, conn, &request(screen, b"update", &post)).await;
            assert_eq!(to, php_location(case), "{case}");
        }
        let post = "user_id=1&email=not-an-email&nickname=admin&display_name=admin&pass1=&pass2=";
        let to = redirected(&writer, conn, &request("profile", b"update", post)).await;
        assert_eq!(to, php_location("profile-email"));
        let post = "user_id=1&email=new-admin%40example.test&url=&nickname=admin&display_name=admin\
                    &admin_color=fresh&role=editor&admin_bar_front=1&locale=\
                    &pass1=Another+pass&pass2=Another+pass";
        let to = redirected(&writer, conn, &request("profile", b"update", post)).await;
        assert_eq!(to, php_location("profile-password"));
        assert!(password_is(&writer, conn, 1, b"Another pass").await);
        Ok(())
    };
    let nothing = async |_: &mut Connection| Ok::<_, String>(());
    let report = check_writes(&fixture_database(), "admin-profile", nothing, action).await;
    report.unwrap();
}

/// The color scheme is saved as PHP saves it and answered with PHP's JSON.
#[tokio::test]
async fn color_schemes_are_php_s() {
    let _turn = turn().await;
    let writer = writer().await;
    let action = async |conn: &mut Connection, ()| {
        let post = "color_scheme=ocean";
        let request = request("admin-ajax", b"save-user-color-scheme", post);
        let Ok(Answer::Page(json)) = answered(&writer, conn, &request).await else {
            panic!("no JSON");
        };
        assert_eq!(json.into_bytes(), php_answer("color-scheme").2);
        Ok(())
    };
    let golden = "admin-color-scheme";
    let nothing = async |_: &mut Connection| Ok::<_, String>(());
    let report = check_writes(&fixture_database(), golden, nothing, action).await;
    report.unwrap();
}

/// Edit User's password reset link keeps PHP's key and answers PHP's JSON.
#[tokio::test]
async fn password_reset_links_are_php_s() {
    let _turn = turn().await;
    let writer = writer().await;
    let setup = async |conn: &mut Connection| {
        Ok::<_, String>(new_user(&writer, conn, "oracle-link", "author").await)
    };
    let action = async |conn: &mut Connection, id: u64| {
        let post = format!("user_id={id}");
        let request = request("admin-ajax", b"send-password-reset", &post);
        let Ok(Answer::Page(json)) = answered(&writer, conn, &request).await else {
            panic!("no JSON");
        };
        let php = String::from_utf8(php_answer("send-password-reset").2).unwrap();
        assert_eq!(json, php.replace("other", "oracle-link"));
        Ok(())
    };
    let golden = "admin-send-password-reset";
    let report = check_writes(&fixture_database(), golden, setup, action).await;
    report.unwrap();
}

/// The current user's capabilities read only the roles, theirs and the
/// configuration's constants.
#[test]
fn only_the_roles_and_the_user_s_capabilities_are_read() {
    let mut current = Current {
        id: 1,
        prefix: "wp_".into(),
        roles: None,
        caps: None,
        constants: Constants::default(),
    };
    assert!(current.post(1).is_none() && current.post_meta(1, "k").is_none());
    assert!(current.comment_post(1).is_none() && current.term_taxonomy(1).is_none());
    assert!(!current.constant("MULTISITE") && !current.can("list_users"));
    assert!(current.constant("DISALLOW_UNFILTERED_HTML"));
    current.constants.disallow_unfiltered_html = false;
    assert!(!current.constant("DISALLOW_UNFILTERED_HTML"));
}
