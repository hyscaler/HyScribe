// SPDX-License-Identifier: AGPL-3.0-or-later

//! `users.php`: the users list, `WP_Users_List_Table`.

use hyscribe_kses::{esc_attr, esc_html, esc_url};
use hyscribe_model::User;
use hyscribe_phpser::{
    Key, Maybe, Value, add_query_arg, is_empty_string, maybe_unserialize, remove_query_arg,
    urlencode,
};
use hyscribe_render::formatting::{user_email_display, user_name_display};
use hyscribe_render::{number_format, user_avatar};

use crate::admin_bar::{BarSite, Browser};
use crate::dashboard::DashboardSite;
use crate::list_table::{
    Column, Order, Sort, View, bulk_actions, column_headers, fill, pagination, row_actions,
    screen_options, search_box, table_description, views,
};
use crate::menu::{Screen, link};
use crate::page::{HelpTab, Page, admin_page};
use crate::posts::{Answer, hidden_columns, per_page};

/// What the users list asks of the site beyond the dashboard's.
pub trait UsersSite: DashboardSite {
    /// The `{prefix}user_roles` option, as stored.
    fn user_roles(&self) -> Option<Vec<u8>>;
    /// Every user's `{prefix}capabilities`, as `count_users()` reads them.
    fn all_capabilities(&self) -> Vec<Vec<u8>>;
    /// The first `number` users by login, and how many there are.
    fn list_users(&self, number: u64) -> (Vec<ListedUser>, u64);
    /// `current_user_can( $cap, $id )` of a user.
    fn can_user(&self, cap: &str, id: u64) -> bool;
    /// `get_author_posts_url( $id )`.
    fn author_url(&self, id: u64) -> String;
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

const HELP_OVERVIEW: &str = "<p>This screen lists all the existing users for your site. Each user has one of five defined roles as set by the site admin: Site Administrator, Editor, Author, Contributor, or Subscriber. Users with roles other than Administrator will see fewer options in the dashboard navigation when they are logged in, based on their role.</p><p>To add a new user for your site, click the Add User button at the top of the screen or Add User in the Users menu section.</p>";

const HELP_CONTENT: &str = "<p>You can customize the display of this screen in a number of ways:</p><ul><li>You can hide/display columns based on your needs and decide how many users to list per screen using the Screen Options tab.</li><li>You can filter the list of users by User Role using the text links above the users list to show All, Administrator, Editor, Author, Contributor, or Subscriber. The default view is to show all users. Unused User Roles are not listed.</li><li>You can view all posts made by a user by clicking on the number under the Posts column.</li></ul>";

const HELP_ACTIONS: &str = "<p>Hovering over a row in the users list will display action links that allow you to manage users. You can perform the following actions:</p><ul><li><strong>Edit</strong> takes you to the editable profile screen for that user. You can also reach that screen by clicking on the username.</li><li><strong>Delete</strong> brings you to the Delete Users screen for confirmation, where you can permanently remove a user from your site and delete their content. You can also delete multiple users at once by using bulk actions.</li><li><strong>View</strong> takes you to a public author archive which lists all the posts published by the user.</li>";

const HELP_RESET: &str = "<li><strong>Send password reset</strong> sends the user an email with a link to set a new password.</li>";

const HELP_SIDEBAR: &str = "<p><strong>For more information:</strong></p><p><a href=\"https://wordpress.org/documentation/article/users-screen/\">Documentation on Managing Users</a></p><p><a href=\"https://wordpress.org/documentation/article/roles-and-capabilities/\">Descriptions of Roles and Capabilities</a></p><p><a href=\"https://wordpress.org/support/forums/\">Support forums</a></p>";

fn text(bytes: &[u8]) -> String {
    String::from_utf8_lossy(bytes).into_owned()
}

/// The array `stored` unserializes to, or none.
fn array_of(stored: &[u8]) -> Vec<(Key, Value)> {
    match maybe_unserialize(stored) {
        Maybe::Value(Value::Array(array)) => array,
        _ => Vec::new(),
    }
}

/// `wp_roles()->get_names()` of the `stored` roles: each role's key and
/// name, in stored order, those without a name left out.
fn role_names(stored: &[u8]) -> Vec<(String, String)> {
    array_of(stored)
        .iter()
        .filter_map(|(key, role)| {
            let name = role.get_str("name")?.as_bytes()?;
            Some((text(&key.to_php_string()), text(name)))
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
        let edit = if site.can_user("edit_user", id) {
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
        if id != me && site.can_user("delete_user", id) {
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
        if id != me && site.can_user("edit_user", id) {
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
pub fn users(
    site: &dyn UsersSite,
    canonical: &str,
    origin: &str,
    request_uri: &str,
    browser: &Browser,
) -> Answer {
    if !site.can("list_users") {
        return Answer::Denied(
            "<h1>You need a higher level of permission.</h1><p>Sorry, you are not allowed to list users.</p>",
        );
    }
    let per_page = per_page(site, "users_per_page");
    let (found, total) = site.list_users(per_page);
    let pages = total.div_ceil(per_page);
    let roles = role_names(&site.user_roles().unwrap_or_default());
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
    let rows: String = found
        .iter()
        .map(|listed| format!("\n\t{}", listing.row(listed)))
        .collect();
    let (count, avail) = count_users(site, &roles);
    let mut links = vec![View {
        class: "all".into(),
        url: "users.php".into(),
        label: format!(
            "All <span class=\"count\">({})</span>",
            number_format(count)
        ),
        current: true,
    }];
    for (role, name) in &roles {
        if let Some((_, n)) = avail.iter().find(|(key, _)| key == role) {
            links.push(View {
                class: role.clone(),
                url: format!("users.php?role={role}"),
                label: format!(
                    "{name} <span class=\"count\">({})</span>",
                    number_format(*n)
                ),
                current: false,
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
            current: false,
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
    // `wp_dropdown_roles()`, the editable roles last first.
    let options: String = roles
        .iter()
        .rev()
        .map(|(role, name)| {
            format!(
                "\n\t<option value='{}'>{name}</option>",
                esc_attr(role.as_bytes())
            )
        })
        .collect();
    let tablenav = |which: &str| {
        let (id, button) = if which == "bottom" {
            ("new_role2", "changeit2")
        } else {
            ("new_role", "changeit")
        };
        let change = if site.can("promote_users") && !found.is_empty() {
            format!(
                "<label class=\"screen-reader-text\" for=\"{id}\">\n\t\t\tChange role to&hellip;\t\t</label>\n\t\t<select name=\"{id}\" id=\"{id}\">\n\t\t\t<option value=\"\">Change role to&hellip;</option>\n\t\t\t{options}\t\t\t<option value=\"none\">&mdash; No role for this site &mdash;</option>\n\t\t</select>\n\t\t\t<input type=\"submit\" name=\"{button}\" id=\"{button}\" class=\"button\" value=\"Change\"  />"
            )
        } else {
            String::new()
        };
        format!(
            "<div class=\"tablenav {which}\">\n\n\t\t\t\t<div class=\"alignleft actions bulkactions\">\n\t\t\t{}\t\t</div>\n\t\t\t<div class=\"alignleft actions\">\n\t\t\t\t{change}\t\t</div>\n\t\t{}\n\t\t<br class=\"clear\" />\n\t</div>",
            bulk_actions(which, &actions),
            pagination(which, "Users list navigation", total, pages, 1, canonical)
        )
    };
    let referer = String::from_utf8_lossy(&esc_url(&remove_query_arg(
        &[b"_wp_http_referer"],
        request_uri.as_bytes(),
    )))
    .into_owned();
    let order = Order::default();
    let unpaged =
        String::from_utf8_lossy(&remove_query_arg(&[b"paged"], canonical.as_bytes())).into_owned();
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
    let content = fill(
        PAGE,
        &[
            ("heading_links", add),
            ("views", views("Filter users list", &links)),
            ("search", search_box("Search Users", "user", "", &[])),
            (
                "nonce",
                format!(
                    "<input type=\"hidden\" id=\"_wpnonce\" name=\"_wpnonce\" value=\"{}\" /><input type=\"hidden\" name=\"_wp_http_referer\" value=\"{referer}\" />",
                    site.nonce("bulk-users")
                ),
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
            ("bottom", format!("\t{}", tablenav("bottom"))),
        ],
    );
    let screen_options = screen_options(
        &columns,
        &hidden,
        "users_per_page",
        per_page,
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
        admin_footer: "",
        footer_scripts: "",
        content: &content,
        canonical,
        origin,
        view: None,
    };
    admin_page(site, &page, browser).map_or(
        Answer::Denied("Sorry, you are not allowed to access this page."),
        Answer::Page,
    )
}

#[cfg(test)]
pub(crate) mod content;
#[cfg(test)]
mod tests;
