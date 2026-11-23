// SPDX-License-Identifier: AGPL-3.0-or-later

use super::*;

fn set(names: &[&str]) -> BTreeSet<String> {
    names.iter().map(|name| (*name).into()).collect()
}

fn grants() -> Grants {
    Grants {
        actions: set(&["init", "wpcf7_event"]),
        filters: set(&["the_content", "option_wpcf7"]),
        shortcodes: set(&["contact-form-7"]),
        sensitive: set(&["map_meta_cap", "option_siteurl"]),
        options: set(&["wpcf7"]),
        meta: set(&["_form", "_yoast_wpseo_*"]),
        post_types: set(&["wpcf7_contact_form"]),
        taxonomies: set(&["wpcf7_kind"]),
        read_taxonomies: set(&["category"]),
        rest: set(&["contact-form-7/v1"]),
        mail: true,
        filesystem: set(&["uploads/wpcf7_uploads"]),
        transients: set(&["wpcf7_*"]),
        site_transients: set(&["wpcf7_site"]),
        roles: set(&["wpseo_manager"]),
        capabilities: set(&["wpseo_manage_options"]),
        hosts: set(&["www.google.com"]),
    }
}

#[test]
fn sensitive_hooks_by_name_and_by_option() {
    let grants = grants();
    for hook in SENSITIVE {
        assert!(grants.is_sensitive(hook), "{hook}");
    }
    assert!(grants.is_sensitive("option_siteurl"));
    assert!(grants.is_sensitive("pre_option_siteurl"));
    assert!(grants.is_sensitive("pre_update_option_siteurl"));
    assert!(!grants.is_sensitive("option_wpcf7"));
    assert!(!grants.is_sensitive("pre_option_wpcf7"));
    assert!(!grants.is_sensitive("pre_update_option_wpcf7"));
    assert!(!grants.is_sensitive("update_option_siteurl"));
    assert!(!grants.is_sensitive("the_content"));
}

#[test]
fn checks_the_manifest_before_loading() {
    assert_eq!(grants().check(), Ok(()));
    let mut named = grants();
    named.filters.insert("user_has_cap".into());
    assert_eq!(
        named.check(),
        Err("user_has_cap is a sensitive hook: name it under [hooks] sensitive".into())
    );
    let mut named = grants();
    named.actions.insert("pre_update_option_home".into());
    assert_eq!(
        named.check(),
        Err("pre_update_option_home is a sensitive hook: name it under [hooks] sensitive".into())
    );
    for list in [0, 1, 2] {
        let mut refused = grants();
        [
            &mut refused.actions,
            &mut refused.filters,
            &mut refused.sensitive,
        ]
        .into_iter()
        .nth(list)
        .map(|hooks| hooks.insert("phpmailer_init".into()));
        assert_eq!(
            refused.check(),
            Err("phpmailer_init is not a hook extensions attach to".into())
        );
    }
    let mut every = grants();
    every.meta.insert("*".into());
    assert_eq!(
        every.check(),
        Err("a meta grant names its keys or their prefix".into())
    );
}

#[test]
fn hooks_by_kind_and_sensitivity() {
    let grants = grants();
    assert_eq!(grants.hook("init", true), Ok(()));
    assert_eq!(
        grants.hook("init", false),
        Err("init is not in the manifest".into())
    );
    assert_eq!(grants.hook("the_content", false), Ok(()));
    assert_eq!(
        grants.hook("the_content", true),
        Err("the_content is not in the manifest".into())
    );
    assert_eq!(grants.hook("map_meta_cap", false), Ok(()));
    assert_eq!(grants.hook("map_meta_cap", true), Ok(()));
    assert_eq!(grants.hook("option_siteurl", false), Ok(()));
    assert_eq!(grants.hook("option_wpcf7", false), Ok(()));
    assert_eq!(
        grants.hook("user_has_cap", false),
        Err("user_has_cap is not in the manifest".into())
    );
    assert_eq!(grants.cron("wpcf7_event"), Ok(()));
    assert_eq!(
        grants.cron("wp_version_check"),
        Err("wp_version_check is not in the manifest".into())
    );
}

