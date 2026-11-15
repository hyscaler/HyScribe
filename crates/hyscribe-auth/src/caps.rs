// SPDX-License-Identifier: AGPL-3.0-or-later

//! Roles and capabilities: `WP_User::has_cap()` over `map_meta_cap()`, for
//! a single site. HyScribe never changes code or files from the browser, so
//! the capabilities to do so map to `do_not_allow` (docs/ADMIN.md).

use std::collections::HashMap;
use std::sync::OnceLock;

use hyscribe_phpser::{Key, Maybe, Value, isset, json_decode, maybe_unserialize};

/// What `map_meta_cap()` reads of the post types, taxonomies and statuses.
const REGISTRY: &[u8] = include_bytes!("../../../wordpress/wp-includes/capabilities.json");

/// The capabilities that would change code or files on the server.
const FILE_MODS: [&str; 15] = [
    "edit_files",
    "edit_plugins",
    "edit_themes",
    "update_plugins",
    "delete_plugins",
    "install_plugins",
    "upload_plugins",
    "update_themes",
    "delete_themes",
    "install_themes",
    "upload_themes",
    "update_core",
    "update_php",
    "install_languages",
    "update_languages",
];

/// A post as `map_meta_cap()` reads it.
#[derive(Clone)]
pub struct CapPost {
    /// `ID`.
    pub id: u64,
    /// `post_author`.
    pub author: u64,
    /// `post_status`.
    pub status: String,
    /// `post_type`.
    pub post_type: String,
    /// `post_parent`.
    pub parent: u64,
}

/// The site data capability checks read.
pub trait CapabilityData {
    /// `get_option( $name )`, as stored.
    fn option(&self, name: &str) -> Option<Vec<u8>>;
    /// `get_user_meta( $user_id, $key, true )`, as stored.
    fn user_meta(&self, user_id: u64, key: &str) -> Option<Vec<u8>>;
    /// Whether the user exists.
    fn user_exists(&self, user_id: u64) -> bool;
    /// `get_post( $id )`.
    fn post(&self, id: u64) -> Option<CapPost>;
    /// `get_post_meta( $id, $key, true )`, as stored.
    fn post_meta(&self, id: u64, key: &str) -> Option<Vec<u8>>;
    /// The post of the comment `id`, 0 for none; `None` without the comment.
    fn comment_post(&self, id: u64) -> Option<u64>;
    /// The taxonomy of the term `id`, `None` without the term.
    fn term_taxonomy(&self, id: u64) -> Option<String>;
    /// Whether `wp-config.php` defines the constant as true.
    fn constant(&self, name: &str) -> bool;
}

/// The capability names of a post type or taxonomy.
struct Caps {
    map_meta_cap: bool,
    /// The singular base of its capability names.
    capability_type: String,
    cap: HashMap<String, String>,
}

struct Registry {
    post_types: HashMap<String, Caps>,
    post_type_meta_caps: HashMap<String, String>,
    taxonomies: HashMap<String, Caps>,
    statuses: HashMap<String, (bool, bool)>,
}

fn registry() -> &'static Registry {
    static REGISTRY_DATA: OnceLock<Registry> = OnceLock::new();
    REGISTRY_DATA.get_or_init(|| {
        let data = json_decode(REGISTRY).unwrap_or(Value::Null);
        let entries = |name: &str| {
            data.get_str(name)
                .and_then(Value::as_array)
                .unwrap_or_default()
        };
        let text = |value: &Value| String::from_utf8_lossy(&value.to_php_string()).into_owned();
        let key_text = |key: &Key| String::from_utf8_lossy(&key.to_php_string()).into_owned();
        let caps = |name: &str| {
            entries(name)
                .iter()
                .map(|(key, value)| {
                    let cap = value
                        .get_str("cap")
                        .and_then(Value::as_array)
                        .unwrap_or_default();
                    let caps = Caps {
                        map_meta_cap: value.get_str("map_meta_cap") == Some(&Value::Bool(true)),
                        capability_type: value
                            .get_str("capability_type")
                            .map(text)
                            .unwrap_or_default(),
                        cap: cap.iter().map(|(k, v)| (key_text(k), text(v))).collect(),
                    };
                    (key_text(key), caps)
                })
                .collect()
        };
        Registry {
            post_types: caps("post_types"),
            post_type_meta_caps: entries("post_type_meta_caps")
                .iter()
                .map(|(key, value)| (key_text(key), text(value)))
                .collect(),
            taxonomies: caps("taxonomies"),
            statuses: entries("post_statuses")
                .iter()
                .map(|(key, value)| {
                    let flag = |name| value.get_str(name) == Some(&Value::Bool(true));
                    (key_text(key), (flag("public"), flag("private")))
                })
                .collect(),
        }
    })
}

