// SPDX-License-Identifier: AGPL-3.0-or-later

//! `users.php`: the users list, `WP_Users_List_Table`.

use hyscribe_kses::{esc_attr, esc_html, esc_url};
use hyscribe_model::USER_ORDERBY;
use hyscribe_model::User;
use hyscribe_phpser::{
    Key, Maybe, Value, add_query_arg, addslashes, is_empty_string, maybe_unserialize, php_trim,
    remove_query_arg, stripslashes, urlencode,
};
use hyscribe_render::formatting::{user_email_display, user_name_display};
use hyscribe_render::{number_format, user_avatar};

use crate::admin_bar::BarSite;
use crate::dashboard::DashboardSite;
use crate::list_table::{
    Column, Order, Sort, View, bulk_actions, column_headers, fill, pagenum, pagination,
    row_actions, screen_options, search_box, table_description, views, without_referer,
    wp_nonce_field,
};
use crate::menu::{Screen, link};
use crate::page::{HelpTab, Page, Request, admin_notice, admin_page, wp_admin_notice};
use crate::posts::{Answer, hidden_columns, per_page};
use crate::text;

/// What the users list asks of the site beyond the dashboard's.
pub trait UsersSite: DashboardSite {
    /// The `{prefix}user_roles` option, as stored.
    fn user_roles(&self) -> Option<Vec<u8>>;
    /// Every user's `{prefix}capabilities`, as `count_users()` reads them.
    fn all_capabilities(&self) -> Vec<Vec<u8>>;
    /// The users `query` finds, and how many there are.
    fn list_users(&self, query: &UserQuery<'_>) -> (Vec<ListedUser>, u64);
    /// `wp_get_users_with_no_role()`: those whose capabilities do not match
    /// `regex`.
    fn users_without_role(&self, regex: &str) -> Vec<u64>;
    /// `get_author_posts_url( $id )`.
    fn author_url(&self, id: u64) -> String;
    /// Whether the user is a post's author or a link's owner.
    fn owns_content(&self, id: u64) -> bool;
}

/// The users the list asks for.
pub struct UserQuery<'a> {
    /// Roles each user has.
    pub roles: &'a [String],
    /// Only these users, unless none.
    pub include: &'a [u64],
    /// The columns searched and the text sought.
    pub search: Option<(&'a [&'static str], &'a str)>,
    /// `ORDER BY` columns of `USER_ORDERBY`, descending where true.
    pub order: &'a [(&'static str, bool)],
    /// How many to skip.
    pub offset: u64,
    /// How many to list.
    pub number: u64,
}

