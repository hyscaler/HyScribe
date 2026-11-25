// SPDX-License-Identifier: AGPL-3.0-or-later

//! The revisions screen, `revision.php`: two revisions of a post or page
//! compared, with `wp_prepare_revisions_for_js()`'s data for its script.

use hyscribe_kses::{esc_html, esc_url};
use hyscribe_model::{Post, PostQuery, StatusFilter};
use hyscribe_phpser::{Key, Value, absint, is_empty_string, json_encode_localized, parse_str};
use hyscribe_render::date::{self, Zone, human_time_diff, stored_unix};
use hyscribe_render::formatting::sanitize_text_field;
use hyscribe_render::post::admin_title;
use hyscribe_render::text_diff::wp_text_diff;
use hyscribe_render::user_avatar;

use crate::admin_bar::BarSite;
use crate::menu::Screen;
use crate::page::{HelpTab, Page, Request, admin_page};
use crate::posts::{Answer, PostsSite, draft_or_post_title, edit_link, lock_holder};

const TEMPLATES: &str = include_str!("revisions.html");

const HELP: [HelpTab; 1] = [HelpTab {
    id: "revisions-overview",
    title: "Overview",
    content: "<p>This screen is used for managing your content revisions.</p><p>Revisions are saved copies of your post or page, which are periodically created as you update your content. The red text on the left shows the content that was removed. The green text on the right shows the content that was added.</p><p>From this screen you can review, compare, and restore revisions:</p><ul><li>To navigate between revisions, <strong>drag the slider handle left or right</strong> or <strong>use the Previous or Next buttons</strong>.</li><li>Compare two different revisions by <strong>selecting the &#8220;Compare any two revisions&#8221; box</strong> to the side.</li><li>To restore a revision, <strong>click Restore This Revision</strong>.</li></ul>",
}];

const SIDEBAR: &str = "<p><strong>For more information:</strong></p><p><a href=\"https://wordpress.org/documentation/article/revisions/\">Revisions Management</a></p><p><a href=\"https://wordpress.org/support/forums/\">Support forums</a></p>";

/// `revision.php`'s arguments, as it reads `$_REQUEST`.
pub struct RevisionArgs {
    /// `$revision_id`: `revision`, else `to`.
    pub revision: u64,
    /// `$action`, sanitized.
    pub action: String,
    /// `$from`, when numeric.
    pub from: Option<u64>,
}

impl RevisionArgs {
    /// The arguments of `request`, `None` for one that is not text.
    pub fn read(request: &dyn Fn(&str) -> Option<Value>) -> Option<RevisionArgs> {
        let text = |name| match request(name) {
            Some(Value::String(text)) => Some(Some(text)),
            None => Some(None),
            Some(_) => None,
        };
        let filled = |name| Some(text(name)?.filter(|text| !is_empty_string(text)));
        let numeric = |name| {
            let value = filled(name)?.map(Value::String);
            Some(value.filter(Value::is_numeric).map(|value| absint(&value)))
        };
        let (to, from) = (numeric("to")?, numeric("from")?);
        let revision = filled("revision")?.map_or(0, |id| absint(&Value::String(id)));
        let action = filled("action")?.map_or_else(Vec::new, |a| sanitize_text_field(&a));
        Some(RevisionArgs {
            revision: if revision == 0 {
                to.unwrap_or_default()
            } else {
                revision
            },
            action: String::from_utf8_lossy(&action).into_owned(),
            from,
        })
    }
}

/// Whether `revision` is an autosave: `wp_is_post_autosave()`.
fn is_autosave(revision: &Post) -> bool {
    let marker = format!("{}-autosave", revision.parent);
    revision
        .name
        .windows(marker.len())
        .any(|w| w == marker.as_bytes())
}

