// SPDX-License-Identifier: AGPL-3.0-or-later

//! Switching themes as `switch_theme()` writes it from the admin, and what
//! `check_theme_switched()` does on the next load, for the embedded block
//! themes: they register no sidebars and no navigation menus, so the
//! widgets and menu locations of the theme left are carried over as
//! `retrieve_widgets()` and `_wp_menus_changed()` carry them to such a theme.
//! Each runs inside the caller's [`Write`], which the caller commits.

use hyscribe_db::Write;
use hyscribe_phpser::{Key, Value};

use crate::options::{Autoload, add_option, delete_option, get_option, stored, update_option};
use crate::{Error, now};

/// `$wp_theme_features`' counterpart for widgets: the base of each core
/// widget, in the order `wp_widgets_init()` registers them.
const WIDGET_BASES: [&str; 18] = [
    "pages",
    "calendar",
    "archives",
    "media_audio",
    "media_image",
    "media_gallery",
    "media_video",
    "meta",
    "search",
    "text",
    "categories",
    "recent-posts",
    "recent-comments",
    "rss",
    "tag_cloud",
    "nav_menu",
    "custom_html",
    "block",
];

/// The option holding a theme's modifications.
pub(crate) fn mods_option(stylesheet: &str) -> String {
    format!("theme_mods_{stylesheet}")
}

pub(crate) fn key(name: &str) -> Key {
    Key::String(name.as_bytes().to_vec())
}

pub(crate) fn text(value: &[u8]) -> String {
    String::from_utf8_lossy(value).into_owned()
}

/// `get_theme_mods()` of the active theme `stylesheet` (named `name`): the
/// theme's option, which moves from its deprecated place only in an old
/// site, not ported. `None` when the option is not an array.
pub(crate) async fn theme_mods_lookup(
    write: &mut Write<'_>,
    stylesheet: &str,
    name: &str,
) -> Result<Option<Vec<(Key, Value)>>, Error> {
    match get_option(write, &mods_option(stylesheet)).await? {
        Value::Array(mods) => Ok(Some(mods)),
        Value::Bool(false) => {
            let current = get_option(write, "current_theme").await?;
            let current = match current {
                Value::Bool(false) => name.as_bytes().to_vec(),
                current => current.to_php_string(),
            };
            let legacy = format!("mods_{}", text(&current));
            if write.option_value(&legacy).await?.is_some() {
                return Err(Error::Unported("theme mods kept under the theme's name"));
            }
            Ok(Some(Vec::new()))
        }
        _ => Ok(None),
    }
}

async fn theme_mods(
    write: &mut Write<'_>,
    stylesheet: &str,
    name: &str,
) -> Result<Vec<(Key, Value)>, Error> {
    Ok(theme_mods_lookup(write, stylesheet, name)
        .await?
        .unwrap_or_default())
}

/// The value of `name` in `entries`.
fn entry<'a>(entries: &'a [(Key, Value)], name: &str) -> Option<&'a Value> {
    entries
        .iter()
        .find(|(known, _)| *known == key(name))
        .map(|(_, value)| value)
}

/// `get_theme_mod( $name )`: `false` when it is not set.
async fn get_theme_mod(
    write: &mut Write<'_>,
    stylesheet: &str,
    theme_name: &str,
    name: &str,
) -> Result<Value, Error> {
    let mods = theme_mods(write, stylesheet, theme_name).await?;
    Ok(entry(&mods, name).cloned().unwrap_or(Value::Bool(false)))
}

/// `set_theme_mod( $name, $value )` of the active theme `stylesheet`.
async fn set_theme_mod(
    write: &mut Write<'_>,
    (stylesheet, theme_name): (&str, &str),
    name: &str,
    value: Value,
) -> Result<(), Error> {
    let mut mods = theme_mods(write, stylesheet, theme_name).await?;
    match mods.iter_mut().find(|(known, _)| *known == key(name)) {
        Some((_, old)) => *old = value,
        None => mods.push((key(name), value)),
    }
    update_option(write, &mods_option(stylesheet), &Value::Array(mods), None).await?;
    Ok(())
}

/// `wp_get_sidebars_widgets()` in the admin: the `sidebars_widgets` option
/// without its `array_version`.
async fn sidebars_widgets(write: &mut Write<'_>) -> Result<Vec<(Key, Value)>, Error> {
    match get_option(write, "sidebars_widgets").await? {
        Value::Array(mut sidebars) => {
            sidebars.retain(|(known, _)| *known != key("array_version"));
            Ok(sidebars)
        }
        Value::Bool(false) => Ok(Vec::new()),
        _ => Err(Error::Unported(
            "a sidebars_widgets option that is not an array",
        )),
    }
}

