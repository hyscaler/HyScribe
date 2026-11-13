// SPDX-License-Identifier: AGPL-3.0-or-later

//! The admin bar on hs-admin's screens: `WP_Admin_Bar` with the nodes core
//! adds in the admin, for a single site. Everyone here may `read`, as the
//! admin turns away anyone who may not, so every item has a link; and only
//! plugins nest groups in groups or menus three deep, so containers and
//! arrows are left out.

use hyscribe_kses::{esc_attr, esc_url, str_contains, wp_strip_all_tags};
use hyscribe_render::number_format;

use crate::menu::MenuUser;

/// The browser a page is for: its `User-Agent` and `Sec-CH-UA-Mobile`
/// headers.
#[derive(Clone, Copy, Default)]
pub struct Browser<'a> {
    /// `User-Agent`.
    pub user_agent: &'a [u8],
    /// `Sec-CH-UA-Mobile`, if sent.
    pub mobile_hint: Option<&'a [u8]>,
}

impl Browser<'_> {
    /// `wp_is_mobile()`.
    pub fn is_mobile(&self) -> bool {
        match self.mobile_hint {
            Some(hint) => hint == b"?1",
            None => [
                &b"Mobile"[..],
                b"Android",
                b"Silk/",
                b"Kindle",
                b"BlackBerry",
                b"Opera Mini",
                b"Opera Mobi",
            ]
            .iter()
            .any(|name| str_contains(self.user_agent, name)),
        }
    }
}

/// What the admin bar shows of the site and the current user.
pub trait BarSite: MenuUser {
    /// `home_url( '/' )`; without its slash, `get_home_url()`.
    fn home_url(&self) -> String;
    /// `admin_url( $path )`.
    fn admin_url(&self, path: &str) -> String;
    /// `get_bloginfo( 'name' )`, as stored.
    fn blogname(&self) -> String;
    /// `wp_logout_url()`, as `wp_nonce_url()` escapes it.
    fn logout_url(&self) -> String;
    /// The current user's `display_name` and `user_login`.
    fn names(&self) -> (String, String);
    /// `get_avatar( $user_id, $size )`.
    fn avatar(&self, size: i64) -> String;
}

/// A node's `meta`.
#[derive(Default)]
struct Meta {
    class: String,
    menu_title: String,
    onclick: &'static str,
    html: String,
}

/// A node: an item, or a group of them.
struct Node {
    id: String,
    title: String,
    parent: String,
    href: String,
    group: bool,
    meta: Meta,
    children: Vec<usize>,
}

/// `WP_Admin_Bar`'s nodes, in the order they were added.
#[derive(Default)]
struct Bar {
    nodes: Vec<Node>,
}

impl Bar {
    fn add(&mut self, id: &str, parent: &str, title: String, href: String, meta: Meta) {
        self.push(id, parent, (title, href), meta, false);
    }

    fn add_group(&mut self, id: &str, parent: &str, class: &str) {
        let meta = Meta {
            class: class.into(),
            ..Meta::default()
        };
        self.push(id, parent, (String::new(), String::new()), meta, true);
    }

    fn push(
        &mut self,
        id: &str,
        parent: &str,
        (title, href): (String, String),
        meta: Meta,
        group: bool,
    ) {
        self.nodes.push(Node {
            id: id.into(),
            title,
            parent: parent.into(),
            href,
            group,
            meta,
            children: Vec::new(),
        });
    }

    fn find(&self, id: &str) -> Option<usize> {
        self.nodes.iter().position(|node| node.id == id)
    }

