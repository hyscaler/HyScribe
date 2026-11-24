// SPDX-License-Identifier: AGPL-3.0-or-later

//! The admin menu: `wp-admin/menu.php`, `wp-admin/includes/menu.php` and
//! `_wp_menu_output()`, for a single site with a block theme and the
//! extensions' admin pages (SDK 0.4). Links drop `.php`, as hs-admin's
//! screens do (ADR 0008).

use hyscribe_kses::{esc_attr, esc_url};
use hyscribe_phpser::{Value, add_query_arg, md5};
use hyscribe_render::formatting::{sanitize_title, wptexturize};
use hyscribe_render::number_format;

/// What the menu asks of the user and the site.
pub trait MenuUser {
    /// `current_user_can( $cap )`.
    fn can(&self, cap: &str) -> bool;
    /// `wp_count_comments()->moderated`.
    fn moderated_comments(&self) -> u64;
    /// `get_transient( $name )`, as stored.
    fn transient(&self, name: &str) -> Option<Vec<u8>>;
    /// The active extensions' admin pages, in the order added.
    fn plugin_pages(&self) -> Vec<PluginPage>;
}

/// An extension's admin page (SDK 0.4), as `add_menu_page()` or
/// `add_submenu_page()` added it.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct PluginPage {
    /// The parent's file or slug; none for a top level page.
    pub parent: Option<String>,
    /// `$page_title`, markup.
    pub page_title: String,
    /// `$menu_title`, markup.
    pub menu_title: String,
    /// `$capability`.
    pub capability: String,
    /// `$menu_slug`.
    pub slug: String,
    /// A top level page's icon: a `dashicons-*` class, or empty.
    pub icon: String,
    /// `$position`.
    pub position: Option<i32>,
}

/// The Site Health count: the critical issues of the last cached check,
/// printed as PHP prints them but escaped, which PHP leaves out.
fn site_health_count(user: &dyn MenuUser) -> String {
    if !user.can("view_site_health_checks") {
        return String::new();
    }
    let result = user
        .transient("health-check-site-status-result")
        .unwrap_or_default();
    let critical = match hyscribe_phpser::json_decode(&result) {
        Some(issues) if issues.as_array().is_some_and(|issues| !issues.is_empty()) => {
            issues.get_str("critical").cloned().unwrap_or(Value::Null)
        }
        _ => Value::Int(0),
    };
    let class = esc_attr(&critical.to_php_string());
    // `number_format()` rounds half away from zero.
    let number = number_format(critical.to_float().round() as i128);
    format!(
        "<span class=\"menu-counter site-health-counter count-{class}\"><span class=\"count\">{number}</span></span>"
    )
}

/// The admin screen the menu is shown on, as the screen's file sets it up.
pub struct Screen<'a> {
    /// `$pagenow`, the screen's file: `edit.php`.
    pub pagenow: &'a str,
    /// `$typenow`, the post type it lists or edits.
    pub typenow: &'a str,
    /// The taxonomy it lists, if any.
    pub taxonomy: &'a str,
    /// `$parent_file`, if the screen sets one. PHP checks access before the
    /// screen's file runs, so the check starts without it.
    pub parent_file: Option<&'a str>,
    /// `$submenu_file`, if the screen sets one.
    pub submenu_file: Option<&'a str>,
    /// `$plugin_page`: the `page` an extension's admin page is asked by.
    pub plugin_page: Option<&'a str>,
}

/// A `$menu` entry: title, capability, slug, classes, ID and icon.
struct Item {
    title: String,
    cap: String,
    slug: String,
    classes: String,
    id: String,
    icon: String,
}

/// A `$submenu` entry: title, capability, slug and classes.
struct Sub {
    title: String,
    cap: String,
    slug: String,
    class: &'static str,
}

fn item(title: &str, cap: &str, slug: &str, classes: &str, id: &str, icon: &str) -> Item {
    Item {
        title: title.into(),
        cap: cap.into(),
        slug: slug.into(),
        classes: classes.into(),
        id: id.into(),
        icon: icon.into(),
    }
}

fn separator(slug: &str) -> Item {
    item("", "read", slug, "wp-menu-separator", "", "")
}

fn sub(title: &str, cap: &str, slug: &str) -> Sub {
    Sub {
        title: title.into(),
        cap: cap.into(),
        slug: slug.into(),
        class: "",
    }
}

/// `$submenu`: parents in the order they were first added.
#[derive(Default)]
struct Submenu(Vec<(String, Vec<Sub>)>);

impl Submenu {
    fn add(&mut self, parent: &str, entry: Sub) {
        self.put(parent, None, entry);
    }

    /// Adds `entry` under `parent`, at `at` among its entries or last.
    fn put(&mut self, parent: &str, at: Option<usize>, entry: Sub) {
        match self.0.iter_mut().find(|(p, _)| p == parent) {
            Some((_, entries)) => entries.insert(at.unwrap_or(entries.len()), entry),
            None => self.0.push((parent.into(), vec![entry])),
        }
    }