/// `wp_set_option_autoload_values()` of one option: its autoload turned `on`
/// or `off` when it is not already, a missing option left alone.
async fn set_autoload(write: &mut Write<'_>, name: &str, on: bool) -> Result<(), Error> {
    let wanted = if on { "on" } else { "off" };
    let current = write.option_autoload(name).await?.unwrap_or_default();
    if current != wanted {
        let value = write.option_value(name).await?.unwrap_or_default();
        write.update_option_autoload(name, &value, wanted).await?;
    }
    Ok(())
}

/// `WP_Theme::delete_pattern_cache()` of the themes `stylesheets`. The cache
/// is named after a hash of the theme's folder on the server PHP ran on,
/// which is not known here: a cache is the theme's whose patterns are named
/// after its stylesheet.
async fn delete_pattern_caches(write: &mut Write<'_>, stylesheets: &[&str]) -> Result<(), Error> {
    let prefix = "_site_transient_wp_theme_files_patterns-";
    for (option, value) in write.options_starting(prefix).await? {
        let patterns = stored(&value);
        let patterns = patterns.get_str("patterns").and_then(Value::as_array);
        let named = patterns.unwrap_or_default().iter().any(|(_, pattern)| {
            let slug = pattern.get_str("slug").map(Value::to_php_string);
            let slug = text(&slug.unwrap_or_default());
            stylesheets
                .iter()
                .any(|stylesheet| slug.starts_with(&format!("{stylesheet}/")))
        });
        if named {
            // `delete_site_transient()`: the option, and its timeout.
            let name = option.strip_prefix("_site_transient_").unwrap_or_default();
            delete_option(write, &option).await?;
            delete_option(write, &format!("_site_transient_timeout_{name}")).await?;
        }
    }
    Ok(())
}

/// `switch_theme( $stylesheet )` of the embedded block theme `stylesheet`,
/// named `name`, from the admin, when the active theme is the embedded
/// `old` (named `old_name`): what is written at once. `theme_switched` is
/// left for the next load, [`after_switch_theme`].
pub async fn switch_theme(
    write: &mut Write<'_>,
    stylesheet: (&str, &str),
    old: (&str, &str),
) -> Result<(), Error> {
    let ((stylesheet, name), (old, old_name)) = (stylesheet, old);
    // `$sidebars_widgets`, which the admin has set.
    let sidebars = sidebars_widgets(write).await?;
    let kept = Value::Array(vec![
        (key("time"), Value::Int(now())),
        (key("data"), Value::Array(sidebars)),
    ]);
    set_theme_mod(write, (old, old_name), "sidebars_widgets", kept).await?;
    let locations = get_theme_mod(write, old, old_name, "nav_menu_locations").await?;
    update_option(
        write,
        "theme_switch_menu_locations",
        &locations,
        Some(Autoload::On),
    )
    .await?;
    update_option(write, "template", &text_value(stylesheet), None).await?;
    update_option(write, "stylesheet", &text_value(stylesheet), None).await?;
    // A single themes folder: no roots are kept.
    delete_option(write, "template_root").await?;
    delete_option(write, "stylesheet_root").await?;
    update_option(write, "current_theme", &text_value(name), None).await?;
    // Migrates from the old `mods_{name}` option to `theme_mods_{slug}`.
    if get_option(write, &mods_option(stylesheet)).await? == Value::Bool(false) {
        if write.option_value(&format!("mods_{name}")).await?.is_some() {
            return Err(Error::Unported("theme mods kept under the theme's name"));
        }
        let mut defaults = vec![(Key::Int(0), Value::Bool(false))];
        if !locations.is_empty() {
            defaults.push((key("nav_menu_locations"), locations));
        }
        let defaults = Value::Array(defaults);
        add_option(write, &mods_option(stylesheet), &defaults, None).await?;
    }
    // Stores classic sidebars for later use by block themes: a block
    // theme registers none.
    let theme = (stylesheet, name);
    set_theme_mod(
        write,
        theme,
        "wp_classic_sidebars",
        Value::Array(Vec::new()),
    )
    .await?;
    update_option(write, "theme_switched", &text_value(old), None).await?;
    delete_pattern_caches(write, &[stylesheet, old]).await?;
    set_autoload(write, &mods_option(old), false).await?;
    set_autoload(write, &mods_option(stylesheet), stylesheet != old).await?;
    Ok(())
}

fn text_value(text: &str) -> Value {
    Value::String(text.as_bytes().to_vec())
}

