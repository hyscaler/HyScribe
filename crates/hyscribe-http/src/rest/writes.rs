// SPDX-License-Identifier: AGPL-3.0-or-later

//! The REST API's writes (MVP 4.3, ADR 0013's amendment): a request whose
//! checks pass in `hyscribe-rest` gives a plan, written here in one action
//! of the writer (ADR 0012) with `hyscribe-write`'s ported functions, as
//! `saves::act()` writes hs-admin's; the answer is then made from the site
//! reloaded, as PHP prepares the post it has just written.

use std::cell::OnceCell;

use hyscribe_db::{DbError, Meta, Write};
use hyscribe_phpser::Value;
use hyscribe_rest::{Outcome, Pending, Plan, Request, Save, Served};
use hyscribe_write::Error;
use hyscribe_write::options::{delete_metadata, update_metadata};
use hyscribe_write::posts::{
    Context, PostArgs, Revisioned, stick_post, unstick_post, wp_create_post_autosave,
    wp_delete_post, wp_insert_post, wp_trash_post, wp_unique_post_slug, wp_update_post,
};
use hyscribe_write::terms::{Taxonomy, TermList, TermRef, wp_set_object_terms, wp_set_post_terms};
use tokio::runtime::Handle;

use axum::response::Response;

use super::{Rest, sent};
use crate::Site;
use crate::login::LoginReads;

/// The current user, their session's token, the time and the runtime.
pub(super) type Asker = (u64, Vec<u8>, i64, Handle);

/// The answer to a write whose checks passed: refused without a writer
///, else `pending`'s plan written in one action and the answer made
/// from the site reloaded; 501 for what `hyscribe-write` does not port,
/// and the database error page for an error, the write rolled back.
pub(super) fn answer(
    site: &Site,
    request: &Request,
    pending: Pending,
    (user, token, now, handle): Asker,
) -> Response {
    let Some(writer) = &site.writer else {
        return sent(read_only());
    };
    let plan = &pending.plan;
    let ctx = context(site, plan, now);
    let outcome = handle.block_on(async { act(writer.begin().await, plan, &ctx).await });
    let served = match outcome {
        // What PHP refuses past the checks only as another write gets in.
        Err(Error::Unported(_) | Error::Refused(_)) => Ok(Served::not_ported()),
        Err(err) => Err(err),
        Ok(outcome) => {
            let reloaded = handle.block_on(site.reloaded());
            let fresh = reloaded.as_ref().unwrap_or(site);
            let mut reads = LoginReads::new(fresh, handle, now);
            reads.act_as(user, token);
            let rest = Rest {
                reads,
                site: fresh,
                user,
                caps: OnceCell::new(),
            };
            let served = hyscribe_rest::written(&rest, request, pending, outcome);
            let Rest { reads, .. } = rest;
            reads.reads.finish().map(|()| served).map_err(Error::Db)
        }
    };
    match served {
        Ok(served) => sent(served),
        Err(err) => crate::html(crate::database_error(site, &format!("{err:?}"))),
    }
}

/// The posts' context of `plan` on `site` at `now`, its uploads on the
/// host of `siteurl` (`wp_upload_dir()`).
pub(super) fn context(site: &Site, plan: &Plan, now: i64) -> Context {
    Context {
        user: plan.user,
        can_publish: plan.can_publish,
        home: site.home.as_bytes().to_vec(),
        upload_host: hyscribe_render::link::host(site.raw_option("siteurl")),
        unfiltered_html: plan.unfiltered_html,
        constants: site.constants,
        now,
    }
}

/// Shadow mode's refusal, `wp_die()`'s JSON as a REST request gets it.
fn read_only() -> Served {
    let error = format!(
        r#"{{"code":"wp_die","message":"{}","data":{{"status":403}},"additional_errors":[]}}"#,
        hyscribe_admin::login::READ_ONLY
    );
    Served {
        status: 403,
        headers: vec![(
            "Content-Type".into(),
            "application/json; charset=UTF-8".into(),
        )],
        body: error.into_bytes(),
    }
}

/// Writes `plan` as one action in `write`, once begun: committed when it
/// is written, rolled back when PHP refuses it or an error stops it.
pub(super) async fn act(
    write: Result<Write<'_>, DbError>,
    plan: &Plan,
    ctx: &Context,
) -> Result<Outcome, Error> {
    let mut write = write?;
    let outcome = written(&mut write, plan, ctx).await?;
    match outcome {
        Outcome::Written(_) => write.commit().await.map(|()| outcome).map_err(Into::into),
        Outcome::Refused(_) | Outcome::InvalidText(_) => Ok(outcome),
    }
}

/// Writes `plan` in `write`: what the write came to, or the error that
/// stopped it.
async fn written(write: &mut Write<'_>, plan: &Plan, ctx: &Context) -> Result<Outcome, Error> {
    let done = |written: bool, id: u64| match written {
        true => Outcome::Written(id),
        false => Outcome::Refused("rest_cannot_delete"),
    };
    match &plan.write {
        hyscribe_rest::Write::Save(save) | hyscribe_rest::Write::Autosave(save) => {
            let slug = slug(write, save).await?;
            match plan.write {
                hyscribe_rest::Write::Save(_) => saved(write, save, slug, ctx).await,
                _ => autosaved(write, save, slug, ctx).await,
            }
        }
        hyscribe_rest::Write::Trash(id) => Ok(done(wp_trash_post(write, ctx, *id).await?, *id)),
        hyscribe_rest::Write::Delete(id) => {
            Ok(done(wp_delete_post(write, ctx, *id, true).await?, *id))
        }
    }
}