    fn get(&self, parent: &str) -> &[Sub] {
        self.0
            .iter()
            .find(|(p, _)| p == parent)
            .map_or(&[], |(_, entries)| entries)
    }

    fn take(&mut self, parent: &str) -> Vec<Sub> {
        self.0
            .iter()
            .position(|(p, _)| p == parent)
            .map(|at| self.0.remove(at).1)
            .unwrap_or_default()
    }
}

/// What the menu is built into, with what the user was denied, the hook
/// names of the menus (`$admin_page_hooks`), the parents that moved
/// (`$_wp_real_parent_file`) and the extensions' pages: those registered
/// (`$_registered_pages`) and those with a callback for the user.
#[derive(Default)]
struct Menu {
    items: Vec<(f64, Item)>,
    submenu: Submenu,
    menu_nopriv: Vec<String>,
    submenu_nopriv: Vec<(String, String)>,
    page_hooks: Vec<(String, String)>,
    real_parents: Vec<(String, String)>,
    registered: Vec<String>,
    actions: Vec<String>,
    /// The screen's parent, which a page asked under `admin.php` takes.
    parent: String,
    /// `$_parent_pages`: each page added and its parent, none at the top.
    parent_pages: Vec<(String, Option<String>)>,
}

impl Menu {
    fn submenu_denied(&self, parent: &str, page: &str) -> bool {
        self.submenu_nopriv
            .iter()
            .any(|(p, s)| p == parent && s == page)
    }

    fn page_hook(&self, slug: &str) -> Option<&str> {
        let found = self.page_hooks.iter().find(|(menu, _)| menu == slug);
        found.map(|(_, hook)| hook.as_str())
    }

    /// `$_wp_real_parent_file[ $parent ] ?? $parent`.
    fn real_parent<'a>(&'a self, parent: &'a str) -> &'a str {
        let found = self.real_parents.iter().find(|(old, _)| old == parent);
        found.map_or(parent, |(_, new)| new.as_str())
    }

    /// `get_plugin_page_hookname( $plugin_page, $parent_page )`: under no
    /// parent or `admin.php`, the screen's parent's type.
    fn hookname(&self, plugin_page: &str, parent_page: &str) -> String {
        let parent = match parent_page {
            "" | "admin.php" => &self.parent,
            parent => self.real_parent(parent),
        };
        let page_type = match self.page_hook(plugin_page) {
            Some(_) => "toplevel",
            None => self.page_hook(parent).unwrap_or("admin"),
        };
        format!("{page_type}_page_{}", plugin_page.replace(".php", ""))
    }

    /// `get_plugin_page_hook()`: the hook name, when a page's callback is on it.
    fn plugin_hook(&self, plugin_page: &str, parent_page: &str) -> Option<String> {
        let hook = self.hookname(plugin_page, parent_page);
        self.actions.contains(&hook).then_some(hook)
    }
}

/// `$admin_page_hooks` of the core menus: a menu's file, or the post type
/// it lists, as `sanitize_title()` makes it, under its older names.
fn core_page_hooks(items: &[(f64, Item)]) -> Vec<(String, String)> {
    let compat = [
        ("index", "dashboard"),
        ("edit", "posts"),
        ("post", "posts"),
        ("upload", "media"),
        ("link-manager", "links"),
        ("edit-pages", "pages"),
        ("page", "pages"),
        ("edit-comments", "comments"),
        ("options-general", "settings"),
        ("themes", "appearance"),
    ];
    let mut hooks = Vec::new();
    for (_, item) in items {
        let (file, query) = item.slug.split_once('?').unwrap_or((&item.slug, ""));
        let name = query.strip_prefix("post_type=").unwrap_or(file);
        let name = name.strip_suffix(".php").unwrap_or(name);
        let name = String::from_utf8_lossy(&sanitize_title(name.as_bytes())).into_owned();
        let found = compat.iter().find(|(old, _)| *old == name);
        hooks.push((
            item.slug.clone(),
            found.map_or(name, |(_, new)| new.to_string()),
        ));
    }
    hooks
}

/// `add_menu_page()` and `add_submenu_page()` of the extensions' pages, run
/// where `admin_menu` fires.
fn add_plugin_pages(menu: &mut Menu, user: &dyn MenuUser) {
    for page in user.plugin_pages() {
        match page.parent.as_deref() {
            None => add_menu_page(menu, user, page),
            Some(parent) => {
                let parent = menu.real_parent(parent).to_string();
                add_submenu_page(menu, user, &parent, page);
            }
        }
    }
}

