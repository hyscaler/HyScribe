// SPDX-License-Identifier: AGPL-3.0-or-later

//! The site's own templates and template parts: the `wp_template` and
//! `wp_template_part` posts the site editor saves for the active theme,
//! which take the place of its files, as `resolve_block_template()` and
//! `render_block_core_template_part()` find them. Their content is used as
//! saved: with no hooked blocks, `apply_block_hooks_to_content()` changes
//! nothing a page shows.

use hyscribe_model::{Post, PostQuery};
use hyscribe_phpser::is_empty_string;

use crate::formatting::sanitize_title_for_query;
use crate::post::page_template_slug;
use crate::site::{Request, Site};
use crate::theme::Theme;

/// The published posts of `post_type` for `theme` named one of `names`,
/// newest first, as `WP_Query` finds them with a `wp_theme` taxonomy query.
fn theme_posts(site: &dyn Site, theme: &Theme, post_type: &str, names: &[&[u8]]) -> Vec<Post> {
    let names = names
        .iter()
        .map(|name| String::from_utf8_lossy(&sanitize_title_for_query(name)).into_owned())
        .collect();
    let posts = site.query(&PostQuery {
        post_types: vec![post_type.to_string()],
        names,
        limit: u32::MAX,
        ..PostQuery::default()
    });
    let slug = theme.slug().as_bytes();
    posts
        .into_iter()
        .filter(|post| {
            let themes = site.post_terms(post.id, "wp_theme");
            themes
                .iter()
                .any(|term| term.name.eq_ignore_ascii_case(slug))
        })
        .collect()
}

/// `get_block_templates( array( 'slug__in' => $candidates ) )`, ordered by
/// the candidates as `resolve_block_template()` orders them, and the
/// content `locate_block_template()` renders of the first for a visitor:
/// a customised template emptied shows its theme file, if it has one.
pub fn block_template(theme: &Theme, site: &dyn Site, candidates: &[String]) -> Option<Vec<u8>> {
    let slugs: Vec<&[u8]> = candidates.iter().map(String::as_bytes).collect();
    let mut unique = slugs.clone();
    unique.sort_unstable();
    unique.dedup();
    let mut posts = theme_posts(site, theme, "wp_template", &slugs);
    posts.truncate(unique.len());
    // A slug not asked for has no priority, which PHP takes as 0.
    let priority = |slug: &[u8]| slugs.iter().position(|s| *s == slug).unwrap_or(0);
    let files = candidates
        .iter()
        .filter(|slug| !posts.iter().any(|post| post.name == slug.as_bytes()))
        .filter_map(|slug| theme.template(std::slice::from_ref(slug)))
        .map(|(slug, content)| (priority(slug.as_bytes()), content.to_vec()))
        .collect::<Vec<_>>();
    let customised = posts.into_iter().map(|post| {
        let file = theme.template(&[String::from_utf8_lossy(&post.name).into_owned()]);
        let content = match file {
            Some((_, file)) if is_empty_string(&post.content) => file.to_vec(),
            _ => post.content,
        };
        (priority(&post.name), content)
    });
    // The first of those of the highest priority, as PHP's sort is stable.
    let first = customised.chain(files).min_by_key(|(at, _)| *at);
    first.map(|(_, content)| content)
}

/// The template part `slug` the site has: its published post for the
/// theme, newest first, with its area term, else the theme's file.
pub(crate) fn template_part(
    theme: &Theme,
    site: &dyn Site,
    slug: &[u8],
) -> Option<(Vec<u8>, Vec<u8>)> {
    let post = theme_posts(site, theme, "wp_template_part", &[slug])
        .into_iter()
        .next();
    let Some(post) = post else {
        let (content, area) = theme.part(slug)?;
        return Some((content.to_vec(), area.to_vec()));
    };
    let areas = site.post_terms(post.id, "wp_template_part_area");
    let area = areas
        .into_iter()
        .next()
        .map_or_else(|| b"uncategorized".to_vec(), |term| term.name);
    Some((post.content, area))
}

/// `candidates` with the template a singular post's `_wp_page_template`
/// names first among its type's own: after `front-page` on the front page,
/// before `singular` and `index`, which end the list. Its type's templates
/// keep their place when one repeats it, as `array_flip()` keeps the last.
pub fn with_page_template(
    site: &dyn Site,
    request: &Request,
    candidates: &[String],
) -> Vec<String> {
    let mut candidates = candidates.to_vec();
    let at = usize::from(request.front_page);
    let own = candidates.get(at..candidates.len().saturating_sub(2));
    if let Some(slug) =
        page_template(site, request).filter(|slug| own.is_some_and(|own| !own.contains(slug)))
    {
        candidates.insert(at, slug);
    }
    candidates
}

/// The template a singular post's `_wp_page_template` names, which
/// `get_page_template()` and `get_single_template()` try first: none for a
/// path `validate_file()` refuses, and without a `.php` or `.html` suffix
/// (`_strip_template_file_suffix()`).
fn page_template(site: &dyn Site, request: &Request) -> Option<String> {
    let post = request.posts.first().filter(|_| request.singular)?;
    if post.post_type == "attachment" {
        return None;
    }
    let slug = page_template_slug(site, post.id)?;
    let slug = String::from_utf8_lossy(&slug).into_owned();
    if !valid_file(&slug) {
        return None;
    }
    let stripped = [".php", ".html"]
        .into_iter()
        .find_map(|suffix| slug.strip_suffix(suffix));
    Some(stripped.unwrap_or(&slug).to_string())
}

/// `validate_file( $file ) === 0`: no `../` but one ending the path, which
/// is not `../` alone, and no drive letter.
fn valid_file(file: &str) -> bool {
    let file = file.replace('\\', "/");
    let ups = file.matches("../").count();
    let ups_ok = ups == 0 || (ups == 1 && file.ends_with("../") && file != "../");
    ups_ok && file.get(1..2) != Some(":")
}

#[cfg(test)]
mod tests;
