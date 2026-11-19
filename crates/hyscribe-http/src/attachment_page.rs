// SPDX-License-Identifier: AGPL-3.0-or-later

//! What a request for an attachment by its ID gets: the redirect to its file
//! that `redirect_canonical()` sends while `wp_attachment_pages_enabled` is
//! off, else the attachment's page, or the 404 page of one a visitor cannot
//! see.

use std::sync::Arc;

use hyscribe_db::DbError;
use hyscribe_model::Post;
use hyscribe_render::attachment::{attachment_url, post_status};
use hyscribe_render::site::Site as RenderSite;

use crate::{Outcome, Paging, Site, lossy, not_found, single_page, tack_with};

/// The post types `is_post_type_viewable()` accepts among the ones a
/// visitor can reach.
const VIEWABLE_TYPES: [&str; 3] = ["post", "page", "attachment"];

/// What an attachment's page needs of the database.
#[derive(Default)]
struct Facts {
    /// `get_post_status()`.
    status: String,
    /// `wp_get_attachment_url()`.
    url: Option<String>,
    /// `is_post_publicly_viewable()` of the parent, when it exists.
    parent_viewable: Option<bool>,
}

/// The outcome of a request that found the attachment `post`; `asked` when
/// it asked for an attachment (`attachment_id`, `attachment`), not just a
/// post of that name, which is one only without a parent.
pub(crate) async fn outcome(
    site: &Arc<Site>,
    post: Post,
    raw_query: Option<&str>,
    asked: bool,
) -> Result<Outcome, DbError> {
    let attachment = asked || post.parent == 0;
    let wanted = post.clone();
    let facts = site
        .read_through(Facts::default(), move |reads| {
            let parent = (wanted.parent != 0).then(|| reads.post(wanted.parent));
            Facts {
                status: post_status(reads, &wanted),
                url: attachment_url(reads, wanted.id).map(|url| lossy(&url)),
                parent_viewable: parent.flatten().map(|parent| {
                    VIEWABLE_TYPES.contains(&parent.post_type.as_str())
                        && post_status(reads, &parent) == "publish"
                }),
            }
        })
        .await?;
    if facts.status != "publish" {
        return Ok(Outcome::Page(Box::new(not_found(site, false))));
    }
    let pages = site.option("wp_attachment_pages_enabled");
    let enabled = !matches!(pages.as_str(), "" | "0");
    // `redirect_canonical()` drops the redirect of a file whose parent
    // cannot be seen.
    if let (true, false, Some(url), true) = (
        attachment,
        enabled,
        &facts.url,
        facts.parent_viewable.unwrap_or(true),
    ) {
        return Ok(Outcome::Redirect(tack_with(url, raw_query, &[], false)));
    }
    let mut page = single_page(site, post, Paging::default());
    page.request.attachment = attachment;
    Ok(Outcome::Page(Box::new(page)))
}
