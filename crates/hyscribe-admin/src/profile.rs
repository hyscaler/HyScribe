// SPDX-License-Identifier: AGPL-3.0-or-later

//! `profile.php` and `user-edit.php`: a user's profile, the current
//! user's with `IS_PROFILE_PAGE`.

use hyscribe_kses::{esc_attr, esc_html, esc_url, esc_url_raw};
use hyscribe_model::User;
use hyscribe_phpser::{
    Key, Maybe, Value, absint, is_empty_string, maybe_unserialize, remove_query_arg,
};
use hyscribe_render::user_avatar;

use crate::admin_bar::{BarSite, Browser};
use crate::list_table::{checked, fill, selected};
use crate::menu::Screen;
use crate::page::{HelpTab, Page, SCHEMES, admin_page};
use crate::posts::Answer;
use crate::users::{UsersSite, dropdown_roles, role_names};

/// What the profile asks of the site beyond the users list's.
pub trait ProfileSite: UsersSite {
    /// `get_userdata( $id )` with the user's name meta; `None` for none.
    fn profile_user(&self, id: u64) -> Option<User>;
    /// `get_user_option( $name, $id )`, as stored.
    fn user_option_of(&self, id: u64, name: &str) -> Option<Vec<u8>>;
    /// `user_can( $id, $cap )`.
    fn user_can(&self, id: u64, cap: &str) -> bool;
    /// How many of a user's sessions are unexpired.
    fn sessions(&self, id: u64) -> usize;
    /// `wp_generate_password( 24 )`, the password the form offers.
    fn generated_password(&self) -> String;
}

const PAGE: &str = include_str!("profile/page.html");

const HELP_OVERVIEW: &str = "<p>Your profile contains information about you (your &#8220;account&#8221;) as well as some personal options related to using WordPress.</p><p>You can change your password, turn on keyboard shortcuts, change the color scheme of your WordPress administration screens, and turn off the WYSIWYG (Visual) editor, among other things. You can hide the Toolbar (formerly called the Admin Bar) from the front end of your site, however it cannot be disabled on the admin screens.</p><p>You can select the language you wish to use while using the WordPress administration screen without affecting the language site visitors see.</p><p>Your username cannot be changed, but you can use other fields to enter your real name or a nickname, and change which name to display on your posts.</p><p>You can log out of other devices, such as your phone or a public computer, by clicking the Log Out Everywhere Else button.</p><p>Required fields are indicated; the rest are optional. Profile information will only be displayed if your theme is set up to do so.</p><p>Remember to click the Update Profile button when you are finished.</p>";

const HELP_SIDEBAR: &str = "<p><strong>For more information:</strong></p><p><a href=\"https://wordpress.org/documentation/article/users-your-profile-screen/\">Documentation on User Profiles</a></p><p><a href=\"https://wordpress.org/support/forums/\">Support forums</a></p>";

fn text(bytes: &[u8]) -> String {
    String::from_utf8_lossy(bytes).into_owned()
}