/// `post_type_exists( $name )`.
pub fn post_type_exists(name: &str) -> bool {
    registry().post_types.contains_key(name)
}

/// The taxonomy's capability `name` (`manage_terms`, `edit_terms` and so
/// on), `None` for a taxonomy not registered.
pub fn taxonomy_cap(taxonomy: &str, name: &str) -> Option<String> {
    let tax = registry().taxonomies.get(taxonomy)?;
    Some(tax.cap.get(name).cloned().unwrap_or_default())
}

/// A stored option or meta value, unserialized.
fn stored(value: Option<Vec<u8>>) -> Value {
    let value = value.unwrap_or_default();
    match maybe_unserialize(&value) {
        Maybe::Value(value) => value,
        // PHP keeps the stored string when it does not unserialize.
        _ => Value::String(value),
    }
}

/// `(int) get_option( $name )`.
fn option_int(data: &dyn CapabilityData, name: &str) -> i64 {
    stored(data.option(name)).to_int()
}

/// The roles that grant `cap`, as `WP_User_Query` lists them for its
/// `capability` argument.
pub fn roles_with(data: &dyn CapabilityData, prefix: &str, cap: &str) -> Vec<String> {
    let Value::Array(roles) = stored(data.option(&format!("{prefix}user_roles"))) else {
        return Vec::new();
    };
    roles
        .iter()
        .filter(|(_, role)| {
            role.get_str("capabilities")
                .and_then(Value::as_array)
                .unwrap_or_default()
                .iter()
                .any(|(key, granted)| key.to_php_string() == cap.as_bytes() && !granted.is_empty())
        })
        .map(|(key, _)| String::from_utf8_lossy(&key.to_php_string()).into_owned())
        .collect()
}

/// A user's capabilities, as `WP_User` loads them for the site.
pub struct User {
    id: u64,
    allcaps: Vec<(Key, Value)>,
}

impl User {
    /// `new WP_User( $id )`: its `{prefix}capabilities`, roles first, then
    /// its own capabilities over them. No user has none.
    pub fn load(data: &dyn CapabilityData, prefix: &str, id: u64) -> User {
        if id == 0 || !data.user_exists(id) {
            return User {
                id: 0,
                allcaps: Vec::new(),
            };
        }
        let roles = stored(data.option(&format!("{prefix}user_roles")));
        let caps = match stored(data.user_meta(id, &format!("{prefix}capabilities"))) {
            Value::Array(caps) => caps,
            _ => Vec::new(),
        };
        let mut allcaps = Vec::new();
        for (key, _) in &caps {
            // `is_role()`: a role WordPress names.
            let Some(role) = roles.get(key).filter(|role| isset(role.get_str("name"))) else {
                continue;
            };
            let granted = role
                .get_str("capabilities")
                .and_then(Value::as_array)
                .unwrap_or_default();
            merge(&mut allcaps, granted);
        }
        merge(&mut allcaps, &caps);
        User { id, allcaps }
    }

    /// `$user->allcaps`.
    pub fn allcaps(&self) -> &[(Key, Value)] {
        &self.allcaps
    }