#[test]
fn names_by_grant() {
    let grants = grants();
    assert_eq!(grants.option("wpcf7"), Ok(()));
    assert_eq!(
        grants.option("siteurl"),
        Err("option siteurl is not in the manifest".into())
    );
    assert_eq!(grants.meta("_form"), Ok(()));
    assert_eq!(grants.meta("_yoast_wpseo_title"), Ok(()));
    assert_eq!(grants.meta("_yoast_wpseo_"), Ok(()));
    assert_eq!(
        grants.meta("_form_x"),
        Err("meta key _form_x is not in the manifest".into())
    );
    assert_eq!(
        grants.meta("_yoast_wpse"),
        Err("meta key _yoast_wpse is not in the manifest".into())
    );
    assert_eq!(grants.post_type("wpcf7_contact_form"), Ok(()));
    assert_eq!(
        grants.post_type("page"),
        Err("post type page is not in the manifest".into())
    );
    assert_eq!(grants.taxonomy("wpcf7_kind"), Ok(()));
    assert_eq!(
        grants.taxonomy("category"),
        Err("taxonomy category is not in the manifest".into())
    );
    assert_eq!(grants.rest("contact-form-7/v1"), Ok(()));
    assert_eq!(
        grants.rest("wp/v2"),
        Err("REST namespace wp/v2 is not in the manifest".into())
    );
    assert_eq!(grants.shortcode("contact-form-7"), Ok(()));
    assert_eq!(
        grants.shortcode("gallery"),
        Err("shortcode gallery is not in the manifest".into())
    );
    assert_eq!(grants.transient("wpcf7_x", false), Ok(()));
    assert_eq!(grants.transient("wpcf7_site", true), Ok(()));
    assert_eq!(
        grants.transient("wpcf7_site_x", true),
        Err("site transient wpcf7_site_x is not in the manifest".into())
    );
    assert_eq!(
        grants.transient("doing_cron", false),
        Err("transient doing_cron is not in the manifest".into())
    );
    assert_eq!(grants.role("wpseo_manager"), Ok(()));
    assert_eq!(
        grants.role("editor"),
        Err("role editor is not in the manifest".into())
    );
    assert_eq!(grants.capability("wpseo_manage_options"), Ok(()));
    assert_eq!(
        grants.capability("manage_options"),
        Err("capability manage_options is not in the manifest".into())
    );
    assert!(grants.host("WWW.google.com"));
    assert!(!grants.host("google.com"));
}

#[test]
fn mail_and_its_attachments() {
    let grants = grants();
    let files = |paths: &[&str]| {
        paths
            .iter()
            .map(|path| (*path).into())
            .collect::<Vec<String>>()
    };
    assert_eq!(grants.mail(&[]), Ok(()));
    assert_eq!(
        grants.mail(&files(&["uploads/wpcf7_uploads/a1/cv.pdf"])),
        Ok(())
    );
    for outside in [
        "uploads/wpcf7_uploads",
        "uploads/wpcf7_uploadsx/cv.pdf",
        "uploads/wpcf7_uploads/../../wp-config.php",
        "uploads/wpcf7_uploads//cv.pdf",
        "uploads/wpcf7_uploads/./cv.pdf",
        "uploads/wpcf7_uploads/a\\cv.pdf",
        "uploads/other/cv.pdf",
        "/uploads/wpcf7_uploads/cv.pdf",
    ] {
        assert_eq!(
            grants.mail(&files(&["uploads/wpcf7_uploads/ok.pdf", outside])),
            Err(format!("{outside} is not in a folder the manifest grants")),
        );
    }
    let quiet = Grants::default();
    assert_eq!(quiet.mail(&[]), Err("mail is not in the manifest".into()));
}

#[test]
fn relative_paths() {
    assert!(relative("public/js/app.js"));
    assert!(relative("a"));
    for path in [
        "", "/a", "a/", "a//b", "./a", "a/../b", "..", "a\\b", "c:a", "a\0",
    ] {
        assert!(!relative(path), "{path:?}");
    }
}

#[test]
fn lists_what_it_asks_for() {
    let listed = grants().listed();
    let labels: Vec<&str> = listed.iter().map(|(label, _)| *label).collect();
    assert_eq!(
        labels,
        [
            "Options",
            "Post types",
            "Meta keys",
            "Taxonomies it registers",
            "Taxonomies it reads",
            "REST namespaces",
            "Actions",
            "Filters",
            "Shortcodes",
            "Mail",
            "Mail attachments from",
            "Transients",
            "Site transients",
            "Roles it adds",
            "Capabilities it gives roles",
            "Sends requests to",
        ]
    );
    assert_eq!(listed[1].1, ["wpcf7_contact_form"]);
    assert_eq!(listed[9].1, ["wp_mail()"]);
    assert_eq!(Grants::default().listed(), []);
}