/// `admin_color_scheme_picker()`: the default, fresh and light schemes
/// first, then the others by name, `current` chosen.
fn color_picker(site: &dyn ProfileSite, current: &str) -> String {
    let mut schemes: Vec<_> = SCHEMES.iter().collect();
    let first = ["modern", "fresh", "light"];
    schemes.sort_by_key(|scheme| {
        let place = first.iter().position(|name| *name == scheme.name);
        (place.unwrap_or(first.len()), scheme.name)
    });
    let current = if schemes.iter().any(|scheme| scheme.name == current) {
        current
    } else {
        "modern"
    };
    let mut out = format!(
        "<input type=\"hidden\" id=\"color-nonce\" name=\"color-nonce\" value=\"{}\" />",
        site.nonce("save-color-scheme")
    );
    for (i, scheme) in schemes.into_iter().enumerate() {
        let name = scheme.name;
        let chosen = name == current;
        let url = if scheme.stylesheet {
            BarSite::admin_url(site, &format!("css/colors/{name}/colors.min.css"))
        } else {
            String::new()
        };
        let [base, focus, current] = scheme.icons;
        let icons = format!(
            "{{\"icons\":{{\"base\":\"{base}\",\"focus\":\"{focus}\",\"current\":\"{current}\"}}}}"
        );
        let shades: String = scheme
            .palette
            .iter()
            .enumerate()
            .map(|(i, color)| {
                format!(
                    "\t\t\t\t\t\t\t\t\t{}<div class=\"color-palette-shade\" style=\"background-color: {color}\">&nbsp;</div>\n",
                    if i > 0 { "\t" } else { "" }
                )
            })
            .collect();
        out.push_str(&format!(
            "{}\t\t\t<div class=\"color-option {}\">\n\t\t\t\t<input name=\"admin_color\" id=\"admin_color_{name}\" type=\"radio\" value=\"{name}\" {} />\n\t\t\t\t<input type=\"hidden\" class=\"css_url\" value=\"{}\" />\n\t\t\t\t<input type=\"hidden\" class=\"icon_colors\" value=\"{}\" />\n\t\t\t\t<label for=\"admin_color_{name}\">{}</label>\n\t\t\t\t<div class=\"color-palette\">\n{shades}\t\t\t\t\t\t\t\t\t</div>\n\t\t\t</div>\n",
            if i > 0 { "\t\t\t" } else { "" },
            if chosen { "selected" } else { "" },
            checked(chosen),
            text(&esc_url(url.as_bytes())),
            esc_attr(icons.as_bytes()),
            esc_html(scheme.label.as_bytes())
        ));
    }
    out
}

/// A personal option's check box row, opened after `open` and its lines
/// indented by `inner` more, `id` its field, checked when `on`.
fn option_row(
    class: &str,
    heading: &str,
    (open, inner): (usize, &str),
    id: &str,
    on: bool,
    label: &str,
) -> String {
    format!(
        "{}<tr class=\"{class}\">\n{inner}\t\t\t\t\t\t<th scope=\"row\">{heading}</th>\n{inner}\t\t\t\t\t\t<td>\n{inner}\t\t\t\t\t\t\t<label for=\"{id}\"><input name=\"{id}\" type=\"checkbox\" id=\"{id}\" value=\"false\" {} />\n{inner}\t\t\t\t\t\t\t\t{label}{inner}\t\t\t\t\t\t\t</label>\n{inner}\t\t\t\t\t\t</td>\n{inner}\t\t\t\t\t</tr>\n",
        "\t".repeat(open),
        checked(on)
    )
}

/// `sanitize_user_field()` in the edit context: `esc_attr()`.
fn edit(value: &[u8]) -> String {
    esc_attr(value)
}

/// The display name choices: the nickname, login, names and both orders,
/// with the current one first when it is none of them, each once.
fn display_options(user: &User, nickname: &str) -> String {
    let (first, last) = (edit(&user.first_name), edit(&user.last_name));
    let mut names = vec![nickname.to_string(), edit(&user.login)];
    let given = |name: &str| !is_empty_string(name.as_bytes());
    match (given(&first), given(&last)) {
        (true, true) => names.extend([
            first.clone(),
            last.clone(),
            format!("{first} {last}"),
            format!("{last} {first}"),
        ]),
        (true, false) => names.push(first),
        (false, true) => names.push(last),
        (false, false) => {}
    }
    let display = edit(&user.display_name);
    if !names.contains(&display) {
        names.insert(0, display.clone());
    }
    let mut unique: Vec<String> = Vec::new();
    for name in names {
        let name = text(hyscribe_phpser::php_trim(name.as_bytes()));
        if !unique.contains(&name) {
            unique.push(name);
        }
    }
    unique
        .iter()
        .enumerate()
        .map(|(i, name)| {
            let tabs = if i == 0 { 20 } else { 17 };
            let selected = selected(*name == display);
            format!("{}<option {selected}>{name}</option>\n", "\t".repeat(tabs))
        })
        .collect()
}