    /// `has_cap( $cap, ...$args )`: every capability `map_meta_cap()` asks
    /// for is granted.
    pub fn has_cap(&self, data: &dyn CapabilityData, cap: &str, args: &[Value]) -> bool {
        // Multisite's super admin rules are not ported: nothing is granted.
        if data.constant("MULTISITE") {
            return false;
        }
        // A user level, deprecated since 2.0, as `translate_level_to_cap()`.
        let level = format!("level_{cap}");
        let numeric = Value::String(cap.as_bytes().to_vec()).is_numeric();
        let cap = if numeric { level.as_str() } else { cap };
        let caps = map_meta_cap(data, self, cap, args);
        let mut allcaps = self.allcaps.clone();
        let granted = |allcaps: &[(Key, Value)], name: &str| {
            allcaps
                .iter()
                .rev()
                .find(|(key, _)| key.to_php_string() == name.as_bytes())
                .is_some_and(|(_, value)| !value.is_empty())
        };
        // The `user_has_cap` filters core adds.
        let grant = |allcaps: &mut Vec<(Key, Value)>, name: &str, when: bool| {
            if when {
                merge(
                    allcaps,
                    &[(Key::String(name.as_bytes().to_vec()), Value::Bool(true))],
                );
            }
        };
        let languages = ["update_core", "install_plugins", "install_themes"]
            .iter()
            .any(|name| granted(&allcaps, name));
        grant(&mut allcaps, "install_languages", languages);
        let plugins = granted(&allcaps, "activate_plugins");
        grant(&mut allcaps, "resume_plugins", plugins);
        let themes = granted(&allcaps, "switch_themes");
        grant(&mut allcaps, "resume_themes", themes);
        let health = granted(&allcaps, "install_plugins");
        grant(&mut allcaps, "view_site_health_checks", health);
        grant(&mut allcaps, "exist", true);
        caps.iter()
            .all(|cap| cap != "do_not_allow" && granted(&allcaps, cap))
    }
}

/// `array_merge()` of capability arrays: string keys replaced in place.
fn merge(into: &mut Vec<(Key, Value)>, from: &[(Key, Value)]) {
    for (key, value) in from {
        match into.iter_mut().find(|(k, _)| k == key) {
            Some((_, old)) => *old = value.clone(),
            None => into.push((key.clone(), value.clone())),
        }
    }
}

/// `get_post_status()`: an attachment's inherited status, from its parent.
/// A loop of parents, which PHP never leaves, has no status.
pub fn post_status(data: &dyn CapabilityData, post: &CapPost) -> String {
    inherited_status(data, post, &mut Vec::new())
}

fn inherited_status(data: &dyn CapabilityData, post: &CapPost, seen: &mut Vec<u64>) -> String {
    if seen.contains(&post.id) {
        return String::new();
    }
    seen.push(post.id);
    if post.post_type != "attachment" {
        return post.status.clone();
    }
    if post.status != "inherit" {
        let kept = matches!(post.status.as_str(), "private" | "trash" | "auto-draft");
        return if kept {
            post.status.clone()
        } else {
            "publish".into()
        };
    }
    let parent = (post.parent != post.id)
        .then(|| data.post(post.parent))
        .flatten();
    let Some(parent) = parent else {
        return "publish".into();
    };
    let status = inherited_status(data, &parent, seen);
    if status != "trash" {
        return status;
    }
    let trashed = stored(data.post_meta(parent.id, "_wp_trash_meta_status"));
    if trashed.is_empty() {
        "publish".into()
    } else {
        String::from_utf8_lossy(&trashed.to_php_string()).into_owned()
    }
}

/// `is_protected_meta()`: the key's first byte, past the bytes PCRE's
/// `[^\x20-\x7E\p{L}]` strips, is an underscore.
fn is_protected_meta(key: &[u8]) -> bool {
    let kept = |byte: &&u8| matches!(**byte, 0x20..=0x7e | 0xaa | 0xb5 | 0xba | 0xc0..=0xd6 | 0xd8..=0xf6 | 0xf8..=0xff);
    key.iter().find(kept) == Some(&b'_')
}