fn add_menu_page(menu: &mut Menu, user: &dyn MenuUser, page: PluginPage) {
    let title = sanitize_title(page.menu_title.as_bytes());
    let title = String::from_utf8_lossy(&title).into_owned();
    menu.page_hooks.push((page.slug.clone(), title));
    let hookname = menu.hookname(&page.slug, "");
    if user.can(&page.capability) {
        menu.actions.push(hookname.clone());
    }
    let (icon, icon_class) = match page.icon.as_str() {
        "" => ("dashicons-admin-generic", "menu-icon-generic "),
        icon => (icon, ""),
    };
    let classes = format!("menu-top {icon_class}{hookname}");
    let new = item(
        &page.menu_title,
        &page.capability,
        &page.slug,
        &classes,
        &hookname,
        icon,
    );
    let integers = menu
        .items
        .iter()
        .map(|(key, _)| *key)
        .filter(|key| key.fract() == 0.0);
    let next = integers.fold(-1.0, f64::max) + 1.0;
    let key = match page.position.map(f64::from) {
        None => next,
        Some(position) if menu.items.iter().any(|(key, _)| *key == position) => {
            // `base_convert( substr( md5( $menu_slug . $menu_title ), -4 ), 16, 10 ) * 0.00001`.
            let hash = md5(format!("{}{}", page.slug, page.menu_title).as_bytes());
            let tail = hash.get(hash.len().saturating_sub(4)..).unwrap_or_default();
            let tail = u16::from_str_radix(tail, 16).unwrap_or_default();
            position + f64::from(tail) * 0.00001
        }
        Some(position) => position,
    };
    menu.items.push((key, new));
    menu.registered.push(hookname);
    menu.parent_pages.push((page.slug, None));
}

fn add_submenu_page(menu: &mut Menu, user: &dyn MenuUser, parent: &str, page: PluginPage) {
    if !user.can(&page.capability) {
        menu.submenu_nopriv.push((parent.into(), page.slug));
        return;
    }
    if menu.submenu.get(parent).is_empty() && page.slug != parent {
        let found = menu.items.iter().find(|(_, item)| item.slug == parent);
        if let Some((_, top)) = found.filter(|(_, top)| user.can(&top.cap)) {
            let first = sub(&top.title, &top.cap, &top.slug);
            menu.submenu.add(parent, first);
        }
    }
    let new = sub(&page.menu_title, &page.capability, &page.slug);
    let count = menu.submenu.get(parent).len();
    let position = page
        .position
        .map(|position| position.max(0).unsigned_abs() as usize);
    menu.submenu
        .put(parent, position.filter(|at| *at < count), new);
    let hookname = menu.hookname(&page.slug, parent);
    menu.actions.push(hookname.clone());
    menu.registered.push(hookname);
    menu.parent_pages.push((page.slug, Some(parent.into())));
}

/// The active theme as the Appearance menu shows it.
#[derive(Clone, Copy)]
pub enum Look<'a> {
    /// The bundled block theme.
    Block,
    /// A missing theme, classic without supports: its Customize link, and
    /// whether its parent is the bundled theme, whose `theme.json` it has.
    Classic(&'a str, bool),
}