/// The capabilities granted besides roles, those denied marked, as the
/// Additional Capabilities section lists them; empty when there are none.
fn additional_caps(capabilities: &[u8], roles: &[String]) -> String {
    let caps = match maybe_unserialize(capabilities) {
        Maybe::Value(Value::Array(caps)) => caps,
        _ => Vec::new(),
    };
    let own: Vec<String> = caps
        .iter()
        .filter_map(|(key, granted): &(Key, Value)| {
            let name = text(&key.to_php_string());
            if roles.contains(&name) {
                return None;
            }
            Some(if granted.is_empty() {
                format!("Denied: {name}")
            } else {
                name
            })
        })
        .collect();
    if own.is_empty() {
        return String::new();
    }
    format!(
        "\t\t\t\t\t<h2>Additional Capabilities</h2>\n\n\t\t\t\t\t<table class=\"form-table\" role=\"presentation\">\n\t\t\t\t\t\t<tr class=\"user-capabilities-wrap\">\n\t\t\t\t\t\t\t<th scope=\"row\">Capabilities</th>\n\t\t\t\t\t\t\t<td>\n\t\t\t\t\t\t\t\t{}\t\t\t\t\t\t\t</td>\n\t\t\t\t\t\t</tr>\n\t\t\t\t\t</table>\n\t\t\t\t",
        own.join(", ")
    )
}

/// The notice of a pending email change `stored` as `_new_email`, with a
/// link to cancel it; none for no change or one to the `current` address.
fn pending_email(site: &dyn ProfileSite, stored: &[u8], current: &[u8]) -> String {
    let new = match maybe_unserialize(stored) {
        Maybe::Value(new @ Value::Array(_)) if !new.is_empty() => new,
        _ => return String::new(),
    };
    let email = new.get_str("newemail").map(Value::to_php_string);
    if email.as_deref() == Some(current) {
        return String::new();
    }
    let me = site.user_id();
    let url = BarSite::admin_url(site, &format!("profile.php?dismiss={me}_new_email"));
    format!(
        "<div class=\"notice updated inline\"><p>There is a pending change of your email to <code>{}</code>. <a href=\"{}&amp;_wpnonce={}\">Cancel</a></p></div>",
        esc_html(&email.unwrap_or_default()),
        text(&esc_url(url.as_bytes())),
        site.nonce(&format!("dismiss-{me}_new_email"))
    )
}

/// `_get_admin_bar_pref()`: on when `stored` is unset or `true`.
fn admin_bar_pref(stored: Option<&[u8]>) -> bool {
    stored.is_none_or(|pref| pref == b"true")
}

/// What `user-edit.php` is asked, as requested.
#[derive(Default)]
pub struct EditArgs {
    /// `user_id`.
    pub user_id: Option<String>,
    /// `wp_http_referer`.
    pub referer: Option<String>,
}

/// The request a screen answers.
pub struct Request<'a> {
    /// The requested URL without `wp_removable_query_args()`.
    pub canonical: &'a str,
    /// The scheme and host of the request.
    pub origin: &'a str,
    /// `$_SERVER['REQUEST_URI']`, slashed.
    pub uri: &'a str,
    /// The browser that asks.
    pub browser: &'a Browser<'a>,
}

/// `user-edit.php`'s own: its cleaned `wp_http_referer` and `(int)
/// $_GET['user_id']`, which the user-profile script carries.
struct Edit {
    referer: Vec<u8>,
    script_id: i64,
}

/// `profile.php` for the current user.
pub fn profile(site: &dyn ProfileSite, request: &Request) -> Answer {
    let user = site.profile_user(site.user_id()).unwrap_or_default();
    form(site, &user, None, request)
}

/// `user-edit.php`: a user's profile, the current user's as on
/// `profile.php`; `wp_die()` for no such user or one they may not edit.
pub fn user_edit(site: &dyn ProfileSite, args: &EditArgs, request: &Request) -> Answer {
    let given = |arg: &Option<String>| {
        arg.as_deref()
            .map(|text| Value::String(text.as_bytes().to_vec()))
            .filter(|value| !value.is_empty())
    };
    let id = given(&args.user_id).map_or(0, |id| absint(&id));
    let Some(user) = site.profile_user(id) else {
        return Answer::Died("Invalid user ID.");
    };
    if !site.can_user("edit_user", id) {
        return Answer::Died("Sorry, you are not allowed to edit this user.");
    }
    // `sanitize_url()`, then without the arguments of an update.
    let referer = given(&args.referer).map_or_else(Vec::new, |referer| {
        let referer = esc_url_raw(&referer.to_php_string());
        remove_query_arg(&[b"update", b"delete_count", b"user_id"], &referer)
    });
    let script_id = args
        .user_id
        .as_deref()
        .map_or(0, |id| Value::String(id.as_bytes().to_vec()).to_int());
    let edit = Edit { referer, script_id };
    form(site, &user, Some(&edit), request)
}