/// The arguments of `users.php` ported so far, as requested.
#[derive(Default)]
pub struct UserArgs {
    /// `role`.
    pub role: Option<String>,
    /// `s`.
    pub search: Option<String>,
    /// `orderby`.
    pub orderby: Option<String>,
    /// `order`.
    pub order: Option<String>,
    /// `paged`.
    pub paged: Option<String>,
    /// `user_order()` of `orderby` and `order`.
    pub clauses: Vec<(&'static str, bool)>,
    /// `update`, the action the list is sent back from, if given.
    pub update: Option<String>,
    /// `delete_count`, the users deleted, if given.
    pub delete_count: Option<String>,
    /// `reset_count`, the users sent a reset link, if given.
    pub reset_count: Option<String>,
    /// `id`, the user added, if given.
    pub id: Option<String>,
    /// `_wp_http_referer` is not empty: the list's own form sent back,
    /// which `users.php` redirects to the list without it.
    pub sent_back: bool,
}

/// `users.php`'s notices of `args`, as printed without `wp_kses_post()`;
/// `request_uri` is slashed.
fn user_notices(site: &dyn UsersSite, args: &UserArgs, request_uri: &str) -> String {
    let count = |value: &Option<String>| {
        value
            .as_deref()
            .map_or(0, |value| Value::String(value.into()).to_int())
    };
    let notice = |message: &str, class: &str| {
        admin_notice(message, "message", &format!("is-dismissible {class}"), true)
    };
    let deleted = count(&args.delete_count);
    let others = |message: &str| {
        if deleted > 0 {
            notice(message, "updated")
        } else {
            String::new()
        }
    };
    match args.update.as_deref() {
        Some("del" | "del_many") => {
            let message = match deleted {
                1 => "User deleted.".into(),
                n => format!("{} users deleted.", number_format(n)),
            };
            notice(&message, "updated")
        }
        Some("add") => {
            let mut message = "New user created.".to_string();
            // `$_GET['id']`, slashed.
            let id = args
                .id
                .as_deref()
                .map(|id| text(&addslashes(id.as_bytes())))
                .filter(|id| !Value::String(id.clone().into_bytes()).is_empty());
            let editable = |id: &String| {
                let id = Value::String(id.clone().into_bytes()).to_int();
                site.can_for("edit_user", id.unsigned_abs())
            };
            if let Some(id) = id.filter(editable) {
                let referer = urlencode(&stripslashes(request_uri.as_bytes()));
                let edit = BarSite::admin_url(site, &format!("user-edit.php?user_id={id}"));
                let url = add_query_arg(&[(b"wp_http_referer", Some(&referer))], edit.as_bytes());
                message.push_str(&format!(
                    " <a href=\"{}\">Edit user</a>",
                    text(&esc_url(&url))
                ));
            }
            notice(&message, "updated")
        }
        Some("resetpassword") => {
            let message = match count(&args.reset_count) {
                1 => "Password reset link sent.".into(),
                n => format!("Password reset links sent to {} users.", number_format(n)),
            };
            notice(&message, "updated")
        }
        Some("promote") => notice("Changed roles.", "updated"),
        Some("err_admin_role") => {
            notice(
                "You cannot change your own role to one that does not allow managing other users. Your role was not changed.",
                "error",
            ) + &others("Other user roles have been changed.")
        }
        Some("err_admin_del") => {
            notice("You cannot delete the current user.", "error")
                + &others("Other users have been deleted.")
        }
        Some("err_missing_reassign") => {
            notice(
                "Users could not be deleted because no user was selected for content reassignment.",
                "error",
            ) + &others("Other users have been deleted.")
        }
        Some("remove") => notice("User removed from this site.", "updated fade"),
        Some("err_admin_remove") => {
            notice("You cannot remove the current user.", "error")
                + &notice("Other users have been removed.", "updated fade")
        }
        _ => String::new(),
    }
}

/// `WP_User_Query`'s `ORDER BY` for `orderby` and `order`, by login when
/// none is valid; `None` for the meta and list orders, not ported, and for
/// the role filter's meta clauses by name when `roles` are asked for.
pub fn user_order(
    orderby: Option<&str>,
    order: Option<&str>,
    roles: bool,
) -> Option<Vec<(&'static str, bool)>> {
    // Ascending when not given, else descending unless it reads ASC.
    let desc = order.is_some_and(|order| !order.eq_ignore_ascii_case("asc"));
    let mut clauses = Vec::new();
    for name in orderby
        .unwrap_or_default()
        .split([',', ' ', '\t', '\n', '\x0b', '\x0c', '\r'])
    {
        let column = match name {
            "" | "0" => continue,
            "login" => "user_login",
            "nicename" => "user_nicename",
            "email" => "user_email",
            "url" => "user_url",
            "registered" => "user_registered",
            "name" | "display_name" => "display_name",
            "ID" | "id" => "ID",
            "meta_value" | "meta_value_num" | "include" | "nicename__in" | "login__in" => {
                return None;
            }
            // `{prefix}usermeta`, `mt1` and so on name the role clauses.
            alias
                if roles
                    && (alias.ends_with("usermeta")
                        || alias.strip_prefix("mt").is_some_and(|n| {
                            !n.is_empty() && n.bytes().all(|b| b.is_ascii_digit())
                        })) =>
            {
                return None;
            }
            other => match USER_ORDERBY.iter().find(|column| **column == other) {
                Some(column) => column,
                None => continue,
            },
        };
        clauses.push((column, desc));
    }
    if clauses.is_empty() {
        clauses.push(("user_login", desc));
    }
    Some(clauses)
}

/// `WP_User_Query`'s search columns for `search`: the email for an
/// address, login and ID for a number, the URL for one, else all but ID.
fn search_columns(search: &str) -> &'static [&'static str] {
    if search.contains('@') {
        &["user_email"]
    } else if Value::String(search.into()).is_numeric() {
        &["user_login", "ID"]
    } else if search.starts_with("http://") || search.starts_with("https://") {
        &["user_url"]
    } else {
        &[
            "user_login",
            "user_url",
            "user_email",
            "user_nicename",
            "display_name",
        ]
    }
}

/// A user on the list.
pub struct ListedUser {
    /// The user.
    pub user: User,
    /// Their `{prefix}capabilities`, as stored.
    pub capabilities: Vec<u8>,
    /// `count_many_users_posts()`: their posts the current user may read.
    pub posts: u64,
}