/// `wp-admin/menu.php` for the active theme's `look`.
fn build(user: &dyn MenuUser, look: Look) -> Menu {
    let mut items = Vec::new();
    let mut submenu = Submenu::default();
    items.push((
        2,
        item(
            "Dashboard",
            "read",
            "index.php",
            "menu-top menu-top-first menu-icon-dashboard",
            "menu-dashboard",
            "dashicons-dashboard",
        ),
    ));
    submenu.add("index.php", sub("Home", "read", "index.php"));
    // Nobody here may update anything, so nothing is counted.
    let updates = ["update_core", "update_plugins", "update_themes"]
        .into_iter()
        .find(|cap| user.can(cap))
        .unwrap_or("update_languages");
    let count =
        "<span class=\"update-plugins count-0\"><span class=\"update-count\">0</span></span>";
    submenu.add(
        "index.php",
        sub(&format!("Updates {count}"), updates, "update-core.php"),
    );
    items.push((4, separator("separator1")));
    items.push((
        10,
        item(
            "Media",
            "upload_files",
            "upload.php",
            "menu-top menu-icon-media",
            "menu-media",
            "dashicons-admin-media",
        ),
    ));
    submenu.add("upload.php", sub("Library", "upload_files", "upload.php"));
    submenu.add(
        "upload.php",
        sub("Add Media File", "upload_files", "media-new.php"),
    );
    items.push((
        15,
        item(
            "Links",
            "manage_links",
            "link-manager.php",
            "menu-top menu-icon-links",
            "menu-links",
            "dashicons-admin-links",
        ),
    ));
    submenu.add(
        "link-manager.php",
        sub("All Links", "manage_links", "link-manager.php"),
    );
    submenu.add(
        "link-manager.php",
        sub("Add Link", "manage_links", "link-add.php"),
    );
    submenu.add(
        "link-manager.php",
        sub(
            "Link Categories",
            "manage_categories",
            "edit-tags.php?taxonomy=link_category",
        ),
    );
    if user.can("edit_posts") {
        let moderated = user.moderated_comments();
        let number = number_format(moderated);
        let text = if moderated == 1 {
            "Comment"
        } else {
            "Comments"
        };
        let title = format!(
            "Comments <span class=\"awaiting-mod count-{moderated}\"><span class=\"pending-count\" aria-hidden=\"true\">{number}</span><span class=\"comments-in-moderation-text screen-reader-text\">{number} {text} in moderation</span></span>"
        );
        items.push((
            25,
            item(
                &title,
                "edit_posts",
                "edit-comments.php",
                "menu-top menu-icon-comments",
                "menu-comments",
                "dashicons-admin-comments",
            ),
        ));
    }
    submenu.add(
        "edit-comments.php",
        sub("All Comments", "edit_posts", "edit-comments.php"),
    );
    // The built in post types, at their own positions.
    items.push((
        5,
        item(
            "Posts",
            "edit_posts",
            "edit.php",
            "menu-top menu-icon-post open-if-no-js",
            "menu-posts",
            "dashicons-admin-post",
        ),
    ));
    submenu.add("edit.php", sub("All Posts", "edit_posts", "edit.php"));
    submenu.add("edit.php", sub("Add Post", "edit_posts", "post-new.php"));
    submenu.add(
        "edit.php",
        sub(
            "Categories",
            "manage_categories",
            "edit-tags.php?taxonomy=category",
        ),
    );
    submenu.add(
        "edit.php",
        sub(
            "Tags",
            "manage_post_tags",
            "edit-tags.php?taxonomy=post_tag",
        ),
    );
    items.push((
        20,
        item(
            "Pages",
            "edit_pages",
            "edit.php?post_type=page",
            "menu-top menu-icon-page",
            "menu-pages",
            "dashicons-admin-page",
        ),
    ));
    submenu.add(
        "edit.php?post_type=page",
        sub("All Pages", "edit_pages", "edit.php?post_type=page"),
    );
    submenu.add(
        "edit.php?post_type=page",
        sub("Add Page", "edit_pages", "post-new.php?post_type=page"),
    );
    items.push((59, separator("separator2")));
    let appearance = if user.can("switch_themes") {
        "switch_themes"
    } else {
        "edit_theme_options"
    };
    items.push((
        60,
        item(
            "Appearance",
            appearance,
            "themes.php",
            "menu-top menu-icon-appearance",
            "menu-appearance",
            "dashicons-admin-appearance",
        ),
    ));
    submenu.add("themes.php", sub("Themes ", appearance, "themes.php"));
    let editor = match look {
        Look::Block => sub("Editor", "edit_theme_options", "site-editor.php"),
        Look::Classic(_, true) => sub("Design", "edit_theme_options", "site-editor.php"),
        Look::Classic(_, false) => sub(
            "Patterns",
            "edit_theme_options",
            "site-editor.php?p=/pattern",
        ),
    };
    submenu.add("themes.php", editor);
    submenu.add(
        "themes.php",
        sub("Fonts", "edit_theme_options", "font-library.php"),
    );
    if let Look::Classic(url, _) = look {
        let entry = Sub {
            class: "hide-if-no-customize",
            ..sub("Customize", "customize", url)
        };
        submenu.add("themes.php", entry);
    }
    items.push((
        65,
        item(
            "Plugins ",
            "activate_plugins",
            "plugins.php",
            "menu-top menu-icon-plugins",
            "menu-plugins",
            "dashicons-admin-plugins",
        ),
    ));
    submenu.add(
        "plugins.php",
        sub("Installed Plugins", "activate_plugins", "plugins.php"),
    );
    submenu.add(
        "plugins.php",
        sub("Add Plugin", "install_plugins", "plugin-install.php"),
    );
    if user.can("list_users") {
        items.push((
            70,
            item(
                "Users",
                "list_users",
                "users.php",
                "menu-top menu-icon-users",
                "menu-users",
                "dashicons-admin-users",
            ),
        ));
        submenu.add("users.php", sub("All Users", "list_users", "users.php"));
        if user.can("create_users") {
            submenu.add("users.php", sub("Add User", "create_users", "user-new.php"));
        }
        submenu.add("users.php", sub("Profile", "read", "profile.php"));
    } else {
        items.push((
            70,
            item(
                "Profile",
                "read",
                "profile.php",
                "menu-top menu-icon-users",
                "menu-users",
                "dashicons-admin-users",
            ),
        ));
        submenu.add("profile.php", sub("Profile", "read", "profile.php"));
        if user.can("create_users") {
            submenu.add(
                "profile.php",
                sub("Add User", "create_users", "user-new.php"),
            );
        }
    }
    items.push((
        75,
        item(
            "Tools",
            "edit_posts",
            "tools.php",
            "menu-top menu-icon-tools",
            "menu-tools",
            "dashicons-admin-tools",
        ),
    ));
    submenu.add(
        "tools.php",
        sub("Available Tools", "edit_posts", "tools.php"),
    );
    submenu.add("tools.php", sub("Import", "import", "import.php"));
    submenu.add("tools.php", sub("Export", "export", "export.php"));
    let health = format!("Site Health {}", site_health_count(user));
    submenu.add(
        "tools.php",
        sub(&health, "view_site_health_checks", "site-health.php"),
    );
    submenu.add(
        "tools.php",
        sub(
            "Export Personal Data",
            "export_others_personal_data",
            "export-personal-data.php",
        ),
    );
    submenu.add(
        "tools.php",
        sub(
            "Erase Personal Data",
            "erase_others_personal_data",
            "erase-personal-data.php",
        ),
    );
    items.push((
        80,
        item(
            "Settings",
            "manage_options",
            "options-general.php",
            "menu-top menu-icon-settings",
            "menu-settings",
            "dashicons-admin-settings",
        ),
    ));
    for (title, slug) in [
        ("General", "options-general.php"),
        ("Connectors", "options-connectors.php"),
        ("Writing", "options-writing.php"),
        ("Reading", "options-reading.php"),
        ("Discussion", "options-discussion.php"),
        ("Media", "options-media.php"),
        ("Permalinks", "options-permalink.php"),
    ] {
        submenu.add("options-general.php", sub(title, "manage_options", slug));
    }
    submenu.add(
        "options-general.php",
        sub("Privacy", "manage_privacy_options", "options-privacy.php"),
    );
    items.push((99, separator("separator-last")));
    let items: Vec<(f64, Item)> = items
        .into_iter()
        .map(|(key, item)| (key as f64, item))
        .collect();
    Menu {
        page_hooks: core_page_hooks(&items),
        items,
        submenu,
        menu_nopriv: Vec::new(),
        submenu_nopriv: Vec::new(),
        real_parents: Vec::new(),
        registered: Vec::new(),
        actions: Vec::new(),
        parent: String::new(),
        parent_pages: Vec::new(),
    }
}