/// `revision.php`, but a restore past its checks: the screen, or where it
/// sends the user; `None` for what is not ported.
pub fn revision(site: &dyn PostsSite, query: &str, request: &Request) -> Option<Answer> {
    let parsed = parse_str(query.as_bytes());
    let args = RevisionArgs::read(&|name| parsed.get_str(name).cloned())?;
    let back = || Some(Answer::Redirect("edit".into()));
    // A restore that reaches the screen failed one of its checks.
    if args.action == "restore" {
        return back();
    }
    let Some(revision) = site
        .post(args.revision)
        .filter(|r| r.post_type == "revision")
    else {
        return back();
    };
    let Some(post) = site.post(revision.parent) else {
        return back();
    };
    if !site.can_post("read_post", &revision) || !site.can_post("edit_post", &post) {
        return back();
    }
    // Other types' revisions may be turned off, which is not ported.
    if !["post", "page"].contains(&post.post_type.as_str()) {
        return None;
    }
    let settings = prepare(site, &post, &revision, args.from)?;
    let link = String::from_utf8_lossy(&esc_url(edit_link(site, &post).as_bytes())).into_owned();
    let disabled = if lock_holder(site, &post).is_some() {
        "disabled=\"disabled\""
    } else {
        "<# if ( data.attributes.current ) { #>\n\t\t\t\t\t\t\tdisabled=\"disabled\"\n\t\t\t\t\t\t<# } #>"
    };
    let content = format!(
        "\n<div class=\"wrap\">\n\t<h1 class=\"long-header\">Compare Revisions of &#8220;<a href=\"{link}\">{}</a>&#8221;</h1>\n\t<a href=\"{link}\">&larr; Go to editor</a></div>\n{}",
        draft_or_post_title(&post),
        TEMPLATES.replace("%disabled%", disabled)
    );
    let parent = if post.post_type == "post" {
        "edit.php"
    } else {
        "edit.php?post_type=page"
    };
    let page = Page {
        screen: Screen {
            pagenow: "revision.php",
            typenow: "",
            taxonomy: "",
            parent_file: Some(parent),
            submenu_file: Some(parent),
            plugin_page: None,
        },
        id: "revision",
        title: "Revisions",
        help: &HELP,
        sidebar: SIDEBAR,
        screen_options: "",
        styles: &[],
        scripts: &["revisions"],
        late_scripts: &[],
        script_data: &[(
            "revisions",
            format!("var _wpRevisionsSettings = {settings};"),
        )],
        script_after: &[],
        late_styles: &[],
        admin_head: "",
        admin_footer: "",
        footer_scripts: "",
        content: &content,
        canonical: request.canonical,
        origin: request.origin,
        view: None,
    };
    Some(Answer::from_page(admin_page(site, &page, request.browser)))
}

/// A PHP array of string keys.
fn object(entries: Vec<(&str, Value)>) -> Value {
    Value::Array(
        entries
            .into_iter()
            .map(|(key, value)| (Key::String(key.as_bytes().to_vec()), value))
            .collect(),
    )
}

/// A PHP list.
fn list(items: Vec<Value>) -> Value {
    Value::Array(
        items
            .into_iter()
            .enumerate()
            .map(|(at, item)| (Key::Int(at as i64), item))
            .collect(),
    )
}

fn string(text: impl Into<Vec<u8>>) -> Value {
    Value::String(text.into())
}