/// `(int) $args[$n]`, if set.
fn arg(args: &[Value], n: usize) -> Option<i64> {
    args.get(n)
        .filter(|value| **value != Value::Null)
        .map(Value::to_int)
}

/// The post `$args[0]` names: `get_post()` takes only a numeric ID.
fn arg_post(data: &dyn CapabilityData, args: &[Value]) -> Option<CapPost> {
    let id = args.first().filter(|id| id.is_numeric())?.to_int();
    u64::try_from(id).ok().and_then(|id| data.post(id))
}

/// `map_meta_cap()`: the primitive capabilities `cap` needs.
fn deny() -> Vec<String> {
    vec!["do_not_allow".to_string()]
}

fn map_meta_cap(data: &dyn CapabilityData, user: &User, cap: &str, args: &[Value]) -> Vec<String> {
    let one = |name: &str| vec![name.to_string()];
    let registry = registry();
    let user_id = i64::try_from(user.id).unwrap_or(0);
    if FILE_MODS.contains(&cap) {
        return deny();
    }
    match cap {
        "remove_user" => {
            let own = arg(args, 0) == Some(user_id);
            if own && !user.has_cap(data, "delete_users", &[]) {
                deny()
            } else {
                one("remove_users")
            }
        }
        "promote_user" | "add_users" => one("promote_users"),
        "edit_user" | "edit_users" => {
            if user_id < 1 {
                deny()
            } else if cap == "edit_user" && arg(args, 0) == Some(user_id) {
                Vec::new()
            } else {
                one("edit_users")
            }
        }
        "delete_post" | "delete_page" | "edit_post" | "edit_page" => {
            post_caps(data, user_id, cap, args)
        }
        "read_post" | "read_page" => {
            let Some(post) = arg_post(data, args) else {
                return deny();
            };
            let post = if post.post_type == "revision" {
                match data.post(post.parent) {
                    Some(parent) => parent,
                    None => return deny(),
                }
            } else {
                post
            };
            let Some(post_type) = registry.post_types.get(&post.post_type) else {
                return one("edit_others_posts");
            };
            let type_cap = |name: &str| post_type.cap.get(name).cloned().unwrap_or_default();
            if !post_type.map_meta_cap {
                return vec![type_cap(cap)];
            }
            let Some(&(public, private)) = registry.statuses.get(&post_status(data, &post)) else {
                return one("edit_others_posts");
            };
            if public || post.author != 0 && i64::try_from(post.author) == Ok(user_id) {
                vec![type_cap("read")]
            } else if private {
                vec![type_cap("read_private_posts")]
            } else {
                map_meta_cap(
                    data,
                    user,
                    "edit_post",
                    &[Value::Int(i64::try_from(post.id).unwrap_or(0))],
                )
            }
        }
        "publish_post" => {
            let Some(post) = arg_post(data, args) else {
                return deny();
            };
            match registry.post_types.get(&post.post_type) {
                Some(post_type) => vec![
                    post_type
                        .cap
                        .get("publish_posts")
                        .cloned()
                        .unwrap_or_default(),
                ],
                None => one("edit_others_posts"),
            }
        }
        "edit_post_meta"
        | "delete_post_meta"
        | "add_post_meta"
        | "edit_comment_meta"
        | "delete_comment_meta"
        | "add_comment_meta"
        | "edit_term_meta"
        | "delete_term_meta"
        | "add_term_meta"
        | "edit_user_meta"
        | "delete_user_meta"
        | "add_user_meta" => {
            let object_type = cap.split('_').nth(1).unwrap_or_default();
            let Some(id) = arg(args, 0) else {
                return deny();
            };
            let object = u64::try_from(id).unwrap_or(0);
            let exists = match object_type {
                "post" => data
                    .post(object)
                    .is_some_and(|post| !post.post_type.is_empty()),
                "comment" => data.comment_post(object).is_some(),
                "term" => data.term_taxonomy(object).is_some(),
                _ => data.user_exists(object),
            };
            if !exists {
                return deny();
            }
            let mut caps = map_meta_cap(
                data,
                user,
                &format!("edit_{object_type}"),
                &[Value::Int(id)],
            );
            let key = args
                .get(1)
                .filter(|key| !key.is_empty())
                .map(Value::to_php_string);
            // Core's one registered meta with an auth callback lets whoever
            // may edit the comment set a note's status.
            let allowed = match key {
                None => true,
                Some(key) if object_type == "comment" && key == b"_wp_note_status" => {
                    user.has_cap(data, "edit_comment", &[Value::Int(id)])
                }
                Some(key) => !is_protected_meta(&key),
            };
            if !allowed {
                caps.push(cap.to_string());
            }
            caps
        }
        // `get_comment()` takes only a numeric ID.
        "edit_comment" => {
            let id = args.first().filter(|id| id.is_numeric()).map(Value::to_int);
            match id.and_then(|id| data.comment_post(u64::try_from(id).ok()?)) {
                None => deny(),
                Some(post) => match data.post(post) {
                    Some(post) => map_meta_cap(
                        data,
                        user,
                        "edit_post",
                        &[Value::Int(i64::try_from(post.id).unwrap_or(0))],
                    ),
                    None => map_meta_cap(data, user, "edit_posts", &[]),
                },
            }
        }
        "unfiltered_upload" if data.constant("ALLOW_UNFILTERED_UPLOADS") => one(cap),
        "unfiltered_upload" => deny(),
        "edit_css" | "unfiltered_html" if data.constant("DISALLOW_UNFILTERED_HTML") => deny(),
        "edit_css" | "unfiltered_html" => one("unfiltered_html"),
        "activate_plugins" | "deactivate_plugins" | "activate_plugin" | "deactivate_plugin" => {
            one("activate_plugins")
        }
        "resume_plugin" => one("resume_plugins"),
        "resume_theme" => one("resume_themes"),
        "delete_user" | "delete_users" => one("delete_users"),
        "manage_links" => {
            if stored(data.option("link_manager_enabled")).is_empty() {
                deny()
            } else {
                one(cap)
            }
        }
        "customize" => one("edit_theme_options"),
        "delete_site" => deny(),
        "edit_term" | "delete_term" | "assign_term" => {
            let Some(term) = arg(args, 0) else {
                return deny();
            };
            let taxonomy = u64::try_from(term)
                .ok()
                .and_then(|id| data.term_taxonomy(id));
            let Some((taxonomy, tax)) =
                taxonomy.and_then(|t| Some((t.clone(), registry.taxonomies.get(&t)?)))
            else {
                return deny();
            };
            let default = [
                format!("default_{taxonomy}"),
                format!("default_term_{taxonomy}"),
            ]
            .iter()
            .any(|name| option_int(data, name) == term);
            if cap == "delete_term" && default {
                return deny();
            }
            let tax_cap = tax.cap.get(&format!("{cap}s")).cloned().unwrap_or_default();
            map_meta_cap(data, user, &tax_cap, &[Value::Int(term)])
        }
        "manage_post_tags" | "edit_categories" | "edit_post_tags" | "delete_categories"
        | "delete_post_tags" => one("manage_categories"),
        "assign_categories" | "assign_post_tags" => one("edit_posts"),
        "setup_network"
        | "export_others_personal_data"
        | "erase_others_personal_data"
        | "manage_privacy_options" => one("manage_options"),
        "update_https" => vec!["manage_options".into(), "update_core".into()],
        "create_app_password"
        | "list_app_passwords"
        | "read_app_password"
        | "edit_app_password"
        | "delete_app_passwords"
        | "delete_app_password" => {
            map_meta_cap(data, user, "edit_user", args.get(..1).unwrap_or_default())
        }
        "edit_block_binding" => {
            // The block editor's context: its post, else the site editor's name.
            let property = |value: Option<&Value>, name: &str| match value {
                Some(Value::Object { properties, .. }) => {
                    hyscribe_phpser::lookup(properties, name).cloned()
                }
                _ => None,
            };
            let context = args.first();
            let post = property(property(context, "post").as_ref(), "ID").map(|id| id.to_int());
            let Some(id) = post else {
                let site_editor = property(context, "name")
                    .is_some_and(|name| name == Value::String(b"core/edit-site".to_vec()));
                return if site_editor {
                    map_meta_cap(data, user, "edit_theme_options", &[])
                } else {
                    deny()
                };
            };
            // `get_post_type_capabilities()` of the type without its own map;
            // for a type mapping meta capabilities it registers `edit_post`.
            let edit = u64::try_from(id)
                .ok()
                .and_then(|id| data.post(id))
                .and_then(|post| registry.post_types.get(&post.post_type))
                .map(|post_type| match post_type.map_meta_cap {
                    true => "edit_post".to_string(),
                    false => format!("edit_{}", post_type.capability_type),
                });
            match edit {
                Some(edit) => map_meta_cap(data, user, &edit, &[Value::Int(id)]),
                None => deny(),
            }
        }
        _ => match registry.post_type_meta_caps.get(cap) {
            Some(meta) => map_meta_cap(data, user, meta, args),
            None => {
                let blocks = [
                    "edit_blocks",
                    "edit_others_blocks",
                    "publish_blocks",
                    "read_private_blocks",
                    "delete_blocks",
                    "delete_private_blocks",
                    "delete_published_blocks",
                    "delete_others_blocks",
                    "edit_private_blocks",
                    "edit_published_blocks",
                ]
                .contains(&cap);
                if blocks {
                    one(&cap.replace("_blocks", "_posts"))
                } else {
                    one(cap)
                }
            }
        },
    }
}