    /// `_bind()`: the nodes as a tree under `root`, with a `-default` group
    /// between an item and its items; `root`'s place.
    fn bind(&mut self) -> usize {
        let root = self.nodes.len();
        self.add("root", "", String::new(), String::new(), Meta::default());
        let bound: Vec<(usize, String, usize)> = self
            .nodes
            .iter()
            .enumerate()
            .filter(|(_, node)| node.id != "root")
            .filter_map(|(at, node)| {
                let parent = if node.parent.is_empty() {
                    "root"
                } else {
                    &node.parent
                };
                Some((at, parent.to_string(), self.find(parent)?))
            })
            .collect();
        let id = |nodes: &[Node], at: usize| {
            nodes
                .get(at)
                .map(|node| node.id.clone())
                .unwrap_or_default()
        };
        for (at, parent_id, mut parent) in bound {
            let class = if parent_id == "root" {
                "ab-top-menu"
            } else {
                "ab-submenu"
            };
            let group = self.nodes.get(at).is_some_and(|node| node.group);
            let parent_group = self.nodes.get(parent).is_some_and(|node| node.group);
            if let Some(node) = self.nodes.get_mut(at).filter(|node| node.group) {
                let meta = &mut node.meta;
                meta.class = if meta.class.is_empty() {
                    class.into()
                } else {
                    format!("{} {class}", meta.class)
                };
            }
            if !parent_group && !group {
                let default_id = format!("{}-default", id(&self.nodes, parent));
                parent = match self.find(&default_id) {
                    Some(default) => default,
                    None => {
                        let default = self.nodes.len();
                        self.add_group(&default_id, &parent_id, class);
                        self.nodes
                            .get_mut(parent)
                            .into_iter()
                            .for_each(|p| p.children.push(default));
                        default
                    }
                };
            }
            let parent_name = id(&self.nodes, parent);
            self.nodes
                .get_mut(at)
                .into_iter()
                .for_each(|node| node.parent = parent_name.clone());
            self.nodes
                .get_mut(parent)
                .into_iter()
                .for_each(|p| p.children.push(at));
        }
        root
    }

    /// A node's children.
    fn children<'a>(&'a self, node: &'a Node) -> impl Iterator<Item = &'a Node> {
        node.children.iter().filter_map(|&at| self.nodes.get(at))
    }

    /// `_render_group()`, of a group with items.
    fn render_group(&self, node: &Node, menu_title: &str, out: &mut String) {
        let class = format!(" class=\"{}\"", esc_attr(node.meta.class.trim().as_bytes()));
        let id = esc_attr(format!("wp-admin-bar-{}", node.id).as_bytes());
        if menu_title.is_empty() {
            out.push_str(&format!("<ul role='menu' id='{id}'{class}>"));
        } else {
            out.push_str(&format!(
                "<ul role='menu' aria-label='{}' id='{id}'{class}>",
                esc_attr(menu_title.as_bytes())
            ));
        }
        for child in self.children(node) {
            self.render_item(child, out);
        }
        out.push_str("</ul>");
    }

    /// `_render_item()`, of an item with a link.
    fn render_item(&self, node: &Node, out: &mut String) {
        let parent = !node.children.is_empty();
        let mut aria = String::from(" role=\"menuitem\"");
        let mut class = String::new();
        if parent {
            class.push_str("menupop ");
            aria.push_str(" aria-expanded=\"false\"");
        }
        class.push_str(&node.meta.class);
        let class = if class.is_empty() {
            String::new()
        } else {
            format!(" class=\"{}\"", esc_attr(class.trim().as_bytes()))
        };
        let id = esc_attr(format!("wp-admin-bar-{}", node.id).as_bytes());
        let href = String::from_utf8_lossy(&esc_url(node.href.as_bytes())).into_owned();
        out.push_str(&format!(
            "<li role='group' id='{id}'{class}><a class='ab-item'{aria} href='{href}'"
        ));
        if !node.meta.onclick.is_empty() {
            out.push_str(&format!(" onclick='{}'", node.meta.onclick));
        }
        out.push_str(&format!(">{}</a>", node.title));
        if parent {
            out.push_str("<div class=\"ab-sub-wrapper\">");
            for group in self.children(node) {
                self.render_group(group, &node.meta.menu_title, out);
            }
            out.push_str("</div>");
        }
        out.push_str(&node.meta.html);
        out.push_str("</li>");
    }
}

/// `wp_html_excerpt()`: the text without tags, cut to `count` characters
/// and without a broken entity at its end, with `more` when it was cut.
pub(crate) fn html_excerpt(text: &str, count: usize, more: &str) -> String {
    let text = String::from_utf8_lossy(&wp_strip_all_tags(text.as_bytes(), true)).into_owned();
    let mut excerpt: Vec<u8> = text.chars().take(count).collect::<String>().into_bytes();
    // `preg_replace( '/&[^;\s]{0,6}$/', '', $excerpt )`, on bytes: from the
    // first `&` that reaches the end with at most six bytes of neither.
    let entity = excerpt.iter().enumerate().position(|(at, &b)| {
        let tail = excerpt.get(at + 1..).unwrap_or_default();
        b == b'&'
            && tail.len() <= 6
            && !tail
                .iter()
                .any(|c| matches!(c, b';' | b' ' | b'\t' | b'\n' | b'\x0B' | b'\x0C' | b'\r'))
    });
    if let Some(at) = entity {
        excerpt.truncate(at);
    }
    if excerpt == text.as_bytes() {
        text
    } else {
        format!(
            "{}{more}",
            String::from_utf8_lossy(hyscribe_phpser::php_trim(&excerpt))
        )
    }
}