/// The rows of the Account Management table after the password's: the
/// reset link and sessions of another user, or the profile's sessions.
fn account_rows(site: &dyn ProfileSite, user: &User, profile: bool) -> (String, String) {
    let sessions = site.sessions(user.id);
    if !profile {
        let name = edit(&user.display_name);
        let reset = format!(
            "\t\t\t\t\t\t\t\t<tr class=\"user-generate-reset-link-wrap hide-if-no-js\">\n\t\t\t\t\t\t\t\t\t<th>Password Reset</th>\n\t\t\t\t\t\t\t\t\t<td>\n\t\t\t\t\t\t\t\t\t\t<div class=\"generate-reset-link\">\n\t\t\t\t\t\t\t\t\t\t\t<button type=\"button\" class=\"button button-secondary\" id=\"generate-reset-link\">\n\t\t\t\t\t\t\t\t\t\t\t\tSend Reset Link\t\t\t\t\t\t\t\t\t\t\t</button>\n\t\t\t\t\t\t\t\t\t\t</div>\n\t\t\t\t\t\t\t\t\t\t<p class=\"description\">\n\t\t\t\t\t\t\t\t\t\t\tSend {} a link to reset their password. This will not change their password, nor will it force a change.\t\t\t\t\t\t\t\t\t\t</p>\n\t\t\t\t\t\t\t\t\t</td>\n\t\t\t\t\t\t\t\t</tr>\n\t\t\t\t\t\t\t",
            esc_html(name.as_bytes())
        );
        let sessions = if sessions == 0 {
            String::new()
        } else {
            format!(
                "\t\t\t\t\t\t\t\t\t\t\t\t\t\t\t<tr class=\"user-sessions-wrap hide-if-no-js\">\n\t\t\t\t\t\t\t\t\t<th>Sessions</th>\n\t\t\t\t\t\t\t\t\t<td>\n\t\t\t\t\t\t\t\t\t\t<p><button type=\"button\" class=\"button\" id=\"destroy-sessions\">Log Out Everywhere</button></p>\n\t\t\t\t\t\t\t\t\t\t<p class=\"description\">\n\t\t\t\t\t\t\t\t\t\t\tLog {name} out of all locations.\t\t\t\t\t\t\t\t\t\t</p>\n\t\t\t\t\t\t\t\t\t</td>\n\t\t\t\t\t\t\t\t</tr>\n"
            )
        };
        return (reset, sessions);
    }
    let sessions = match sessions {
        0 => String::new(),
        1 => "\t\t\t\t\t\t\t\t\t\t\t\t\t\t\t<tr class=\"user-sessions-wrap hide-if-no-js\">\n\t\t\t\t\t\t\t\t\t<th>Sessions</th>\n\t\t\t\t\t\t\t\t\t<td aria-live=\"assertive\">\n\t\t\t\t\t\t\t\t\t\t<div class=\"destroy-sessions\"><button type=\"button\" disabled class=\"button\">Log Out Everywhere Else</button></div>\n\t\t\t\t\t\t\t\t\t\t<p class=\"description\">\n\t\t\t\t\t\t\t\t\t\t\tYou are only logged in at this location.\t\t\t\t\t\t\t\t\t\t</p>\n\t\t\t\t\t\t\t\t\t</td>\n\t\t\t\t\t\t\t\t</tr>\n".into(),
        _ => "\t\t\t\t\t\t\t\t\t\t\t\t\t\t\t<tr class=\"user-sessions-wrap hide-if-no-js\">\n\t\t\t\t\t\t\t\t\t<th>Sessions</th>\n\t\t\t\t\t\t\t\t\t<td aria-live=\"assertive\">\n\t\t\t\t\t\t\t\t\t\t<div class=\"destroy-sessions\"><button type=\"button\" class=\"button\" id=\"destroy-sessions\">Log Out Everywhere Else</button></div>\n\t\t\t\t\t\t\t\t\t\t<p class=\"description\">\n\t\t\t\t\t\t\t\t\t\t\tDid you lose your phone or leave your account logged in at a public computer? You can log out everywhere else, and stay logged in here.\t\t\t\t\t\t\t\t\t\t</p>\n\t\t\t\t\t\t\t\t\t</td>\n\t\t\t\t\t\t\t\t</tr>\n".into(),
    };
    (String::new(), sessions)
}