/// `wp-admin/includes/menu.php`: what the user may not reach is taken out.
fn prune(menu: &mut Menu, user: &dyn MenuUser) {
    for (parent, entries) in &mut menu.submenu.0 {
        entries.retain(|entry| {
            let allowed = user.can(&entry.cap);
            if !allowed {
                menu.submenu_nopriv
                    .push((parent.clone(), entry.slug.clone()));
            }
            allowed
        });
    }
    menu.submenu.0.retain(|(_, entries)| !entries.is_empty());
    // A menu whose first submenu item is another file takes its place. PHP
    // also copies what was denied under it, which matters to no core page.
    let mut moves = Vec::new();
    for (_, item) in &mut menu.items {
        let first = menu.submenu.get(&item.slug).first().map(|s| s.slug.clone());
        if let Some(first) = first.filter(|first| *first != item.slug) {
            moves.push((std::mem::replace(&mut item.slug, first.clone()), first));
        }
    }
    for (old, first) in moves {
        for entry in menu.submenu.take(&old) {
            menu.submenu.add(&first, entry);
        }
        menu.real_parents.push((old, first));
    }
    add_plugin_pages(menu, user);
    // The `admin_menu` hooks add the file editors, which nobody here may use.
    for slug in ["theme-editor.php", "plugin-editor.php"] {
        menu.submenu_nopriv.push(("tools.php".into(), slug.into()));
    }
    let mut kept = Vec::new();
    for (key, entry) in std::mem::take(&mut menu.items) {
        if !user.can(&entry.cap) {
            menu.menu_nopriv.push(entry.slug.clone());
        }
        let subs = menu.submenu.get(&entry.slug);
        if subs.len() == 1 && subs.first().is_some_and(|s| s.slug == entry.slug) {
            menu.submenu.take(&entry.slug);
        }
        if menu.submenu.get(&entry.slug).is_empty() && menu.menu_nopriv.contains(&entry.slug) {
            continue;
        }
        kept.push((key, entry));
    }
    kept.sort_by(|(a, _), (b, _)| a.total_cmp(b));
    // No separator after another, and none last.
    let mut tidy: Vec<(f64, Item)> = Vec::new();
    for (key, entry) in kept {
        let separator = entry.classes.contains("wp-menu-separator");
        let after_separator = tidy
            .last()
            .is_some_and(|(_, last)| last.classes.contains("wp-menu-separator"));
        if !(separator && after_separator) {
            tidy.push((key, entry));
        }
    }
    tidy.pop_if(|(_, last)| last.classes == "wp-menu-separator");
    menu.items = tidy;
}

/// `get_admin_page_parent()`: the parent file of the screen, which it also
/// sets as `$parent_file`.
fn admin_page_parent(menu: &Menu, screen: &Screen, parent_file: &mut String) -> String {
    if let Some(plugin_page) = screen.plugin_page {
        let top = menu.items.iter().any(|(_, item)| item.slug == plugin_page);
        if screen.pagenow == "admin.php"
            && (top || menu.menu_nopriv.iter().any(|m| m == plugin_page))
        {
            *parent_file = plugin_page.to_string();
            return menu.real_parent(plugin_page).to_string();
        }
        if menu.submenu_denied(screen.pagenow, plugin_page) {
            *parent_file = screen.pagenow.to_string();
            return menu.real_parent(screen.pagenow).to_string();
        }
    }
    for (parent, entries) in &menu.submenu.0 {
        for entry in entries {
            let typed = !screen.typenow.is_empty()
                && format!("{}?post_type={}", screen.pagenow, screen.typenow) == entry.slug;
            let plain = screen.typenow.is_empty()
                && screen.pagenow == entry.slug
                && (parent_file.is_empty() || !parent_file.contains('?'));
            let plugin = screen.plugin_page == Some(entry.slug.as_str());
            if typed || plain || plugin {
                *parent_file = parent.to_string();
                return parent.to_string();
            }
        }
    }
    String::new()
}