/// The command palette's script, which labels its shortcut for Apple systems.
const PALETTE_SCRIPT: &str = "<script>\n( \t( applePattern, appleOSLabel ) => {\n\t\tif ( ! ( new RegExp( applePattern, 'i' ) ).test( navigator.userAgent ) ) {\n\t\t\treturn;\n\t\t}\n\t\tconst kbd = document.querySelector( '#wp-admin-bar-command-palette .ab-label kbd' );\n\t\tif ( kbd ) {\n\t\t\tkbd.textContent = appleOSLabel;\n\t\t}\n\t} )( \"Macintosh|Mac OS X|Mac_PowerPC\", \"\\u2318K\" );\n//# sourceURL=wp_admin_bar_command_palette_menu\n</script>\n";

/// The admin bar on an admin screen, for `browser`.
pub fn admin_bar(site: &dyn BarSite, browser: &Browser) -> String {
    let mut bar = Bar::default();
    let icon = "<span class=\"ab-icon\" aria-hidden=\"true\"></span>";
    let profile = site.admin_url("profile.php");
    let (display_name, login) = site.names();
    // wp_admin_bar_my_account_menu(), at priority 0.
    bar.add_group("user-actions", "my-account", "");
    let mut info = site.avatar(64);
    info.push_str(&format!("<span class='display-name'>{display_name}</span>"));
    if display_name != login {
        info.push_str(&format!("<span class='username'>{login}</span>"));
    }
    info.push_str("<span class='display-name edit-profile'>Edit Profile</span>");
    bar.add(
        "user-info",
        "user-actions",
        info,
        profile.clone(),
        Meta::default(),
    );
    bar.add(
        "logout",
        "user-actions",
        "Log Out".into(),
        site.logout_url(),
        Meta::default(),
    );
    // wp_admin_bar_sidebar_toggle().
    let toggle = format!("{icon}<span class=\"screen-reader-text\">Menu</span>");
    bar.add("menu-toggle", "", toggle, "#".into(), Meta::default());
    // wp_admin_bar_wp_menu().
    let meta = Meta {
        menu_title: "About WordPress".into(),
        ..Meta::default()
    };
    let logo = format!("{icon}<span class=\"screen-reader-text\">About WordPress</span>");
    bar.add("wp-logo", "", logo, site.admin_url("about.php"), meta);
    bar.add(
        "about",
        "wp-logo",
        "About WordPress".into(),
        site.admin_url("about.php"),
        Meta::default(),
    );
    bar.add(
        "contribute",
        "wp-logo",
        "Get Involved".into(),
        site.admin_url("contribute.php"),
        Meta::default(),
    );
    for (id, title, href) in [
        ("wporg", "WordPress.org", "https://wordpress.org/"),
        (
            "documentation",
            "Documentation",
            "https://wordpress.org/documentation/",
        ),
        ("learn", "Learn WordPress", "https://learn.wordpress.org/"),
        (
            "support-forums",
            "Support",
            "https://wordpress.org/support/forums/",
        ),
        (
            "feedback",
            "Feedback",
            "https://wordpress.org/support/forum/requests-and-feedback",
        ),
    ] {
        bar.add(
            id,
            "wp-logo-external",
            title.into(),
            href.into(),
            Meta::default(),
        );
    }
    // wp_admin_bar_site_menu(), without a site icon.
    let mut blogname = site.blogname();
    if hyscribe_phpser::is_empty_string(blogname.as_bytes()) {
        // `get_home_url()`, which has no trailing slash.
        let home = site.home_url();
        let home = home.strip_suffix('/').unwrap_or(&home);
        let bare = home
            .trim_start_matches("https://")
            .trim_start_matches("http://");
        blogname = bare.strip_prefix("www.").unwrap_or(bare).to_string();
    }
    let title = html_excerpt(&blogname, 40, "&hellip;");
    let meta = Meta {
        menu_title: title.clone(),
        ..Meta::default()
    };
    bar.add("site-name", "", title, site.home_url(), meta);
    bar.add(
        "view-site",
        "site-name",
        "Visit Site".into(),
        site.home_url(),
        Meta::default(),
    );
    // wp_admin_bar_command_palette_menu().
    let agent = browser.user_agent.to_ascii_lowercase();
    let apple = ["macintosh", "mac os x", "mac_powerpc"]
        .iter()
        .any(|name| agent.windows(name.len()).any(|w| w == name.as_bytes()));
    let shortcut = if apple { "⌘K" } else { "Ctrl+K" };
    let palette = format!(
        "{icon}<span class=\"ab-label\"><kbd>{shortcut}</kbd><span class=\"screen-reader-text\"> Open command palette</span></span>"
    );
    let meta = Meta {
        class: "hide-if-no-js".into(),
        onclick: "wp.data.dispatch( &quot;core/commands&quot; ).open(); return false;",
        html: PALETTE_SCRIPT.into(),
        ..Meta::default()
    };
    bar.add("command-palette", "", palette, "#".into(), meta);
    // wp_admin_bar_comments_menu().
    if site.can("edit_posts") {
        let moderated = site.moderated_comments();
        let number = number_format(moderated);
        let noun = if moderated == 1 {
            "Comment"
        } else {
            "Comments"
        };
        let title = format!(
            "{icon}<span class=\"ab-label awaiting-mod pending-count count-{moderated}\" aria-hidden=\"true\">{number}</span><span class=\"screen-reader-text comments-in-moderation-text\">{number} {noun} in moderation</span>"
        );
        bar.add(
            "comments",
            "",
            title,
            site.admin_url("edit-comments.php"),
            Meta::default(),
        );
    }
    // wp_admin_bar_new_content_menu().
    let actions: Vec<(&str, &str, &str)> = [
        ("post-new.php", "Post", "new-post", "edit_posts"),
        ("media-new.php", "Media", "new-media", "upload_files"),
        ("link-add.php", "Link", "new-link", "manage_links"),
        (
            "post-new.php?post_type=page",
            "Page",
            "new-page",
            "edit_pages",
        ),
        ("user-new.php", "User", "new-user", "create_users"),
    ]
    .into_iter()
    .filter(|(_, _, _, cap)| site.can(cap))
    .map(|(link, title, id, _)| (link, title, id))
    .collect();
    if let Some((first, _, _)) = actions.first() {
        let title = format!("{icon}<span class=\"ab-label\">New</span>");
        let meta = Meta {
            menu_title: "New".into(),
            ..Meta::default()
        };
        bar.add("new-content", "", title, site.admin_url(first), meta);
        for (link, title, id) in &actions {
            bar.add(
                id,
                "new-content",
                (*title).into(),
                site.admin_url(link),
                Meta::default(),
            );
        }
    }
    // wp_admin_bar_add_secondary_groups().
    bar.add_group("top-secondary", "", "ab-top-secondary");
    bar.add_group("wp-logo-external", "wp-logo", "ab-sub-secondary");
    // wp_admin_bar_my_account_item().
    let howdy = format!("Howdy, <span class=\"display-name\">{display_name}</span>");
    let avatar = site.avatar(28);
    let meta = Meta {
        class: if avatar.is_empty() {
            String::new()
        } else {
            "with-avatar".into()
        },
        menu_title: String::from_utf8_lossy(&wp_strip_all_tags(howdy.as_bytes(), false))
            .into_owned(),
        ..Meta::default()
    };
    bar.add(
        "my-account",
        "top-secondary",
        format!("{howdy}{avatar}"),
        profile,
        meta,
    );
    let root = bar.bind();
    let mobile = if browser.is_mobile() { " mobile" } else { "" };
    let mut out = format!(
        "<div id=\"wpadminbar\" class=\"nojq nojs{mobile}\">\n\t\t\t\t\t\t<div class=\"quicklinks\" id=\"wp-toolbar\" role=\"navigation\" aria-label=\"Toolbar\">\n\t\t\t\t",
    );
    for group in bar
        .nodes
        .get(root)
        .into_iter()
        .flat_map(|root| bar.children(root))
    {
        bar.render_group(group, "", &mut out);
    }
    out.push_str("\t\t\t</div>\n\t\t</div>\n\n\t\t\n");
    out
}

#[cfg(test)]
mod tests;