const PAGE: &str = include_str!("users/page.html");
const DELETE_PAGE: &str = include_str!("users/delete.html");
/// A user's choice of what becomes of their content.
const CONTENT_CHOICE: &str = include_str!("users/content-choice.html");

const HELP_OVERVIEW: &str = "<p>This screen lists all the existing users for your site. Each user has one of five defined roles as set by the site admin: Site Administrator, Editor, Author, Contributor, or Subscriber. Users with roles other than Administrator will see fewer options in the dashboard navigation when they are logged in, based on their role.</p><p>To add a new user for your site, click the Add User button at the top of the screen or Add User in the Users menu section.</p>";

const HELP_CONTENT: &str = "<p>You can customize the display of this screen in a number of ways:</p><ul><li>You can hide/display columns based on your needs and decide how many users to list per screen using the Screen Options tab.</li><li>You can filter the list of users by User Role using the text links above the users list to show All, Administrator, Editor, Author, Contributor, or Subscriber. The default view is to show all users. Unused User Roles are not listed.</li><li>You can view all posts made by a user by clicking on the number under the Posts column.</li></ul>";

const HELP_ACTIONS: &str = "<p>Hovering over a row in the users list will display action links that allow you to manage users. You can perform the following actions:</p><ul><li><strong>Edit</strong> takes you to the editable profile screen for that user. You can also reach that screen by clicking on the username.</li><li><strong>Delete</strong> brings you to the Delete Users screen for confirmation, where you can permanently remove a user from your site and delete their content. You can also delete multiple users at once by using bulk actions.</li><li><strong>View</strong> takes you to a public author archive which lists all the posts published by the user.</li>";

const HELP_RESET: &str = "<li><strong>Send password reset</strong> sends the user an email with a link to set a new password.</li>";

const HELP_SIDEBAR: &str = "<p><strong>For more information:</strong></p><p><a href=\"https://wordpress.org/documentation/article/users-screen/\">Documentation on Managing Users</a></p><p><a href=\"https://wordpress.org/documentation/article/roles-and-capabilities/\">Descriptions of Roles and Capabilities</a></p><p><a href=\"https://wordpress.org/support/forums/\">Support forums</a></p>";

/// The array `stored` unserializes to, or none.
fn array_of(stored: &[u8]) -> Vec<(Key, Value)> {
    match maybe_unserialize(stored) {
        Maybe::Value(Value::Array(array)) => array,
        _ => Vec::new(),
    }
}

/// `wp_roles()->get_names()` of the `stored` roles: each role's key and
/// name, in stored order, those without a name left out.
pub(crate) fn role_names(stored: &[u8]) -> Vec<(String, String)> {
    array_of(stored)
        .iter()
        .filter_map(|(key, role)| {
            let name = role.get_str("name")?.as_bytes()?;
            Some((text(&key.to_php_string()), text(name)))
        })
        .collect()
}

/// `wp_dropdown_roles( $selected )`: the editable roles, last first.
pub(crate) fn dropdown_roles(roles: &[(String, String)], selected: &str) -> String {
    roles
        .iter()
        .rev()
        .map(|(role, name)| {
            let chosen = if role == selected {
                "selected='selected' "
            } else {
                ""
            };
            format!(
                "\n\t<option {chosen}value='{}'>{name}</option>",
                esc_attr(role.as_bytes())
            )
        })
        .collect()
}

/// `count_users()`: how many users there are, and of each role, those of
/// none last, as its queries compare, case aside and `=` ignoring trailing
/// spaces.
fn count_users(site: &dyn UsersSite, roles: &[(String, String)]) -> (u64, Vec<(String, u64)>) {
    let all = site.all_capabilities();
    let mut counts: Vec<(String, u64)> = roles
        .iter()
        .map(|(role, _)| {
            let quoted = format!("\"{role}\"").to_ascii_lowercase();
            let count = all
                .iter()
                .filter(|caps| {
                    String::from_utf8_lossy(caps)
                        .to_ascii_lowercase()
                        .contains(&quoted)
                })
                .count();
            (role.clone(), count as u64)
        })
        .filter(|(_, count)| *count > 0)
        .collect();
    let none = all
        .iter()
        .filter(|caps| {
            let end = caps
                .iter()
                .rposition(|b| *b != b' ')
                .map_or(0, |end| end + 1);
            caps.get(..end)
                .is_some_and(|caps| caps.eq_ignore_ascii_case(b"a:0:{}"))
        })
        .count();
    counts.push(("none".into(), none as u64));
    (all.len() as u64, counts)
}

