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
fn mods_option(stylesheet: &str) -> String {
    format!("theme_mods_{stylesheet}")
}

fn key(name: &str) -> Key {
    Key::String(name.as_bytes().to_vec())
}

fn text(value: &[u8]) -> String {
    String::from_utf8_lossy(value).into_owned()
}

/// `get_theme_mods()` of the active theme `stylesheet` (named `name`): the
/// theme's option, which moves from its deprecated place only in an old
/// site, not ported.
async fn theme_mods(
    write: &mut Write<'_>,
    stylesheet: &str,
    name: &str,
) -> Result<Vec<(Key, Value)>, Error> {
    match get_option(write, &mods_option(stylesheet)).await? {
        Value::Array(mods) => Ok(mods),
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
            Ok(Vec::new())
        }
        _ => Ok(Vec::new()),
    }
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
    let Some(current) = write.option_autoload(name).await? else {
        return Ok(());
    };
    if current != wanted {
        let value = write.option_value(name).await?.unwrap_or_default();
        write.update_option_autoload(name, &value, wanted).await?;
    }
    Ok(())
}

/// `delete_site_transient()`.
async fn delete_site_transient(write: &mut Write<'_>, name: &str) -> Result<(), Error> {
    if delete_option(write, &format!("_site_transient_{name}")).await? {
        delete_option(write, &format!("_site_transient_timeout_{name}")).await?;
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
            let name = option.strip_prefix("_site_transient_").unwrap_or_default();
            delete_site_transient(write, name).await?;
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

/// `_wp_remove_unregistered_widgets()`.
fn remove_unregistered(sidebars: Vec<(Key, Value)>, allowed: &[Vec<u8>]) -> Vec<(Key, Value)> {
    sidebars
        .into_iter()
        .map(|(sidebar, widgets)| match widgets {
            Value::Array(widgets) => {
                let kept = widgets
                    .into_iter()
                    .filter(|(_, widget)| allowed.contains(&widget.to_php_string()))
                    .collect();
                (sidebar, Value::Array(kept))
            }
            other => (sidebar, other),
        })
        .collect()
}

/// A sidebar's widgets as the array they are, else not ported.
fn widgets_of(value: &Value) -> Result<&Widgets, Error> {
    match value {
        Value::Array(widgets) => Ok(widgets),
        _ => Err(Error::Unported("a sidebar that is not a list of widgets")),
    }
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

/// `wp_map_sidebars_widgets()` of a theme that registers no sidebar, with
/// `previous` the `sidebars_widgets` theme mod's data from when the theme
/// was last active.
fn map_sidebars_widgets(
    existing: Vec<(Key, Value)>,
    previous: Option<&Value>,
    allowed: &[Vec<u8>],
) -> Result<Vec<(Key, Value)>, Error> {
    let inactive_key = key("wp_inactive_widgets");
    let with = |inactive: Widgets| vec![(inactive_key.clone(), Value::Array(inactive))];
    let orphaned = |sidebar: &Key| matches!(sidebar, Key::String(name) if name.starts_with(b"orphaned_widgets"));
    let mut inactive = Widgets::new();
    if existing.is_empty() {
        return Ok(with(inactive));
    }
    // The inactive and orphaned widgets first, then those left over.
    let mut left = Vec::new();
    for (sidebar, widgets) in &existing {
        if *sidebar == inactive_key || orphaned(sidebar) {
            inactive = merged(&[&inactive, widgets_of(widgets)?]);
        } else {
            left.push(widgets_of(widgets)?);
        }
    }
    for widgets in left {
        if !widgets.is_empty() {
            inactive = merged(&[&inactive, widgets]);
        }
    }
    let Some(Value::Array(previous)) = previous else {
        return Ok(with(inactive));
    };
    // Sidebars from when the theme was last active, but the empty ones, the
    // inactive ones if some are inactive now, and the orphaned ones.
    let mut old: Vec<(Key, Value)> = previous
        .iter()
        .filter(|(_, widgets)| !widgets.is_empty())
        .filter(|(sidebar, _)| inactive.is_empty() || *sidebar != inactive_key)
        .filter(|(sidebar, _)| !orphaned(sidebar))
        .cloned()
        .collect();
    old = remove_unregistered(old, allowed);
    // The widgets revived leave the inactive ones.
    for (_, widgets) in &old {
        let snapshot = inactive.clone();
        for (_, widget) in widgets_of(widgets)? {
            if let Some((found, _)) = snapshot.iter().find(|(_, listed)| listed == widget) {
                inactive.retain(|(at, _)| at != found);
            }
        }
    }
    // What the theme had comes back, over the sidebars of the same name.
    let mut new = with(inactive);
    for (sidebar, widgets) in old {
        match new.iter_mut().find(|(known, _)| *known == sidebar) {
            Some((_, current)) => *current = widgets,
            None => new.push((sidebar, widgets)),
        }
    }
    Ok(new)
}

/// `wp_set_sidebars_widgets()`: the option, versioned.
async fn set_sidebars_widgets(
    write: &mut Write<'_>,
    mut sidebars: Vec<(Key, Value)>,
) -> Result<(), Error> {
    if entry(&sidebars, "array_version").is_none() {
        sidebars.push((key("array_version"), Value::Int(3)));
    }
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
    let sidebars = remove_unregistered(sidebars, &allowed);
    let data = previous.get_str("data");
    let mut sidebars = map_sidebars_widgets(sidebars, data, &allowed)?;
    // A value that is not a list is an empty one.
    for (_, widgets) in &mut sidebars {
        if !matches!(widgets, Value::Array(_)) {
            *widgets = Value::Array(Vec::new());
        }
    }
    let mut shown: Vec<Vec<u8>> = Vec::new();
    for (_, widgets) in &sidebars {
        shown.extend(
            widgets_of(widgets)?
                .iter()
                .map(|(_, widget)| widget.to_php_string()),
        );
    }
    // Lost multi widget instances: only the active and default ones are kept.
    let lost: Widgets = allowed
        .iter()
        .filter(|id| !shown.contains(id))
        .filter(|id| !is_template(id))
        .map(|id| Value::String(id.clone()))
        .zip(0..)
        .map(|(id, at)| (Key::Int(at), id))
        .collect();
    let inactive = key("wp_inactive_widgets");
    let current = sidebars.iter().find(|(known, _)| *known == inactive);
    let current = match current {
        Some((_, widgets)) => widgets_of(widgets)?.clone(),
        None => Widgets::new(),
    };
    let all_inactive = Value::Array(merged(&[&lost, &current]));
    match sidebars.iter_mut().find(|(known, _)| *known == inactive) {
        Some((_, widgets)) => *widgets = all_inactive,
        None => sidebars.push((inactive, all_inactive)),
    }
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