/// `user_can_access_admin_page()`.
fn can_access(menu: &Menu, user: &dyn MenuUser, screen: &Screen, parent_file: &mut String) -> bool {
    let parent = admin_page_parent(menu, screen, parent_file);
    let page = screen.pagenow;
    let asked = screen.plugin_page.unwrap_or(page);
    if menu.submenu_denied(&parent, asked) {
        return false;
    }
    let registered = |plugin_page| {
        menu.registered
            .contains(&menu.hookname(plugin_page, &parent))
    };
    if screen
        .plugin_page
        .is_some_and(|plugin_page| !registered(plugin_page))
    {
        return false;
    }
    if parent.is_empty() {
        let denied = |name: &str| {
            menu.menu_nopriv.iter().any(|m| m == name)
                || menu.submenu_nopriv.iter().any(|(_, s)| s == name)
        };
        return !(denied(page) || screen.plugin_page.is_some_and(denied));
    }
    if screen.plugin_page == Some(parent.as_str()) && menu.menu_nopriv.contains(&parent) {
        return false;
    }
    let reached = |e: &&Sub| screen.plugin_page == Some(e.slug.as_str()) || e.slug == page;
    if let Some(entry) = menu.submenu.get(&parent).iter().find(reached) {
        return user.can(&entry.cap);
    }
    menu.items
        .iter()
        .find(|(_, item)| item.slug == parent)
        .is_none_or(|(_, item)| user.can(&item.cap))
}

/// `add_menu_classes()`: the first and last of each group.
fn menu_classes(items: &mut [(f64, Item)]) {
    let count = items.len();
    let mut added = Vec::new();
    let (mut first_item, mut last) = (false, None);
    for (i, (_, item)) in items.iter().enumerate() {
        if let (true, Some(last)) = (item.slug.starts_with("separator"), last) {
            first_item = true;
            added.push((last, "menu-top-last"));
            continue;
        }
        if first_item {
            first_item = false;
            added.push((i, "menu-top-first"));
        }
        if i + 1 == count {
            added.push((i, "menu-top-last"));
        }
        last = Some(i);
    }
    for (i, (_, item)) in items.iter_mut().enumerate() {
        for (_, class) in added.iter().filter(|(at, _)| *at == i) {
            add_class(&mut item.classes, class);
        }
    }
}

/// `add_cssclass()` on an item, which always has classes.
fn add_class(classes: &mut String, class: &str) {
    classes.push(' ');
    classes.push_str(class);
}

/// hs-admin's name for a `wp-admin` file, with its query: without `.php`
/// (ADR 0008).
pub fn link(file: &str) -> String {
    match file.split_once('?') {
        Some((path, query)) => format!("{}?{query}", path.strip_suffix(".php").unwrap_or(path)),
        None => file.strip_suffix(".php").unwrap_or(file).to_string(),
    }
}

/// What comes before an hs-admin screen's name in a link: the folder plain,
/// JSON-escaped or URL-encoded, or the attribute of a relative link.
const BEFORE_SCREEN: [&str; 7] = [
    "/hs-admin/",
    "\\/hs-admin\\/",
    "%2Fhs-admin%2F",
    "href=\"",
    "href='",
    "action=\"",
    "action='",
];

/// `html` with its links to hs-admin's screens as [`link`] makes them,
/// literal markup copied from `wp-admin` included: `edit.php?x` is `edit?x`.
pub fn clean_links(html: &str) -> String {
    let mut out = String::with_capacity(html.len());
    let mut rest = html;
    while let Some(at) = rest.find(['/', '\\', '%', 'h', 'a']) {
        let (head, tail) = rest.split_at(at);
        out.push_str(head);
        let Some(marker) = BEFORE_SCREEN.iter().find(|m| tail.starts_with(*m)) else {
            // The character found is ASCII.
            let (found, tail) = tail.split_at(1);
            out.push_str(found);
            rest = tail;
            continue;
        };
        let (marker, tail) = tail.split_at(marker.len());
        out.push_str(marker);
        let end = tail
            .find(|c: char| !(c.is_ascii_alphanumeric() || matches!(c, '-' | '_')))
            .unwrap_or(tail.len());
        let (name, tail) = tail.split_at(end);
        out.push_str(name);
        let after = tail.strip_prefix(".php").filter(|after| {
            !name.is_empty()
                && !after.starts_with(|c: char| c.is_ascii_alphanumeric() || "-_./".contains(c))
        });
        rest = after.unwrap_or(tail);
    }
    out.push_str(rest);
    out
}

