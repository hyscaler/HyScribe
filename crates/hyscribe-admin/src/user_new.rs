// SPDX-License-Identifier: AGPL-3.0-or-later

//! The Add User screen, `user-new.php`, on a single site.

use hyscribe_kses::esc_attr;

use crate::list_table::{fill, wp_nonce_field};
use crate::menu::Screen;
use crate::page::{HelpTab, Page, Request, admin_page, wp_admin_notice};
use crate::posts::Answer;
use crate::profile::ProfileSite;
use crate::users::{dropdown_roles, role_names};

const PAGE: &str = include_str!("user_new/page.html");

const HELP_OVERVIEW: &str = "<p>To add a new user to your site, fill in the form on this screen and click the Add User button at the bottom.</p><p>New users are automatically assigned a password, which they can change after logging in. You can view or edit the assigned password by clicking the Show Password button. The username cannot be changed once the user has been added.</p><p>By default, new users will receive an email letting them know they&#8217;ve been added as a user for your site. This email will also contain a password reset link. Uncheck the box if you do not want to send the new user a welcome email.</p><p>Remember to click the Add User button at the bottom of this screen when you are finished.</p>";

const HELP_ROLES: &str = "<p>Here is a basic overview of the different user roles and the permissions associated with each one:</p><ul><li>Subscribers can read comments/comment/receive newsletters, etc. but cannot create regular site content.</li><li>Contributors can write and manage their posts but not publish posts or upload media files.</li><li>Authors can publish and manage their own posts, and are able to upload files.</li><li>Editors can publish posts, manage posts as well as manage other people&#8217;s posts, etc.</li><li>Administrators have access to all the administration features.</li></ul>";

const HELP_SIDEBAR: &str = "<p><strong>For more information:</strong></p><p><a href=\"https://wordpress.org/documentation/article/users-add-new-screen/\">Documentation on Adding New Users</a></p><p><a href=\"https://wordpress.org/support/forums/\">Support forums</a></p>";

/// `user-new.php`'s form to create a user, the role `default_role` names
/// chosen, after the `update` it is sent back with; `wp_die()` for one who
/// may not create users.
pub fn user_new(site: &dyn ProfileSite, update: Option<&str>, request: &Request) -> Answer {
    if !site.can("create_users") {
        return Answer::Denied(
            "<h1>You need a higher level of permission.</h1><p>Sorry, you are not allowed to create users.</p>",
        );
    }
    let role = if site.can("promote_users") {
        let roles = role_names(&site.user_roles().unwrap_or_default());
        let default = site.option("default_role").unwrap_or_default();
        format!(
            "\t<tr class=\"form-field\">\n\t\t<th scope=\"row\"><label for=\"role\">Role</label></th>\n\t\t<td><select name=\"role\" id=\"role\">\n\t\t\t{}\t\t\t</select>\n\t\t</td>\n\t</tr>\n\t\t",
            dropdown_roles(&roles, &String::from_utf8_lossy(&default))
        )
    } else {
        "\t".into()
    };
    let content = fill(
        PAGE,
        &[
            (
                "notice",
                if update == Some("add") {
                    wp_admin_notice("User added.", "message", "is-dismissible updated", true, "")
                } else {
                    String::new()
                },
            ),
            (
                "nonce",
                wp_nonce_field(site, "create-user", "_wpnonce_create-user", request.uri),
            ),
            ("password", esc_attr(site.generated_password().as_bytes())),
            ("role", role),
        ],
    );
    let help = [
        HelpTab {
            id: "overview",
            title: "Overview",
            content: HELP_OVERVIEW,
        },
        HelpTab {
            id: "user-roles",
            title: "User Roles",
            content: HELP_ROLES,
        },
    ];
    let page = Page {
        screen: Screen {
            pagenow: "user-new.php",
            typenow: "",
            taxonomy: "",
            parent_file: Some("users.php"),
            submenu_file: None,
        },
        id: "user",
        title: "Add User",
        help: &help,
        sidebar: HELP_SIDEBAR,
        screen_options: "",
        styles: &[],
        scripts: &["wp-ajax-response", "user-profile"],
        late_scripts: &[],
        script_data: &[],
        admin_footer: "",
        footer_scripts: "",
        content: &content,
        canonical: request.canonical,
        origin: request.origin,
        view: None,
        script_after: &[],
        late_styles: &[],
        admin_head: "",
    };
    Answer::from_page(admin_page(site, &page, request.browser))
}