/// `get_role_list()`: the user's roles WordPress names, keyed, or None.
fn role_list(capabilities: &[u8], roles: &[(String, String)]) -> Vec<(String, String)> {
    let mut list: Vec<(String, String)> = array_of(capabilities)
        .iter()
        .filter_map(|(key, _)| {
            let key = String::from_utf8_lossy(&key.to_php_string()).into_owned();
            let name = roles.iter().find(|(role, _)| *role == key)?.1.clone();
            Some((key, name))
        })
        .collect();
    if list.is_empty() {
        list.push(("none".into(), "None".into()));
    }
    list
}

/// The list's columns.
fn columns() -> Vec<Column> {
    let column = |key, label: &str, sort| Column {
        key,
        label: label.into(),
        sort,
        primary: key == "username",
    };
    vec![
        column("cb", "<input type=\"checkbox\" />", None),
        column(
            "username",
            "Username",
            Some(Sort {
                orderby: "login",
                desc_first: false,
                abbr: "Username",
                caption: "Table ordered by Username.",
                initial: Some("asc"),
            }),
        ),
        column("name", "Name", None),
        column(
            "email",
            "Email",
            Some(Sort {
                orderby: "email",
                desc_first: false,
                abbr: "E-mail",
                caption: "Table ordered by E-mail.",
                initial: None,
            }),
        ),
        column("role", "Role", None),
        column("posts", "Posts", None),
    ]
}

/// The list being printed.
struct Listing<'a> {
    site: &'a dyn UsersSite,
    columns: &'a [Column],
    hidden: &'a [&'a str],
    roles: &'a [(String, String)],
    /// `urlencode()` of the request, the edit links' `wp_http_referer`.
    referer: String,
}

impl Listing<'_> {
    /// `single_row()`.
    fn row(&self, listed: &ListedUser) -> String {
        let site = self.site;
        let user = &listed.user;
        let id = user.id;
        let login = text(&user.login);
        let me = site.user_id();
        let roles = role_list(&listed.capabilities, self.roles);
        let mut actions: Vec<(&str, String)> = Vec::new();
        let edit = if site.can_for("edit_user", id) {
            let base = if id == me {
                BarSite::admin_url(site, "profile.php")
            } else {
                BarSite::admin_url(site, &format!("user-edit.php?user_id={id}"))
            };
            let href = text(&esc_url(&add_query_arg(
                &[(b"wp_http_referer", Some(self.referer.as_bytes()))],
                base.as_bytes(),
            )));
            actions.push(("edit", format!("<a href=\"{href}\">Edit</a>")));
            format!("<strong><a href=\"{href}\">{login}</a></strong><br />")
        } else {
            format!("<strong>{login}</strong><br />")
        };
        let nonce = site.nonce("bulk-users");
        if id != me && site.can_for("delete_user", id) {
            actions.push((
                "delete",
                format!(
                    "<a class='submitdelete' href='{}?action=delete&amp;user={id}&amp;_wpnonce={nonce}'>Delete</a>",
                    link("users.php")
                ),
            ));
        }
        actions.push((
            "view",
            format!(
                "<a href=\"{}\" aria-label=\"{}\">View</a>",
                text(&esc_url(site.author_url(id).as_bytes())),
                esc_attr(
                    format!(
                        "View posts by {}",
                        text(&user_name_display(&user.display_name))
                    )
                    .as_bytes()
                )
            ),
        ));
        if id != me && site.can_for("edit_user", id) {
            actions.push((
                "resetpassword",
                format!(
                    "<a class='resetpassword' href='{}?action=resetpassword&amp;users={id}&amp;_wpnonce={nonce}'>Send password reset</a>",
                    link("users.php")
                ),
            ));
        }
        let classes: Vec<&str> = roles.iter().map(|(role, _)| role.as_str()).collect();
        let names: Vec<&str> = roles.iter().map(|(_, name)| name.as_str()).collect();
        let avatar = text(&user_avatar(&|name| site.option(name), user, 32));
        let mut row = format!("<tr id='user-{id}'>");
        for column in self.columns {
            let key = column.key;
            if key == "cb" {
                row.push_str(&format!(
                    "<td class='check-column'><input type=\"checkbox\" name=\"users[]\" id=\"user_{id}\" class=\"{}\" value=\"{id}\" /><label for=\"user_{id}\"><span class=\"screen-reader-text\">Select {login}</span></label></td>",
                    esc_attr(classes.join(" ").as_bytes())
                ));
                continue;
            }
            let mut classes = format!("{key} column-{key}");
            if column.primary {
                classes.push_str(" has-row-actions column-primary");
            }
            if key == "posts" {
                classes.push_str(" num");
            }
            if self.hidden.contains(&key) {
                classes.push_str(" hidden");
            }
            let label = hyscribe_kses::wp_strip_all_tags(column.label.as_bytes(), false);
            let (tag, extra) = if column.primary {
                (
                    "th",
                    format!(" scope=\"row\" aria-label=\"{}\"", esc_attr(&user.login)),
                )
            } else {
                ("td", String::new())
            };
            row.push_str(&format!(
                "<{tag} class='{classes}' data-colname=\"{}\"{extra}>",
                esc_attr(&label)
            ));
            row.push_str(&match key {
                "username" => format!("{avatar} {edit}{}", row_actions(&actions, false)),
                "name" => name(user),
                "email" => {
                    let email = text(&user_email_display(&user.email));
                    let mailto = format!("mailto:{email}");
                    format!(
                        "<a href='{}'>{email}</a>",
                        text(&esc_url(mailto.as_bytes()))
                    )
                }
                "role" => esc_html(names.join(", ").as_bytes()),
                _ => posts_cell(id, listed.posts),
            });
            row.push_str(&format!("</{tag}>"));
        }
        row.push_str("</tr>");
        row
    }
}