/// `wp_prepare_revisions_for_js( $post, $selected, $from )`, as
/// `wp_localize_script()` encodes it; `None` where PHP goes wrong.
fn prepare(
    site: &dyn PostsSite,
    post: &Post,
    revision: &Post,
    from: Option<u64>,
) -> Option<String> {
    let selected = revision.id;
    let (zone, now) = site.clock();
    let query = PostQuery {
        post_types: vec!["revision".into()],
        statuses: vec![StatusFilter {
            status: "inherit".into(),
            author: None,
            post_type: None,
        }],
        parents: vec![post.id],
        order: vec![
            (hyscribe_model::OrderBy::Date, true),
            (hyscribe_model::OrderBy::Id, true),
        ],
        limit: u32::MAX,
        ..PostQuery::default()
    };
    let (revisions, _) = site.list_posts(&query);
    let title = string(admin_title(post));
    let mut authors: Vec<(u64, Value)> = Vec::new();
    let mut author = Value::Null;
    let mut entries: Vec<(u64, Vec<(&str, Value)>)> = Vec::new();
    let mut current_id: Option<u64> = None;
    for revision in &revisions {
        author = match authors.iter().find(|(id, _)| *id == revision.author) {
            Some((_, known)) => known.clone(),
            None => {
                let user = site.userdata(revision.author)?;
                let avatar = user_avatar(&|name| site.option(name), &user, 32);
                let known = object(vec![
                    ("id", Value::Int(revision.author.cast_signed())),
                    ("avatar", string(avatar)),
                    ("name", string(user.display_name)),
                ]);
                authors.push((revision.author, known.clone()));
                known
            }
        };
        let autosave = is_autosave(revision);
        let mut current = !autosave && revision.modified_gmt == post.modified_gmt;
        // Of revisions saved at once, the highest ID is current.
        if current && let Some(id) = current_id {
            if id < revision.id {
                set_current(&mut entries, id, false);
                current_id = Some(revision.id);
            } else {
                current = false;
            }
        } else if current {
            current_id = Some(revision.id);
        }
        // The screen is the post's editors', who may restore.
        let url = BarSite::admin_url(site, "revision.php");
        let nonce = site.nonce(&format!("restore-post_{}", revision.id));
        let restore = string(format!(
            "{url}?revision={}&action=restore&_wpnonce={nonce}",
            revision.id
        ));
        let modified = stored_unix(&revision.modified_gmt, Zone::Utc);
        entries.push((
            revision.id,
            vec![
                ("id", Value::Int(revision.id.cast_signed())),
                ("title", title.clone()),
                ("author", author.clone()),
                (
                    "date",
                    string(date::format("M j, Y @ H:i", &revision.modified, zone)),
                ),
                (
                    "dateShort",
                    string(date::format("j M Y @ H:i", &revision.modified, zone)),
                ),
                (
                    "timeAgo",
                    string(format!("{} ago", human_time_diff(modified, now))),
                ),
                ("autosave", Value::Bool(autosave)),
                ("current", Value::Bool(current)),
                ("restoreUrl", restore),
            ],
        ));
    }
    // With one revision, the post itself is the first, by its author.
    if entries.len() == 1 {
        let modified = stored_unix(&post.modified_gmt, Zone::Utc);
        entries.push((
            post.id,
            vec![
                ("id", Value::Int(post.id.cast_signed())),
                ("title", title),
                ("author", author),
                (
                    "date",
                    string(date::format("M j, Y @ H:i", &post.modified, zone)),
                ),
                (
                    "dateShort",
                    string(date::format("j M @ H:i", &post.modified, zone)),
                ),
                (
                    "timeAgo",
                    string(format!("{} ago", human_time_diff(modified, now))),
                ),
                ("autosave", Value::Bool(false)),
                ("current", Value::Bool(true)),
                ("restoreUrl", Value::Bool(false)),
            ],
        ));
        current_id = Some(post.id);
    }
    // Saved since the last revision: the newest that is not an autosave.
    if current_id.is_none() {
        let id = revisions
            .iter()
            .rev()
            .find(|revision| !is_autosave(revision))?
            .id;
        set_current(&mut entries, id, true);
    }
    let ids: Vec<u64> = entries.iter().map(|(id, _)| *id).collect();
    let compare = from.is_some();
    let from = match from {
        Some(from) => from,
        None => match ids.iter().position(|&id| id == selected) {
            Some(at) if at > 0 => ids.get(at - 1).copied().unwrap_or_default(),
            _ => 0,
        },
    };
    let fields = ui_diff(site, post, from, revision.clone())?;
    let data = entries
        .into_iter()
        .map(|(_, entry)| object(entry))
        .collect();
    let ids = ids
        .into_iter()
        .map(|id| Value::Int(id.cast_signed()))
        .collect();
    let base = BarSite::admin_url(site, "revision.php");
    // `parse_url( $url, PHP_URL_PATH )` of an absolute URL.
    let path = base
        .split_once("://")
        .and_then(|(_, rest)| rest.split_once('/'));
    let base = path.map_or_else(String::new, |(_, path)| format!("/{path}"));
    let settings = object(vec![
        ("postId", string(post.id.to_string())),
        ("nonce", string(site.nonce("revisions-ajax-nonce"))),
        ("revisionData", list(data)),
        ("to", string(selected.to_string())),
        ("from", string(from.to_string())),
        (
            "diffData",
            list(vec![object(vec![
                ("id", string(format!("{from}:{selected}"))),
                ("fields", fields),
            ])]),
        ),
        ("baseUrl", string(base)),
        ("compareTwoMode", string(if compare { "1" } else { "0" })),
        ("revisionIds", list(ids)),
    ]);
    json_encode_localized(&settings).and_then(|json| String::from_utf8(json).ok())
}

