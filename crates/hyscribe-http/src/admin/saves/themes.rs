// SPDX-License-Identifier: AGPL-3.0-or-later

//! The Themes screen's activation (`themes.php?action=activate`), and what
//! the next load does once a theme was switched (`check_theme_switched()`).

use hyscribe_admin::actions::Asked;
use hyscribe_admin::posts::Answer;
use hyscribe_db::Write;
use hyscribe_phpser::{Value, lookup};
use hyscribe_render::theme;
use hyscribe_write::Error;
use hyscribe_write::options::get_option;
use hyscribe_write::themes::{after_switch_theme, switch_theme, theme_switch_done};

use super::options::flush_rewrite_rules;
use super::{Acting, Saving, Stop, text};
use crate::Site;

/// What `wp_die()` says of a theme the site does not have.
const MISSING: &str = "<h1>An error occurred.</h1><p>The requested theme does not exist.</p>";

/// An embedded theme's stylesheet and its `Name` header.
fn embedded(stylesheet: &[u8]) -> Option<(&'static str, String)> {
    let slug = theme::slugs().find(|slug| slug.as_bytes() == stylesheet)?;
    let entry = theme::screen_entry(slug).unwrap_or_default();
    let name = entry.get_str("name").map(Value::to_php_string);
    Some((slug, text(&name.unwrap_or_default())))
}

/// `switch_theme()` of the theme `stylesheet` names, for a user who may
/// switch themes and passed the nonce, then the Themes screen. Only the
/// embedded themes exist, and only a site on one is switched.
pub(super) fn activate<'a>(
    write: &'a mut Write<'_>,
    asked: &'a Asked<'a>,
    site: &'a dyn Acting,
) -> Saving<'a> {
    Box::pin(async move {
        let wanted = lookup(asked.get, "stylesheet").map(Value::to_php_string);
        let Some(new) = embedded(&wanted.unwrap_or_default()) else {
            return Err(Stop::Died(Answer::Denied(MISSING)));
        };
        let old = get_option(write, "stylesheet").await?.to_php_string();
        let Some(old) = embedded(&old) else {
            return Err(Error::Unported("a switch from a theme the site does not have").into());
        };
        let template = theme::template_of(new.0).unwrap_or(new.0);
        switch_theme(write, (new.0, &new.1), template, (old.0, &old.1)).await?;
        let to = site.admin_url("themes.php?activated=true");
        Ok(Answer::Redirect(to))
    })
}

/// The site once `check_theme_switched()` ran, which every request runs on
/// `init`: after a switch to an embedded theme the menus and widgets of the
/// theme left are carried over and the rewrite rules flushed. A site
/// without a writer, or on a theme that is not embedded, is as it was.
pub(crate) async fn checked(site: Site) -> Site {
    let switched = site.options.get("theme_switched");
    let Some(writer) = site
        .writer
        .as_ref()
        .filter(|_| switched.is_some_and(|value| is_set(value)))
    else {
        return site;
    };
    let stylesheet = site.options.get("stylesheet").cloned().unwrap_or_default();
    let Some((slug, name)) = embedded(&stylesheet) else {
        return site;
    };
    let ran = async {
        let Ok(mut write) = writer.begin().await else {
            return false;
        };
        follow_up(&mut write, slug, &name).await.is_ok() && write.commit().await.is_ok()
    };
    match ran.await {
        true => site.reloaded().await.unwrap_or(site),
        false => site,
    }
}

/// `check_theme_switched()` in one action: the menus and widgets carried
/// over, the rewrite rules flushed, the switch done.
async fn follow_up(write: &mut Write<'_>, slug: &str, name: &str) -> Result<(), Error> {
    if after_switch_theme(write, slug, name).await? {
        flush_rewrite_rules(write).await?;
        theme_switch_done(write).await?;
    }
    Ok(())
}

/// Whether an option's stored value is set: not empty, and not `0`.
fn is_set(value: &[u8]) -> bool {
    !value.is_empty() && value != b"0"
}

#[cfg(test)]
mod tests;