/// The Role row of another user's form: the editable roles with the
/// user's first chosen, or no role.
fn role_row(roles: &[(String, String)], user_roles: &[String]) -> String {
    let role = user_roles.first().map_or("", String::as_str);
    let none = if role.is_empty() {
        " selected=\"selected\""
    } else {
        ""
    };
    format!(
        "\t\t\t\t\t\t<tr class=\"user-role-wrap\">\n\t\t\t\t\t\t\t<th><label for=\"role\">Role</label></th>\n\t\t\t\t\t\t\t<td>\n\t\t\t\t\t\t\t\t<select name=\"role\" id=\"role\">\n\t\t\t\t\t\t\t\t\t{}<option value=\"\"{none}>&mdash; No role for this site &mdash;</option>\t\t\t\t\t\t\t</select>\n\t\t\t\t\t\t\t</td>\n\t\t\t\t\t\t</tr>\n\t\t\t\t\t",
        dropdown_roles(roles, role)
    )
}

/// What tells the profile from another user's form: the heading, Add User
/// link, role row and email field.
fn heading(
    site: &dyn ProfileSite,
    user: &User,
    roles: (&[(String, String)], &[String]),
) -> [String; 4] {
    let email = edit(&user.email);
    if user.id == site.user_id() {
        return [
            "Profile".into(),
            String::new(),
            String::new(),
            format!(
                "<input type=\"email\" name=\"email\" id=\"email\" aria-describedby=\"email-description\" value=\"{email}\" autocomplete=\"email\" class=\"regular-text ltr\" />\n\t\t\t\t\t\t\t\t<p class=\"description\" id=\"email-description\">\n\t\t\t\t\t\t\t\t\tIf you change this, an email will be sent at your new address to confirm it. <strong>The new address will not become active until confirmed.</strong>\t\t\t\t\t\t\t\t</p>"
            ),
        ];
    }
    let add = if site.can("create_users") {
        "\t\t\t\t\t\t\t\t\t<a href=\"user-new\" class=\"page-title-action\">Add User</a>\n\t\t\t\t\t\t\t"
    } else {
        "\t\t\t\t\t\t\t"
    };
    let role = if site.can_user("promote_user", user.id) {
        role_row(roles.0, roles.1)
    } else {
        String::new()
    };
    [
        format!("Edit User {}", edit(&user.display_name)),
        add.into(),
        role,
        format!(
            "<input type=\"email\" name=\"email\" id=\"email\" value=\"{email}\" class=\"regular-text ltr\" />"
        ),
    ]
}