fn set_current(entries: &mut [(u64, Vec<(&str, Value)>)], id: u64, current: bool) {
    let entries = entries.iter_mut().filter(|(at, _)| *at == id);
    for (key, value) in entries.flat_map(|(_, entry)| entry.iter_mut()) {
        if *key == "current" {
            *value = Value::Bool(current);
        }
    }
}

/// `wp_get_revision_ui_diff( $post, $from, $to )`: each field that
/// differs, with its diff, or `false`.
fn ui_diff(site: &dyn PostsSite, post: &Post, from: u64, mut compare_to: Post) -> Option<Value> {
    let none = Some(Value::Bool(false));
    let mut compare_from = match from {
        0 => None,
        id => match site.post(id) {
            Some(found) => Some(found),
            None => return none,
        },
    };
    let ours = |p: &Post| p.parent == post.id || p.id == post.id;
    if compare_from.as_ref().is_some_and(|from| !ours(from)) || !ours(&compare_to) {
        return none;
    }
    if let Some(from) = &mut compare_from {
        // Dates as MySQL stores them compare as `strtotime()` would.
        if from.date_gmt > compare_to.date_gmt {
            std::mem::swap(from, &mut compare_to);
        }
        if is_empty_string(&from.title) {
            from.title = b"(no title)".to_vec();
        }
    }
    if is_empty_string(&compare_to.title) {
        compare_to.title = b"(no title)".to_vec();
    }
    let mut fields = Vec::new();
    for (id, name) in [
        ("post_title", "Title"),
        ("post_content", "Content"),
        ("post_excerpt", "Excerpt"),
    ] {
        let field = |p: &Post| match id {
            "post_title" => p.title.clone(),
            "post_content" => p.content.clone(),
            _ => p.excerpt.clone(),
        };
        let left = compare_from.as_ref().map(field).unwrap_or_default();
        let mut diff = wp_text_diff(&left, &field(&compare_to))?;
        // The title shows unchanged too.
        if diff.is_empty() && id == "post_title" {
            let from_title = compare_from.as_ref().map(|p| p.title.clone());
            diff = format!(
                "<table class=\"diff\"><colgroup><col class=\"content diffsplit left\"><col class=\"content diffsplit middle\"><col class=\"content diffsplit right\"></colgroup><tbody><tr><td>{}</td><td></td><td>{}</td></tr></tbody></table>",
                esc_html(&from_title.unwrap_or_default()),
                esc_html(&compare_to.title)
            );
        }
        if !diff.is_empty() {
            fields.push(object(vec![
                ("id", string(id)),
                ("name", string(name)),
                ("diff", string(diff)),
            ]));
        }
    }
    Some(list(fields))
}
