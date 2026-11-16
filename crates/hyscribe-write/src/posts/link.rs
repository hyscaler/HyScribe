// SPDX-License-Identifier: AGPL-3.0-or-later

//! `get_permalink()` of a saved post, the GUID `wp_insert_post()` and
//! `_transition_post_status()` give it: the front end's links over what
//! the action reads of the post's category, author and parents.

use hyscribe_db::{DbError, PostFields, Write};
use hyscribe_model::Post;
use hyscribe_phpser::Value;
use hyscribe_render::link::{front_page, permalink as plain_link};
use hyscribe_render::permalinks::{Permalinks, PostParts};

use super::{Context, Result, site, truthy};
use crate::caps::Caps;

/// `get_permalink( $id )` of post `id`, saved as `post`.
pub(super) async fn permalink(
    write: &mut Write<'_>,
    ctx: &Context,
    id: u64,
    post: &PostFields,
) -> Result<Vec<u8>> {
    let option = async |write: &mut Write<'_>, name| {
        let value = write.option_value(name).await?.unwrap_or_default();
        Ok::<_, DbError>(String::from_utf8_lossy(&value).into_owned())
    };
    let links = Permalinks::new(&option(write, "permalink_structure").await?, "", "");
    let show_on_front = option(write, "show_on_front").await?;
    let page_on_front = option(write, "page_on_front").await?;
    let front = front_page(show_on_front.as_bytes(), page_on_front.as_bytes());
    let link = Post {
        id,
        date: post.date.clone(),
        // `post_link()` checks as a visitor would; `force_plain()` is ours.
        status: "publish".into(),
        name: post.name.clone(),
        post_type: post.post_type.clone(),
        ..Post::default()
    };
    if !links.pretty() || force_plain(write, ctx, id, post).await? {
        return Ok(plain_link(&ctx.home, front, &link));
    }
    // Read whether or not the structure or the type takes them.
    let author = write.user(post.author).await?;
    let parts = PostParts {
        category: category(write, id).await?,
        author: author
            .map(|(_, user)| String::from_utf8_lossy(&user.nicename).into_owned())
            .unwrap_or_default(),
        page_uri: page_uri(write, id, post).await?,
    };
    let home = String::from_utf8_lossy(&ctx.home);
    // Only types core registers are saved, and each has a pretty link.
    let pretty = links.post_link(&home, front, &link, &parts);
    Ok(pretty.map(String::into_bytes).unwrap_or_default())
}

/// `wp_force_plain_post_permalink( $post )`: only a published post, or a
/// private one the current user may read, takes a pretty link.
async fn force_plain(
    write: &mut Write<'_>,
    ctx: &Context,
    id: u64,
    post: &PostFields,
) -> Result<bool> {
    match post.status.as_str() {
        "publish" => Ok(false),
        "private" => {
            let caps = Caps::load(write, ctx.user, Some(id), ctx.constants).await?;
            Ok(!caps.can("read_post", &[Value::Int(id.cast_signed())]))
        }
        _ => Ok(true),
    }
}

/// `%category%`: the slugs from the top of the post's category with the
/// lowest ID, or the default category's slug when that leaves none.
async fn category(write: &mut Write<'_>, id: u64) -> Result<String> {
    let terms = write.object_terms(id, "category").await?;
    let mut category = Vec::new();
    if let Some(lowest) = terms.iter().map(|&(term, _)| term).min()
        && let Some(term) = write.term_in("category", lowest).await?
    {
        if term.parent != 0 {
            category = category_parents(write, term.parent).await?;
        }
        category.extend(term.slug);
    }
    if !truthy(&category) {
        let default = site::option_int(write, "default_category").await?;
        let term = write.term_in("category", default).await?;
        category = term.map(|term| term.slug).unwrap_or_default();
    }
    Ok(String::from_utf8_lossy(&category).into_owned())
}

/// `get_category_parents( $id, false, '/', true )`: each slug from the top
/// down to `id`'s, followed by `/`. A missing term has an empty slug.
async fn category_parents(write: &mut Write<'_>, id: u64) -> Result<Vec<u8>> {
    let Some(term) = write.term_in("category", id).await? else {
        return Ok(Vec::new());
    };
    let (mut slugs, mut parent) = (vec![term.slug], term.parent);
    // get_ancestors(): the parents, up to none or one already among them.
    let mut ancestors = vec![0];
    while !ancestors.contains(&parent) {
        ancestors.push(parent);
        let term = write.term_in("category", parent).await?;
        let slug;
        (slug, parent) = term.map_or((Vec::new(), 0), |term| (term.slug, term.parent));
        slugs.push(slug);
    }
    Ok(slugs
        .iter()
        .rev()
        .flat_map(|slug| [&slug[..], b"/"].concat())
        .collect())
}

/// `get_page_uri()`: the page's name after those of its ancestors that
/// have one, as `get_post_ancestors()` finds them, the page ending a loop.
async fn page_uri(write: &mut Write<'_>, id: u64, page: &PostFields) -> Result<String> {
    let (mut ancestors, mut parent) = (vec![0, id], page.parent);
    let mut names = Vec::new();
    while !ancestors.contains(&parent) {
        ancestors.push(parent);
        let ancestor = write.post(parent).await?;
        let name;
        (name, parent) = ancestor.map_or((Vec::new(), 0), |post| (post.name, post.parent));
        names.push(name);
    }
    let names = names.iter().rev().filter(|name| !name.is_empty());
    let uri: Vec<u8> = names.flat_map(|name| [&name[..], b"/"].concat()).collect();
    Ok(String::from_utf8_lossy(&[uri, page.name.clone()].concat()).into_owned())
}