/// The Personal Options rows the form shows: the visual editor, syntax
/// highlighting, keyboard shortcuts and infinite scrolling.
fn personal_options(site: &dyn ProfileSite, id: u64) -> [String; 4] {
    let is = |name: &str, value: &[u8]| site.user_option_of(id, name).unwrap_or_default() == value;
    let personal = id != site.user_id() || site.can("edit_posts") || site.can("edit_pages");
    let rich = if personal && is("rich_editing", b"false") {
        option_row(
            "user-rich-editing-wrap",
            "Visual Editor",
            (6, "\t"),
            "rich_editing",
            true,
            "Disable the visual editor when writing",
        ) + "\t\t\t\t\t"
    } else {
        String::new()
    };
    let syntax = if ["edit_theme_options", "edit_plugins", "edit_themes"]
        .iter()
        .any(|cap| site.user_can(id, cap))
    {
        option_row(
            "user-syntax-highlighting-wrap",
            "Syntax Highlighting",
            (10, ""),
            "syntax_highlighting",
            is("syntax_highlighting", b"false"),
            "Disable syntax highlighting when editing code",
        )
    } else {
        String::new()
    };
    let shortcuts = if personal {
        format!(
            "\t\t\t\t\t\t\t\t\t\t<tr class=\"user-comment-shortcuts-wrap\">\n\t\t\t\t\t\t<th scope=\"row\">Keyboard Shortcuts</th>\n\t\t\t\t\t\t<td>\n\t\t\t\t\t\t\t<label for=\"comment_shortcuts\">\n\t\t\t\t\t\t\t\t<input type=\"checkbox\" name=\"comment_shortcuts\" id=\"comment_shortcuts\" value=\"true\" {} />\n\t\t\t\t\t\t\t\tEnable keyboard shortcuts for comment moderation.\t\t\t\t\t\t\t</label>\n\t\t\t\t\t\t\t<a href=\"https://wordpress.org/documentation/article/keyboard-shortcuts-classic-editor/#keyboard-shortcuts-for-comments\">Documentation on Keyboard Shortcuts</a>\t\t\t\t\t\t</td>\n\t\t\t\t\t</tr>\n",
            checked(is("comment_shortcuts", b"true"))
        )
    } else {
        String::new()
    };
    let scrolling = if site.user_can(id, "upload_files") {
        option_row(
            "user-infinite-scrolling-wrap",
            "Infinite Scrolling",
            (10, ""),
            "infinite_scrolling",
            is("infinite_scrolling", b"false"),
            "Disable infinite scrolling in the Media Library grid view",
        )
    } else {
        String::new()
    };
    [rich, syntax, shortcuts, scrolling]
}

/// The user's roles, as `WP_User` names them: the keys of their
/// `capabilities` that are roles.
fn roles_of(capabilities: &[u8], roles: &[String]) -> Vec<String> {
    match maybe_unserialize(capabilities) {
        Maybe::Value(Value::Array(caps)) => caps
            .iter()
            .map(|(key, _)| text(&key.to_php_string()))
            .filter(|key| roles.contains(key))
            .collect(),
        _ => Vec::new(),
    }
}