/// The Name column: first and last names as displayed, either, or Unknown.
fn name(user: &User) -> String {
    let (first, last) = (
        text(&user_name_display(&user.first_name)),
        text(&user_name_display(&user.last_name)),
    );
    let empty = |name: &str| is_empty_string(name.as_bytes());
    match (empty(&first), empty(&last)) {
        (false, false) => format!("{first} {last}"),
        (false, true) => first,
        (true, false) => last,
        (true, true) => "<span aria-hidden=\"true\">&#8212;</span><span class=\"screen-reader-text\">Unknown</span>".into(),
    }
}

/// The Posts column: the count, linked to the author's posts when any.
fn posts_cell(id: u64, posts: u64) -> String {
    if posts == 0 {
        return "0".into();
    }
    let noun = if posts == 1 { "post" } else { "posts" };
    format!(
        "<a href=\"{}?author={id}\" class=\"edit\"><span aria-hidden=\"true\">{posts}</span><span class=\"screen-reader-text\">{} {noun} by this author</span></a>",
        link("edit.php"),
        number_format(posts)
    )
}

/// `users.php` for those who may list users.
pub fn users(site: &dyn UsersSite, args: &UserArgs, request: &Request) -> Answer {
    let &Request {
        canonical,
        origin,
        uri: request_uri,
        ..
    } = request;
    if !site.can("list_users") {
        return Answer::Denied(
            "<h1>You need a higher level of permission.</h1><p>Sorry, you are not allowed to list users.</p>",
        );
    }
    if args.sent_back {
        return Answer::Redirect(without_referer(request_uri));
    }
    let per_page = per_page(site, "users_per_page", 20);
    let page = pagenum(args.paged.as_deref());
    let roles = role_names(&site.user_roles().unwrap_or_default());
    // `$_REQUEST['role']` as requested, slashed; `none` lists those of no role.
    let role = text(&addslashes(
        args.role.as_deref().unwrap_or_default().as_bytes(),
    ));
    let include = if role == "none" {
        let names: Vec<&str> = roles.iter().map(|(key, _)| key.as_str()).collect();
        let regex: String = names
            .join("|")
            .chars()
            .filter(|c| c.is_ascii_alphabetic() || "_|-".contains(*c))
            .collect();
        site.users_without_role(&regex)
    } else {
        Vec::new()
    };
    let wanted: Vec<String> = if role == "none" || is_empty_string(role.as_bytes()) {
        Vec::new()
    } else {
        let mut wanted: Vec<String> = Vec::new();
        for name in role.split(',').map(|name| text(php_trim(name.as_bytes()))) {
            if !wanted.contains(&name) {
                wanted.push(name);
            }
        }
        wanted
    };
    // `wp_unslash( trim( $_REQUEST['s'] ) )`, trimmed while slashed as PHP
    // does, so a NUL stays; sought between wildcards.
    let raw = addslashes(args.search.as_deref().unwrap_or_default().as_bytes());
    let usersearch = text(&stripslashes(php_trim(&raw)));
    let sought = usersearch.trim_matches('*');
    let (found, total) = site.list_users(&UserQuery {
        roles: &wanted,
        include: &include,
        search: (!usersearch.is_empty()).then(|| (search_columns(sought), sought)),
        order: &args.clauses,
        offset: (page - 1).saturating_mul(per_page),
        number: per_page,
    });
    let pages = total.div_ceil(per_page);
    if pages > 0 && page > pages {
        let last = pages.to_string();
        let to = add_query_arg(&[(b"paged", Some(last.as_bytes()))], request_uri.as_bytes());
        return Answer::Redirect(text(&to));
    }
    let columns = columns();
    let hidden = hidden_columns(site, "users");
    let hidden_keys: Vec<&str> = hidden.iter().map(String::as_str).collect();
    let listing = Listing {
        site,
        columns: &columns,
        hidden: &hidden_keys,
        roles: &roles,
        referer: String::from_utf8_lossy(&urlencode(request_uri.as_bytes())).into_owned(),
    };
    let rows: String = if found.is_empty() {
        format!(
            "<tr class=\"no-items\"><td class=\"colspanchange\" colspan=\"{}\">No users found.</td></tr>",
            columns
                .iter()
                .filter(|column| !hidden_keys.contains(&column.key))
                .count()
        )
    } else {
        found
            .iter()
            .map(|listed| format!("\n\t{}", listing.row(listed)))
            .collect()
    };
    let (count, avail) = count_users(site, &roles);
    let mut links = vec![View {
        class: "all".into(),
        url: "users.php".into(),
        label: format!(
            "All <span class=\"count\">({})</span>",
            number_format(count)
        ),
        current: is_empty_string(role.as_bytes()),
    }];
    for (key, name) in &roles {
        if let Some((_, n)) = avail.iter().find(|(counted, _)| counted == key) {
            links.push(View {
                class: key.clone(),
                url: format!("users.php?role={key}"),
                label: format!(
                    "{name} <span class=\"count\">({})</span>",
                    number_format(*n)
                ),
                current: *key == role,
            });
        }
    }
    if let Some((_, none)) = avail.iter().find(|(key, n)| key == "none" && *n > 0) {
        links.push(View {
            class: "none".into(),
            url: "users.php?role=none".into(),
            label: format!(
                "No role <span class=\"count\">({})</span>",
                number_format(*none)
            ),
            current: role == "none",
        });
    }
    let actions: Vec<(&str, &str)> = [
        ("delete", "Delete", "delete_users"),
        ("resetpassword", "Send password reset", "edit_users"),
    ]
    .into_iter()
    .filter(|(_, _, cap)| site.can(cap))
    .map(|(action, label, _)| (action, label))
    .collect();
    let options = dropdown_roles(&roles, "");
    // `display_tablenav()` hides the bulk actions of an empty list.
    let bulk_hidden = if found.is_empty() { " hidden" } else { "" };
    let tablenav = |which: &str| {
        let (id, button) = if which == "bottom" {
            ("new_role2", "changeit2")
        } else {
            ("new_role", "changeit")
        };
        let change = if site.can("promote_users") && !found.is_empty() {
            format!(
                "<label class=\"screen-reader-text\" for=\"{id}\">\n\t\t\tChange role to&hellip;\t\t</label>\n\t\t<select name=\"{id}\" id=\"{id}\">\n\t\t\t<option value=\"\">Change role to&hellip;</option>\n\t\t\t{options}\t\t\t<option value=\"none\">&mdash; No role for this site &mdash;</option>\n\t\t</select>\n\t\t\t<input type=\"submit\" name=\"{button}\" id=\"{button}\" class=\"button\" value=\"Change\"  />\t\t"
            )
        } else {
            String::new()
        };
        format!(
            "<div class=\"tablenav {which}\">\n\n\t\t\t\t<div class=\"alignleft actions bulkactions{bulk_hidden}\">\n\t\t\t{}\t\t</div>\n\t\t\t<div class=\"alignleft actions\">\n\t\t\t\t{change}</div>\n\t\t{}\n\t\t<br class=\"clear\" />\n\t</div>",
            bulk_actions(which, &actions),
            pagination(
                which,
                "Users list navigation",
                total,
                pages,
                page,
                canonical
            )
        )
    };
    let order = Order {
        orderby: args.orderby.as_deref(),
        desc: args.order.as_deref() == Some("desc"),
    };
    let unpaged = crate::list_table::columns_url(origin, request_uri);
    let add = if site.can("create_users") {
        format!(
            "<a href=\"{}\" class=\"page-title-action\">Add User</a>",
            String::from_utf8_lossy(&esc_url(
                BarSite::admin_url(site, "user-new.php").as_bytes()
            ))
        )
    } else {
        String::new()
    };
    let subtitle = if usersearch.is_empty() {
        String::new()
    } else {
        format!(
            "<span class=\"subtitle\">Search results for: <strong>{}</strong></span>",
            esc_html(usersearch.as_bytes())
        )
    };
    // The order fields are kept as requested, slashed.
    let slashed =
        |value: &Option<String>| text(&addslashes(value.as_deref().unwrap_or_default().as_bytes()));
    let (orderby, order_arg) = (slashed(&args.orderby), slashed(&args.order));
    let requested = args.search.as_deref().unwrap_or_default();
    let search = if found.is_empty() && is_empty_string(requested.as_bytes()) {
        String::new()
    } else {
        let fields = [("orderby", orderby.as_str()), ("order", order_arg.as_str())];
        search_box("Search Users", "user", requested, &fields)
    };
    let role_field = if is_empty_string(role.as_bytes()) {
        String::new()
    } else {
        format!(
            "\t\t\t<input type=\"hidden\" name=\"role\" value=\"{}\" />\n\t\t",
            esc_attr(role.as_bytes())
        )
    };
    let content = fill(
        PAGE,
        &[
            ("notices", user_notices(site, args, request_uri)),
            ("heading_links", format!("{add}{subtitle}")),
            ("views", views("Filter users list", &links)),
            ("search", search),
            ("role", role_field),
            (
                "nonce",
                wp_nonce_field(site, "bulk-users", "_wpnonce", request_uri),
            ),
            ("top", format!("{}\n\t\t", tablenav("top"))),
            ("caption", table_description(&columns, &order)),
            (
                "head",
                column_headers(&columns, &hidden_keys, &order, &unpaged, 1, true),
            ),
            ("rows", rows),
            (
                "foot",
                column_headers(&columns, &hidden_keys, &order, &unpaged, 2, false),
            ),
            // `display()` leaves the bottom one out of an empty list.
            (
                "bottom",
                if found.is_empty() {
                    String::new()
                } else {
                    format!("\t{}\n\t\t", tablenav("bottom"))
                },
            ),
        ],
    );
    users_page(site, &content, request)
}