/// The fields `save` gives, on `wp_insert_post()`'s arguments.
fn fields(save: &Save, slug: Option<&Vec<u8>>, args: &mut PostArgs) {
    let put = |field: &mut Vec<u8>, value: &Option<Vec<u8>>| {
        if let Some(value) = value {
            field.clone_from(value);
        }
    };
    put(&mut args.title, &save.title);
    put(&mut args.content, &save.content);
    put(&mut args.excerpt, &save.excerpt);
    put(&mut args.password, &save.password);
    let text = |field: &mut String, value: &Option<String>| {
        if let Some(value) = value {
            field.clone_from(value);
        }
    };
    text(&mut args.status, &save.status);
    text(&mut args.comment_status, &save.comment_status);
    text(&mut args.ping_status, &save.ping_status);
    if let Some((date, gmt)) = &save.date {
        args.date.clone_from(date);
        args.date_gmt.clone_from(gmt);
    }
    args.edit_date = save.edit_date;
    if let Some(slug) = slug {
        args.name = Some(slug.clone());
    }
    args.author = save.author.or(args.author);
    args.parent = save.parent.unwrap_or(args.parent);
    args.menu_order = save.menu_order.unwrap_or(args.menu_order);
    // Only flat taxonomies are given by name, which `Taxonomy` ports.
    let tax_input = save.tax_input.iter().filter_map(|(taxonomy, terms)| {
        Taxonomy::named(taxonomy).map(|taxonomy| (taxonomy, terms.clone()))
    });
    args.tax_input = tax_input.collect();
    args.meta_input.clone_from(&save.meta_input);
}

/// The slug `save` gives, made unique where the controller does.
async fn slug(write: &mut Write<'_>, save: &Save) -> Result<Option<Vec<u8>>, Error> {
    let mut slug = save.slug.clone();
    if let (Some(parent), Some(name)) = (save.unique_slug, &slug) {
        let unique = wp_unique_post_slug(
            write,
            name.clone(),
            (save.id, "publish"),
            &save.post_type,
            parent,
        );
        slug = Some(unique.await?);
    }
    Ok(slug)
}

/// The autosaves controller's `create_item()` from its write on.
async fn autosaved(
    write: &mut Write<'_>,
    save: &Save,
    slug: Option<Vec<u8>>,
    ctx: &Context,
) -> Result<Outcome, Error> {
    let revisioned = Revisioned {
        title: save.title.clone(),
        content: save.content.clone(),
        excerpt: save.excerpt.clone(),
    };
    let change = |args: &mut PostArgs| fields(save, slug.as_ref(), args);
    match wp_create_post_autosave(write, ctx, save.id, &revisioned, &change).await {
        Ok(id) => Ok(Outcome::Written(id)),
        Err(Error::Refused("empty_content")) => Ok(Outcome::Refused("empty_content")),
        Err(err) => Err(err),
    }
}

/// `create_item()` or `update_item()` from `wp_insert_post()` on.
async fn saved(
    write: &mut Write<'_>,
    save: &Save,
    slug: Option<Vec<u8>>,
    ctx: &Context,
) -> Result<Outcome, Error> {
    let saved = if save.id == 0 {
        let mut args = PostArgs {
            post_type: save.post_type.clone(),
            ..PostArgs::default()
        };
        fields(save, slug.as_ref(), &mut args);
        wp_insert_post(write, ctx, args).await
    } else {
        let change = |args: &mut PostArgs| fields(save, slug.as_ref(), args);
        wp_update_post(write, ctx, save.id, &change).await
    };
    let id = match saved {
        Ok(id) => id,
        Err(Error::Refused("empty_content")) => return Ok(Outcome::Refused("empty_content")),
        Err(Error::InvalidText(last_error)) => return Ok(Outcome::InvalidText(last_error)),
        Err(err) => return Err(err),
    };
    match save.sticky {
        Some(true) => stick_post(write, id).await?,
        Some(false) => unstick_post(write, id).await?,
        None => {}
    }
    let any = Value::String(Vec::new());
    match save.thumbnail {
        Some(0) => {
            delete_metadata(write, Meta::Post, id, "_thumbnail_id", &any, false).await?;
        }
        Some(image) => {
            let image = Value::Int(image.cast_signed());
            update_metadata(write, Meta::Post, id, "_thumbnail_id", &image, &any).await?;
        }
        None => {}
    }
    if let Some(format) = &save.format {
        let format = TermList::Text(format);
        wp_set_post_terms(write, id, format, Taxonomy::PostFormat, false).await?;
    }
    if let Some(template) = &save.template {
        let template = Value::String(template.clone());
        update_metadata(write, Meta::Post, id, "_wp_page_template", &template, &any).await?;
    }
    for (taxonomy, ids) in [
        (Taxonomy::Category, &save.categories),
        (Taxonomy::PostTag, &save.tags),
        (Taxonomy::PatternCategory, &save.pattern_categories),
    ] {
        if let Some(ids) = ids {
            let terms: Vec<TermRef> = ids.iter().map(|&id| TermRef::Id(id)).collect();
            wp_set_object_terms(write, id, &terms, taxonomy, false).await?;
        }
    }
    for (key, value) in &save.meta {
        update_metadata(write, Meta::Post, id, key, value, &Value::Null).await?;
    }
    Ok(Outcome::Written(id))
}