/// `wp_enqueue_command_palette_assets()`'s menu commands: label, hs-admin
/// URL and name of each menu item the user may reach and its submenu; an
/// extension's page by `menu_page_url()`, `admin_url` making its URL.
pub fn commands(
    user: &dyn MenuUser,
    look: Look,
    admin_url: &dyn Fn(&str) -> String,
) -> Vec<(String, String, String)> {
    let mut menu = build(user, look);
    prune(&mut menu, user);
    // Every slug the menu holds without `.php` is an extension's page.
    let url = |slug: &str| {
        if slug.contains(".php") {
            return link(slug);
        }
        let found = menu.parent_pages.iter().find(|(page, _)| page == slug);
        let top = |parent: &str| menu.parent_pages.iter().any(|(page, _)| page == parent);
        let parent = found.and_then(|(_, parent)| parent.as_deref().filter(|parent| !top(parent)));
        let path = match parent {
            Some(parent) => {
                let page = [(&b"page"[..], Some(slug.as_bytes()))];
                String::from_utf8_lossy(&add_query_arg(&page, parent.as_bytes())).into_owned()
            }
            None => format!("admin.php?page={slug}"),
        };
        admin_url(&link(&path))
    };
    let mut out = Vec::new();
    for (_, item) in &menu.items {
        if item.title.is_empty() || !user.can(&item.cap) {
            continue;
        }
        let label = root_text(&item.title);
        out.push((label.clone(), url(&item.slug), item.slug.clone()));
        // Pruning left only titled entries the user may reach.
        for entry in menu.submenu.get(&item.slug) {
            out.push((
                format!("{label} > {}", root_text(&entry.title)),
                url(&entry.slug),
                format!("{}-{}", item.slug, entry.slug),
            ));
        }
    }
    out
}

/// The text of a menu title outside its tags, trimmed; core's titles hold
/// no void tags or character references.
fn root_text(title: &str) -> String {
    let mut text = String::new();
    let mut depth = 0usize;
    let mut rest = title;
    while let Some((before, tag)) = rest.split_once('<') {
        if depth == 0 {
            text.push_str(before);
        }
        let (name, after) = tag.split_once('>').unwrap_or((tag, ""));
        if name.starts_with('/') {
            depth = depth.saturating_sub(1);
        } else {
            depth += 1;
        }
        rest = after;
    }
    if depth == 0 {
        text.push_str(rest);
    }
    text.trim().to_string()
}

/// `esc_url()` of a relative `wp-admin` URL, `file.php?query`, then its
/// hs-admin link: `esc_url()` keeps a relative URL only in the PHP form.
pub fn esc_link(php_url: &str) -> String {
    link(&String::from_utf8_lossy(&hyscribe_kses::esc_url(
        php_url.as_bytes(),
    )))
}

/// The admin menu, `#adminmenumain`, on `screen` for `user` and the active
/// theme's `look`; `None` when `user_can_access_admin_page()` turns them away.
pub fn admin_menu(user: &dyn MenuUser, screen: &Screen, look: Look) -> Option<String> {
    let mut menu = build(user, look);
    prune(&mut menu, user);
    // `user_can_access_admin_page()` runs from `admin.php`, before the
    // screen sets its parent file; one it leaves unset keeps what was found.
    let mut found = String::new();
    menu.parent = admin_page_parent(&menu, screen, &mut found);
    found.clear();
    if !can_access(&menu, user, screen, &mut found) {
        return None;
    }
    let mut parent_file = screen.parent_file.map_or(found, String::from);
    menu_classes(&mut menu.items);
    // `menu-header.php` asks for the parent once more.
    admin_page_parent(&menu, screen, &mut parent_file);
    let mut out = String::from(
        "<div id=\"adminmenumain\" role=\"navigation\" aria-label=\"Main menu\">\n<a href=\"#wpbody-content\" class=\"screen-reader-shortcut\">Skip to main content</a>\n<a href=\"#wp-toolbar\" class=\"screen-reader-shortcut\">Skip to toolbar</a>\n<div id=\"adminmenuback\"></div>\n<div id=\"adminmenuwrap\">\n<ul id=\"adminmenu\">\n\n",
    );
    output(&menu, user, screen, &parent_file, &mut out);
    out.push_str("</ul>\n</div>\n</div>\n");
    Some(out)
}

/// `admin.php`'s `$page_hook` for the extension's page `plugin_page` asked
/// under `pagenow`: `get_plugin_page_hook()` under it, then under the page
/// itself; none when no callback is on either for `user`.
pub fn plugin_page_hook(user: &dyn MenuUser, pagenow: &str, plugin_page: &str) -> Option<String> {
    let mut menu = build(user, Look::Block);
    prune(&mut menu, user);
    let screen = Screen {
        pagenow,
        typenow: "",
        taxonomy: "",
        parent_file: None,
        submenu_file: None,
        plugin_page: Some(plugin_page),
    };
    menu.parent = admin_page_parent(&menu, &screen, &mut String::new());
    let hook = menu.plugin_hook(plugin_page, pagenow);
    hook.or_else(|| menu.plugin_hook(plugin_page, plugin_page))
}

/// `file_exists( $menu_file )` in `wp-admin`: a core screen's file.
fn core_file(file: &str) -> bool {
    file.ends_with(".php")
}