/// The users screen around `content`: its help and screen options, which
/// the deletion's confirmation page shares.
fn users_page(site: &dyn UsersSite, content: &str, request: &Request) -> Answer {
    let screen_options = screen_options(
        &columns(),
        &hidden_columns(site, "users"),
        "users_per_page",
        per_page(site, "users_per_page", 20),
        "",
        site.nonce("screen-options-nonce"),
    );
    let actions_help = if site.can("edit_users") {
        format!("{HELP_ACTIONS}{HELP_RESET}</ul>")
    } else {
        format!("{HELP_ACTIONS}</ul>")
    };
    let help = [
        HelpTab {
            id: "overview",
            title: "Overview",
            content: HELP_OVERVIEW,
        },
        HelpTab {
            id: "screen-content",
            title: "Screen Content",
            content: HELP_CONTENT,
        },
        HelpTab {
            id: "action-links",
            title: "Available Actions",
            content: &actions_help,
        },
    ];
    let page = Page {
        screen: Screen {
            pagenow: "users.php",
            typenow: "",
            taxonomy: "",
            parent_file: Some("users.php"),
            submenu_file: None,
        },
        id: "users",
        title: "Users",
        help: &help,
        sidebar: HELP_SIDEBAR,
        screen_options: &screen_options,
        styles: &[],
        scripts: &[],
        late_scripts: &[],
        script_data: &[],
        script_after: &[],
        late_styles: &[],
        admin_head: "",
        admin_footer: "",
        footer_scripts: "",
        content,
        canonical: request.canonical,
        origin: request.origin,
        view: None,
    };
    Answer::from_page(admin_page(site, &page, request.browser))
}