/// `delete_post`, `delete_page`, `edit_post` and `edit_page`.
fn post_caps(data: &dyn CapabilityData, user_id: i64, cap: &str, args: &[Value]) -> Vec<String> {
    let delete = cap.starts_with("delete_");
    let Some(post) = arg_post(data, args) else {
        return deny();
    };
    let post = match (post.post_type == "revision", delete) {
        (false, _) => post,
        (true, true) => return deny(),
        (true, false) => match data.post(post.parent) {
            Some(parent) => parent,
            None => return deny(),
        },
    };
    let id = i64::try_from(post.id).unwrap_or(0);
    if delete
        && (option_int(data, "page_for_posts") == id || option_int(data, "page_on_front") == id)
    {
        return vec!["manage_options".into()];
    }
    let Some(post_type) = registry().post_types.get(&post.post_type) else {
        return vec!["edit_others_posts".into()];
    };
    let type_cap = |name: &str| post_type.cap.get(name).cloned().unwrap_or_default();
    if !post_type.map_meta_cap {
        return vec![type_cap(cap)];
    }
    let verb = if delete { "delete" } else { "edit" };
    let published = |status: &str| matches!(status, "publish" | "future");
    let mut caps = Vec::new();
    if post.author != 0 && i64::try_from(post.author) == Ok(user_id) {
        let trashed = stored(data.post_meta(post.id, "_wp_trash_meta_status"));
        let was_published =
            post.status == "trash" && published(&String::from_utf8_lossy(&trashed.to_php_string()));
        if published(&post.status) || was_published {
            caps.push(type_cap(&format!("{verb}_published_posts")));
        } else {
            caps.push(type_cap(&format!("{verb}_posts")));
        }
    } else {
        caps.push(type_cap(&format!("{verb}_others_posts")));
        if published(&post.status) {
            caps.push(type_cap(&format!("{verb}_published_posts")));
        } else if post.status == "private" {
            caps.push(type_cap(&format!("{verb}_private_posts")));
        }
    }
    if option_int(data, "wp_page_for_privacy_policy") == id {
        caps.push("manage_options".into());
    }
    caps
}

#[cfg(test)]
mod tests;