/// The profile form of `user`, on `profile.php`, or on `user-edit.php`
/// when `shown` there.
fn form(site: &dyn ProfileSite, user: &User, shown: Option<&Edit>, request: &Request) -> Answer {
    let me = site.user_id();
    let id = user.id;
    let profile = id == me;
    let option = |name: &str| site.user_option_of(id, name).unwrap_or_default();
    let [rich, syntax, shortcuts, scrolling] = personal_options(site, id);
    let nickname = edit(&option("nickname"));
    let avatar = text(&user_avatar(&|name| site.option(name), user, 96));
    let gravatar = if profile {
        "\t\t\t\t\t\t\t\t\t<a href=\"https://gravatar.com/\">You can change your profile picture on Gravatar</a>.\t\t\t\t\t\t\t\t"
    } else {
        "\t\t\t\t\t\t\t\t\t\t\t\t\t\t\t\t\t"
    };
    let picture = if avatar.is_empty() {
        String::new()
    } else {
        format!(
            "\t\t\t\t\t\t\t\t\t\t\t<tr class=\"user-profile-picture\">\n\t\t\t\t\t\t\t<th>Profile Picture</th>\n\t\t\t\t\t\t\t<td>\n\t\t\t\t\t\t\t\t{avatar}\t\t\t\t\t\t\t\t<p class=\"description\">\n{gravatar}</p>\n\t\t\t\t\t\t\t</td>\n\t\t\t\t\t\t</tr>\n"
        )
    };
    // `is_role()`: the roles WordPress names.
    let roles = role_names(&site.user_roles().unwrap_or_default());
    let keys: Vec<String> = roles.iter().map(|(key, _)| key.clone()).collect();
    let capabilities = option("capabilities");
    let user_roles = roles_of(&capabilities, &keys);
    let [title, add, role, email] = heading(site, user, (&roles, &user_roles));
    let referer = text(&esc_url(&remove_query_arg(
        &[b"_wp_http_referer"],
        request.uri.as_bytes(),
    )));
    let (reset, sessions) = account_rows(site, user, profile);
    let auto = |name: &str| {
        if profile {
            format!(" autocomplete=\"{name}\"")
        } else {
            String::new()
        }
    };
    let either = |mine: &str, other: &str| if profile { mine } else { other }.to_string();
    let wp_referer = shown
        .filter(|edit| !edit.referer.is_empty())
        .map_or_else(String::new, |edit| {
            format!(
                "\t\t\t\t\t\t\t\t\t<input type=\"hidden\" name=\"wp_http_referer\" value=\"{}\" />\n",
                text(&esc_url(&edit.referer))
            )
        });
    let title = esc_html(title.as_bytes());
    let content = fill(
        PAGE,
        &[
            ("heading", title.clone()),
            ("add", add),
            (
                "action",
                text(&esc_url(
                    BarSite::admin_url(site, &either("profile.php", "user-edit.php")).as_bytes(),
                )),
            ),
            ("nonce", site.nonce(&format!("update-user_{id}"))),
            ("referer", referer),
            ("wp_referer", wp_referer),
            ("me", me.to_string()),
            ("rich", rich),
            ("syntax", syntax),
            ("colors", color_picker(site, &text(&option("admin_color")))),
            ("shortcuts", shortcuts),
            (
                "front",
                checked(admin_bar_pref(
                    site.user_option_of(id, "show_admin_bar_front").as_deref(),
                ))
                .into(),
            ),
            ("scrolling", scrolling),
            ("login", edit(&user.login)),
            ("role", role),
            ("first", edit(&user.first_name)),
            ("given", auto("given-name")),
            ("last", edit(&user.last_name)),
            ("family", auto("family-name")),
            ("nickname", nickname.clone()),
            ("nick_auto", auto("nickname")),
            ("display", display_options(user, &nickname)),
            ("email_field", email),
            (
                "pending",
                if profile {
                    pending_email(site, &option("_new_email"), &user.email)
                } else {
                    String::new()
                },
            ),
            ("url", edit(&esc_url(edit(&user.url).as_bytes()))),
            ("about", either("About Yourself", "About the user")),
            ("description", esc_html(&user.description)),
            ("picture", picture),
            ("password", esc_attr(site.generated_password().as_bytes())),
            (
                "pass2",
                either(
                    "Type your new password again.",
                    "Type the new password again.",
                ),
            ),
            ("reset", reset),
            ("sessions", sessions),
            ("caps", additional_caps(&capabilities, &keys)),
            ("user_id", id.to_string()),
            ("submit", either("Update Profile", "Update User")),
        ],
    );
    let help = [HelpTab {
        id: "overview",
        title: "Overview",
        content: HELP_OVERVIEW,
    }];
    let edit_users = site.can("edit_users");
    let users = |on: bool| if on { "users.php" } else { "profile.php" };
    let script_data = shown.map(|edit| {
        let script_id = edit.script_id;
        (
            "user-profile",
            format!(
                "var userProfileL10n = {{\"user_id\":\"{script_id}\",\"nonce\":\"{}\"}};",
                site.nonce(&format!("reset-password-for-{script_id}"))
            ),
        )
    });
    let author = site.author_url(id);
    let page = Page {
        screen: Screen {
            pagenow: if shown.is_some() {
                "user-edit.php"
            } else {
                "profile.php"
            },
            typenow: "",
            taxonomy: "",
            parent_file: Some(users(edit_users)),
            submenu_file: Some(users(edit_users && !profile)),
        },
        id: if shown.is_some() {
            "user-edit"
        } else {
            "profile"
        },
        title: &title,
        help: &help,
        sidebar: HELP_SIDEBAR,
        screen_options: "",
        styles: &[],
        scripts: &["user-profile"],
        late_scripts: &[],
        script_data: script_data.as_slice(),
        script_after: &[],
        late_styles: &[],
        admin_head: "",
        admin_footer: "",
        footer_scripts: "",
        content: &content,
        canonical: request.canonical,
        origin: request.origin,
        view: shown.map(|_| ("View User", author.as_str())),
    };
    admin_page(site, &page, request.browser).map_or(
        Answer::Denied("Sorry, you are not allowed to access this page."),
        Answer::Page,
    )
}

#[cfg(test)]
mod tests;