/// What the deletion's confirmation page is asked.
#[derive(Default)]
pub struct DeleteArgs {
    /// `$_REQUEST['users']`, `intval()`ed, else `$_REQUEST['user']`'s; empty
    /// for neither.
    pub users: Vec<i64>,
    /// `error`, set when the form came back with no option chosen.
    pub error: bool,
    /// `wp_http_referer`, unslashed, if given.
    pub referer: Option<String>,
}

/// `users.php`'s `delete`: the confirmation page, each user but the current
/// one with a choice for their content when they own any; with no user,
/// the list or the page `wp_http_referer` names. `None` for a user missing.
pub fn delete_users(site: &dyn UsersSite, args: &DeleteArgs, request: &Request) -> Option<Answer> {
    let removable = [&b"wp_http_referer"[..], b"updated", b"delete_count"];
    let redirect = args
        .referer
        .as_ref()
        .map(|referer| text(&remove_query_arg(&removable, referer.as_bytes())));
    if args.users.is_empty() {
        return Some(Answer::Redirect(redirect.unwrap_or_else(|| "users".into())));
    }
    let me = site.user_id();
    let deleted: Vec<u64> = args
        .users
        .iter()
        .filter_map(|id| u64::try_from(*id).ok())
        .filter(|id| *id != me)
        .collect();
    let mut items = String::new();
    for &id in &args.users {
        let user = site.userdata(u64::try_from(id).unwrap_or_default())?;
        let login = text(&user.login);
        if user.id == me {
            items.push_str(&format!(
                "<li>ID #{id}: {login} <strong>The current user will not be deleted.</strong></li>\n"
            ));
            continue;
        }
        let choice = if site.owns_content(user.id) {
            let others = reassign_dropdown(site, id, &deleted);
            fill(
                CONTENT_CHOICE,
                &[("id", id.to_string()), ("login", login), ("users", others)],
            )
        } else {
            format!(
                "\t\t\t\t\t<input type=\"hidden\" name=\"delete_option[{id}]\" value=\"delete\" required />\n\t\t\t\t\t<p>\n\t\t\t\t\t<strong>{login}</strong> (ID #{id}): This user does not have any content.\t\t\t\t\t</p>\n\t\t\t\t\t"
            )
        };
        items.push_str(&format!(
            "<li><input type=\"hidden\" name=\"users[]\" value=\"{id}\" />{choice}</li>\n"
        ));
    }
    let specified = if args.users.len() == 1 {
        "this user"
    } else {
        "these users"
    };
    let confirm = if args
        .users
        .iter()
        .any(|id| u64::try_from(*id).is_ok_and(|id| id != me))
    {
        "\t\t\t\t\t<input type=\"hidden\" name=\"action\" value=\"dodelete\" />\n\t\t\t<p class=\"submit\"><input type=\"submit\" name=\"submit\" id=\"submit\" class=\"button button-primary\" value=\"Confirm Deletion\"  /></p>\n"
    } else {
        "\t\t\n\t\t\t<p>There are no valid users selected for deletion.</p>\n\n"
    };
    let content = fill(
        DELETE_PAGE,
        &[
            (
                "nonce",
                wp_nonce_field(site, "delete-users", "_wpnonce", request.uri),
            ),
            (
                "referer",
                redirect.map_or_else(String::new, |redirect| {
                    format!(
                        "<input type=\"hidden\" name=\"wp_http_referer\" value=\"{}\" />",
                        esc_attr(redirect.as_bytes())
                    )
                }),
            ),
            (
                "error",
                if args.error {
                    wp_admin_notice(
                        "<strong>Error:</strong> Please select an option.",
                        "",
                        "error",
                        true,
                        "",
                    )
                } else {
                    String::new()
                },
            ),
            ("specified", specified.into()),
            ("items", items),
            ("confirm", confirm.into()),
        ],
    );
    Some(users_page(site, &content, request))
}

/// `wp_dropdown_users()` of the users content of user `id` may pass to:
/// all but those being deleted, by display name.
fn reassign_dropdown(site: &dyn UsersSite, id: i64, deleted: &[u64]) -> String {
    let query = UserQuery {
        roles: &[],
        include: &[],
        search: None,
        order: &[("display_name", false)],
        offset: 0,
        number: u64::MAX,
    };
    let options: String = site
        .list_users(&query)
        .0
        .iter()
        .filter(|listed| !deleted.contains(&listed.user.id))
        .map(|listed| {
            let user = &listed.user;
            let name = [&user.display_name[..], b" (", &user.login, b")"].concat();
            format!(
                "\t<option value='{}'>{}</option>\n",
                user.id,
                esc_html(&name)
            )
        })
        .collect();
    format!(
        "<select name='reassign_user[{id}]' id='reassign_user_{id}' class=''>\n\t<option value=''>Select a user</option>\n{options}</select>"
    )
}

#[cfg(test)]
pub(crate) mod content;
#[cfg(test)]
mod tests;