/// `_wp_menu_output()`.
fn output(menu: &Menu, user: &dyn MenuUser, screen: &Screen, parent_file: &str, out: &mut String) {
    let self_file = screen.pagenow;
    let mut first = true;
    for (_, item) in &menu.items {
        let mut class = Vec::new();
        let mut aria = String::new();
        if first {
            class.push("wp-first-item".to_string());
            first = false;
        }
        let subs = menu.submenu.get(&item.slug);
        if !subs.is_empty() {
            class.push("wp-has-submenu".into());
        }
        let current = (!parent_file.is_empty() && item.slug == parent_file)
            || (screen.typenow.is_empty() && self_file == item.slug);
        if current {
            if subs.is_empty() {
                class.push("current".into());
                aria.push_str("aria-current=\"page\"");
            } else {
                class.push("wp-has-current-submenu wp-menu-open".into());
            }
        } else {
            class.push("wp-not-current-submenu".into());
            if !subs.is_empty() {
                aria.push_str("data-ariahaspopup");
            }
        }
        class.push(esc_attr(item.classes.as_bytes()));
        let class = format!(" class=\"{}\"", class.join(" "));
        let id = if item.id.is_empty() {
            String::new()
        } else {
            format!(" id=\"{}\"", item.id)
        };
        let separator = class.contains("wp-menu-separator");
        let hidden = if separator {
            " aria-hidden=\"true\""
        } else {
            ""
        };
        let image = if item.icon.is_empty() {
            String::new()
        } else {
            "<br />".into()
        };
        let image_class = if item.icon.is_empty() {
            " dashicons-before".to_string()
        } else {
            format!(" dashicons-before {}", item.icon)
        };
        let title = String::from_utf8_lossy(&wptexturize(item.title.as_bytes(), &[])).into_owned();
        out.push_str(&format!("\n\t<li{class}{id}{hidden}>"));
        let anchor = |href: &str, name: &str| {
            format!(
                "\n\t<a href='{}'{class} {aria}><div class='wp-menu-image{image_class}' aria-hidden='true'>{image}</div><div class='wp-menu-name'>{name}</div></a>",
                link(href)
            )
        };
        if separator {
            out.push_str("<div class=\"separator\"></div>");
        } else if let Some(first_sub) = subs.first() {
            if menu.plugin_hook(&first_sub.slug, &item.slug).is_some() {
                let href = format!("admin.php?page={}", first_sub.slug);
                out.push_str(anchor(&href, &title).trim_start_matches("\n\t"));
            } else {
                out.push_str(&anchor(&first_sub.slug, &title));
            }
        } else if menu.plugin_hook(&item.slug, "admin.php").is_some() {
            out.push_str(&anchor(
                &format!("admin.php?page={}", item.slug),
                &item.title,
            ));
        } else {
            // Pruning left only items without a submenu that the user may reach.
            out.push_str(&anchor(&item.slug, &item.title));
        }
        if !subs.is_empty() {
            out.push_str(&format!("\n\t<ul class='wp-submenu wp-submenu-wrap'><li class='wp-submenu-head' aria-hidden='true'>{}</li>", item.title));
            let mut first_sub = true;
            for entry in subs.iter().filter(|e| user.can(&e.cap)) {
                let mut class = Vec::new();
                let mut aria = "";
                if first_sub {
                    class.push("wp-first-item");
                    first_sub = false;
                }
                let (menu_file, _) = item.slug.split_once('?').unwrap_or((&item.slug, ""));
                let current = match (screen.submenu_file, screen.plugin_page) {
                    (Some(file), _) => file == entry.slug,
                    (None, None) => self_file == entry.slug,
                    // `$self_type` is never a parent here: no typed screen has a page.
                    (None, Some(plugin_page)) => {
                        plugin_page == entry.slug
                            && (item.slug == self_file || !core_file(menu_file))
                    }
                };
                if current {
                    class.push("current");
                    aria = " aria-current=\"page\"";
                }
                if !entry.class.is_empty() {
                    class.push(entry.class);
                }
                let class = if class.is_empty() {
                    String::new()
                } else {
                    format!(" class=\"{}\"", class.join(" "))
                };
                let title =
                    String::from_utf8_lossy(&wptexturize(entry.title.as_bytes(), &[])).into_owned();
                let href = match menu.plugin_hook(&entry.slug, &item.slug) {
                    Some(_) => {
                        let file = if core_file(menu_file) {
                            item.slug.as_str()
                        } else {
                            "admin.php"
                        };
                        let page = [(&b"page"[..], Some(entry.slug.as_bytes()))];
                        let url = add_query_arg(&page, file.as_bytes());
                        String::from_utf8_lossy(&esc_url(&url)).into_owned()
                    }
                    None => entry.slug.clone(),
                };
                out.push_str(&format!(
                    "<li{class}><a href='{}'{class}{aria}>{title}</a></li>",
                    link(&href)
                ));
            }
            out.push_str("</ul>");
        }
        out.push_str("</li>");
    }
    out.push_str("<li id=\"collapse-menu\" class=\"hide-if-no-js\"><button type=\"button\" id=\"collapse-button\" aria-label=\"Collapse Main Menu\" aria-expanded=\"true\"><span class=\"collapse-button-icon\" aria-hidden=\"true\"></span><span class=\"collapse-button-label\">Collapse Menu</span></button></li>");
}

#[cfg(test)]
mod tests;