/// The widget instances PHP has registered: each core widget's numbered
/// instances kept in its `widget_<base>` option, or its one template.
async fn registered_widgets(write: &mut Write<'_>) -> Result<Vec<Vec<u8>>, Error> {
    if !get_option(write, "link_manager_enabled").await?.is_empty() {
        return Err(Error::Unported("the Links widget"));
    }
    let mut ids = Vec::new();
    for base in WIDGET_BASES {
        let settings = get_option(write, &format!("widget_{base}")).await?;
        let numbers: Vec<i64> = settings
            .as_array()
            .unwrap_or_default()
            .iter()
            .filter_map(|(number, _)| match number {
                Key::Int(number) => Some(*number),
                Key::String(_) => None,
            })
            .collect();
        if numbers.is_empty() {
            ids.push(format!("{base}-1").into_bytes());
        }
        ids.extend(
            numbers
                .iter()
                .map(|number| format!("{base}-{number}").into_bytes()),
        );
    }
    Ok(ids)
}

/// A sidebar's widgets: their keys kept, as `array_intersect()` keeps them.
type Widgets = Vec<(Key, Value)>;

/// Sidebars and their widgets, in order.
type Sidebars = Vec<(Key, Widgets)>;

/// The sidebars of an option's array, each a list of widgets, else not
/// ported.
fn lists(entries: &[(Key, Value)]) -> Result<Sidebars, Error> {
    entries
        .iter()
        .map(|(sidebar, widgets)| match widgets {
            Value::Array(widgets) => Ok((sidebar.clone(), widgets.clone())),
            _ => Err(Error::Unported("a sidebar that is not a list of widgets")),
        })
        .collect()
}

/// `_wp_remove_unregistered_widgets()`.
fn remove_unregistered(sidebars: Sidebars, allowed: &[Vec<u8>]) -> Sidebars {
    let registered = |widgets: Widgets| -> Widgets {
        let kept = widgets
            .into_iter()
            .filter(|(_, widget)| allowed.contains(&widget.to_php_string()));
        kept.collect()
    };
    let kept = sidebars
        .into_iter()
        .map(|(sidebar, widgets)| (sidebar, registered(widgets)));
    kept.collect()
}

/// `array_merge()` of numeric lists: values in order, renumbered.
fn merged(lists: &[&Widgets]) -> Widgets {
    let values = lists
        .iter()
        .flat_map(|list| list.iter().map(|(_, value)| value));
    values
        .cloned()
        .zip(0..)
        .map(|(value, at)| (Key::Int(at), value))
        .collect()
}

/// What `wp_map_sidebars_widgets()` gives a theme that registers no sidebar:
/// its inactive widgets, and the sidebars it had when it was last active.
struct Mapped {
    inactive: Widgets,
    restored: Sidebars,
}

/// `wp_map_sidebars_widgets()` of a theme that registers no sidebar, with
/// `previous` the `sidebars_widgets` theme mod's data from when the theme
/// was last active.
fn map_sidebars_widgets(
    existing: &Sidebars,
    previous: Option<&Value>,
    allowed: &[Vec<u8>],
) -> Result<Mapped, Error> {
    let inactive_key = key("wp_inactive_widgets");
    let orphaned = |sidebar: &Key| matches!(sidebar, Key::String(name) if name.starts_with(b"orphaned_widgets"));
    let mut mapped = Mapped {
        inactive: Widgets::new(),
        restored: Sidebars::new(),
    };
    if existing.is_empty() {
        return Ok(mapped);
    }
    // The inactive and orphaned widgets first, then those left over.
    let (first, left): (Vec<_>, Vec<_>) = existing
        .iter()
        .partition(|(sidebar, _)| *sidebar == inactive_key || orphaned(sidebar));
    for (_, widgets) in first.into_iter().chain(left) {
        mapped.inactive = merged(&[&mapped.inactive, widgets]);
    }
    let Some(Value::Array(previous)) = previous else {
        return Ok(mapped);
    };
    // Sidebars from when the theme was last active, but the empty ones, the
    // inactive ones if some are inactive now, and the orphaned ones.
    let old: Vec<(Key, Value)> = previous
        .iter()
        .filter(|(_, widgets)| !widgets.is_empty())
        .filter(|(sidebar, _)| mapped.inactive.is_empty() || *sidebar != inactive_key)
        .filter(|(sidebar, _)| !orphaned(sidebar))
        .cloned()
        .collect();
    let old = remove_unregistered(lists(&old)?, allowed);
    // The widgets revived leave the inactive ones.
    for (_, widgets) in &old {
        let snapshot = mapped.inactive.clone();
        for (_, widget) in widgets {
            if let Some((found, _)) = snapshot.iter().find(|(_, listed)| listed == widget) {
                mapped.inactive.retain(|(at, _)| at != found);
            }
        }
    }
    // What the theme had comes back, over the inactive widgets if it had any.
    for (sidebar, widgets) in old {
        if sidebar == inactive_key {
            mapped.inactive = widgets;
        } else {
            mapped.restored.push((sidebar, widgets));
        }
    }
    Ok(mapped)
}

/// `wp_set_sidebars_widgets()` of sidebars that have no version yet: the
/// option, versioned.
async fn set_sidebars_widgets(
    write: &mut Write<'_>,
    mut sidebars: Vec<(Key, Value)>,
) -> Result<(), Error> {
    sidebars.push((key("array_version"), Value::Int(3)));
    update_option(write, "sidebars_widgets", &Value::Array(sidebars), None).await?;
    Ok(())
}

/// `retrieve_widgets( true )` for the active embedded block theme
/// `stylesheet` (named `name`), which registers no sidebar.
async fn retrieve_widgets(
    write: &mut Write<'_>,
    stylesheet: &str,
    name: &str,
) -> Result<(), Error> {
    let allowed = registered_widgets(write).await?;
    let sidebars = sidebars_widgets(write).await?;
    let previous = get_theme_mod(write, stylesheet, name, "sidebars_widgets").await?;
    if !matches!(previous, Value::Array(_)) && sidebars.is_empty() {
        return Ok(());
    }
    let sidebars = remove_unregistered(lists(&sidebars)?, &allowed);
    let mapped = map_sidebars_widgets(&sidebars, previous.get_str("data"), &allowed)?;
    let restored = mapped.restored.iter().flat_map(|(_, widgets)| widgets);
    let mut shown: Vec<Vec<u8>> = restored.map(|(_, widget)| widget.to_php_string()).collect();
    shown.extend(
        mapped
            .inactive
            .iter()
            .map(|(_, widget)| widget.to_php_string()),
    );
    // Lost multi widget instances: only the active and default ones are kept.
    let lost: Widgets = allowed
        .iter()
        .filter(|id| !shown.contains(id))
        .filter(|id| !is_template(id))
        .map(|id| Value::String(id.clone()))
        .zip(0..)
        .map(|(id, at)| (Key::Int(at), id))
        .collect();
    let inactive = Value::Array(merged(&[&lost, &mapped.inactive]));
    let mut sidebars = vec![(key("wp_inactive_widgets"), inactive)];
    let restored = mapped.restored.into_iter();
    sidebars.extend(restored.map(|(sidebar, widgets)| (sidebar, Value::Array(widgets))));
    set_sidebars_widgets(write, sidebars).await
}

/// Whether a widget's ID ends in a number below 2, the template of a widget
/// that has no instance.
fn is_template(id: &[u8]) -> bool {
    let id = String::from_utf8_lossy(id);
    let Some((head, number)) = id.rsplit_once('-') else {
        return false;
    };
    let digits = !number.is_empty() && number.bytes().all(|b| b.is_ascii_digit());
    digits && !head.is_empty() && number.parse::<u64>().is_ok_and(|number| number < 2)
}

/// `_wp_menus_changed()` for the active embedded block theme, which
/// registers no menu location: none is kept.
async fn menus_changed(write: &mut Write<'_>, stylesheet: &str, name: &str) -> Result<(), Error> {
    set_theme_mod(
        write,
        (stylesheet, name),
        "nav_menu_locations",
        Value::Array(Vec::new()),
    )
    .await?;
    delete_option(write, "theme_switch_menu_locations").await?;
    Ok(())
}

/// What `check_theme_switched()` does on the next load once a theme was
/// switched to the active embedded block theme `stylesheet` (named `name`):
/// `after_switch_theme`'s menus and widgets carried over. False when no
/// theme was switched; otherwise the caller flushes the rewrite rules, then
/// calls [`theme_switch_done`].
pub async fn after_switch_theme(
    write: &mut Write<'_>,
    stylesheet: &str,
    name: &str,
) -> Result<bool, Error> {
    if get_option(write, "theme_switched").await?.is_empty() {
        return Ok(false);
    }
    // The Customizer carried menus and widgets over up front.
    if get_option(write, "theme_switched_via_customizer")
        .await?
        .is_empty()
    {
        menus_changed(write, stylesheet, name).await?;
        retrieve_widgets(write, stylesheet, name).await?;
    } else {
        update_option(
            write,
            "theme_switched_via_customizer",
            &Value::Bool(false),
            None,
        )
        .await?;
    }
    Ok(true)
}

/// `update_option( 'theme_switched', false )`, the last of
/// `check_theme_switched()`.
pub async fn theme_switch_done(write: &mut Write<'_>) -> Result<(), Error> {
    update_option(write, "theme_switched", &Value::Bool(false), None).await?;
    Ok(())
}

#[cfg(test)]
mod tests;
