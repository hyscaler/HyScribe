// SPDX-License-Identifier: AGPL-3.0-or-later

//! The REST API's writes (MVP 4.3, ADR 0013's amendment): a request whose
//! checks pass in `hyscribe-rest` gives a plan, written here in one action
//! of the writer (ADR 0012) with `hyscribe-write`'s ported functions, as
//! `saves::act()` writes hs-admin's; the answer is then made from the site
//! reloaded, as PHP prepares the post it has just written.

use std::cell::OnceCell;
use std::sync::Arc;

use hyscribe_db::{DbError, Meta, Write};
use hyscribe_phpser::Value;
use hyscribe_phpser::addslashes;
use hyscribe_render::date::Zone;
use hyscribe_rest::{
    Answer, CommentSave, Outcome, Pending, Plan, Request, Save, Served, Setting, SettingsSave,
    TermSave, UserSave,
};
use hyscribe_write::Error;
use hyscribe_write::comments::{
    CommentEdit, Commenter, NewComment, Refused, allow_and_insert_comment, delete_comment,
    restore_comment, set_comment_status, spam_comment, trash_comment, update_comment,
};
use hyscribe_write::options::{
    add_option, delete_metadata, delete_option, get_option, sanitize_option, update_metadata,
    update_option,
};
use hyscribe_write::posts::{
    Context, PostArgs, Revisioned, stick_post, unstick_post, wp_create_post_autosave,
    wp_delete_post, wp_insert_post, wp_trash_post, wp_unique_post_slug, wp_update_post,
};
use hyscribe_write::terms::{
    Deletion, Taxonomy, TermArgs, TermError, TermList, TermRef, wp_delete_term, wp_insert_term,
    wp_set_object_terms, wp_set_post_terms, wp_update_term,
};
use hyscribe_write::users::{UserData, add_role, wp_delete_user, wp_insert_user, wp_update_user};
use tokio::runtime::Handle;

use axum::response::Response;

use super::{Rest, sent};
use crate::Site;
use crate::login::LoginReads;
use crate::mail::{Outbox, queue_changes};

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
    let outcome = made(site, writer, &pending.plan, now, &handle);
    finished(
        site,
        writer,
        request,
        pending,
        outcome,
        (user, token, now, handle),
    )
}

/// `plan` written with `writer` in one action, the mail it queues sent once
/// it commits.
fn made(
    site: &Site,
    writer: &hyscribe_db::Writer,
    plan: &Plan,
    now: i64,
    handle: &Handle,
) -> Result<Outcome, Error> {
    let ctx = context(site, plan, now);
    let outbox = Outbox::new(Arc::clone(&site.mailer));
    handle.block_on(async { act(writer.begin().await, plan, &ctx, &outbox).await })
}

/// The answer to a write that came to `outcome`: made from the site
/// reloaded, 501 for what `hyscribe-write` does not port, and the database
/// error page for an error, the write rolled back. A batch's next write is
/// made with `writer` in its own action, one after the other.
pub(super) fn finished(
    site: &Site,
    writer: &hyscribe_db::Writer,
    request: &Request,
    pending: Pending,
    outcome: Result<Outcome, Error>,
    (user, token, now, handle): Asker,
) -> Response {
    let (mut pending, mut outcome) = (pending, outcome);
    loop {
        let answered = match outcome {
            // What PHP refuses past the checks only as another write gets in.
            Err(Error::Unported(_) | Error::Refused(_)) => Ok(Answer::Served(Served::not_ported())),
            Err(err) => Err(err),
            Ok(outcome) => {
                let reloaded = handle.block_on(site.reloaded());
                let fresh = reloaded.as_ref().unwrap_or(site);
                let mut reads = LoginReads::new(fresh, handle.clone(), now);
                reads.act_as(user, token.clone());
                let rest = Rest {
                    reads,
                    site: fresh,
                    user,
                    caps: OnceCell::new(),
                };
                let answered = hyscribe_rest::written(&rest, request, pending, outcome);
                let Rest { reads, .. } = rest;
                reads.reads.finish().map(|()| answered).map_err(Error::Db)
            }
        };
        match answered {
            Ok(Answer::Served(served)) => return sent(served),
            Ok(Answer::Write(next)) => {
                pending = *next;
                outcome = made(site, writer, &pending.plan, now, &handle);
            }
            Err(err) => return crate::html(crate::database_error(site, &format!("{err:?}"))),
        }
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
pub(super) fn read_only() -> Served {
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
/// is written, then the mails it queued in `outbox` sent; rolled back,
/// its mails dropped, when PHP refuses it or an error stops it.
pub(super) async fn act(
    write: Result<Write<'_>, DbError>,
    plan: &Plan,
    ctx: &Context,
    outbox: &Outbox,
) -> Result<Outcome, Error> {
    let mut write = write?;
    let outcome = match written(&mut write, plan, ctx, outbox).await {
        Ok(outcome @ (Outcome::Written(_) | Outcome::Cached(_))) => {
            write.commit().await.map(|()| outcome).map_err(Into::into)
        }
        outcome => outcome,
    };
    match outcome {
        Ok(Outcome::Written(_) | Outcome::Cached(_)) => {
            outbox.send().await;
        }
        _ => outbox.clear(),
    }
    outcome
}

/// Writes `plan` in `write`: what the write came to, or the error that
/// stopped it.
async fn written(
    write: &mut Write<'_>,
    plan: &Plan,
    ctx: &Context,
    outbox: &Outbox,
) -> Result<Outcome, Error> {
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
        // Its file is written first, by `rest::uploads`.
        hyscribe_rest::Write::Upload(_) => Err(Error::Unported("an upload's file")),
        hyscribe_rest::Write::Term(save) => term_saved(write, save).await,
        hyscribe_rest::Write::DeleteTerm(taxonomy, id) => {
            let deleted = wp_delete_term(write, taxonomy_of(taxonomy), *id).await;
            Ok(done(term_kept(deleted)? == Ok(Deletion::Deleted), *id))
        }
        hyscribe_rest::Write::Settings(save) => settings_saved(write, save).await,
        hyscribe_rest::Write::Comment(save) => comment_saved(write, save, plan, ctx).await,
        hyscribe_rest::Write::TrashComment(id) => {
            let trashed = trash_comment(write, comments_context(ctx), *id).await?;
            Ok(done(trashed, *id))
        }
        hyscribe_rest::Write::DeleteComment(id) => {
            let deleted = delete_comment(write, comments_context(ctx), *id, true).await?;
            Ok(done(deleted, *id))
        }
        hyscribe_rest::Write::User(save) => user_saved(write, save, ctx, outbox).await,
        hyscribe_rest::Write::DeleteUser(id, reassign) => {
            Ok(done(wp_delete_user(write, *id, *reassign, ctx).await?, *id))
        }
    }
}

/// A user's create or update from `wp_insert_user()` or `wp_update_user()`
/// on, its fields slashed, an update's notices to the user queued in
/// `outbox`, then each role added.
async fn user_saved(
    write: &mut Write<'_>,
    save: &UserSave,
    ctx: &Context,
    outbox: &Outbox,
) -> Result<Outcome, Error> {
    let slashed = |field: &Option<Vec<u8>>| field.as_deref().map(addslashes);
    let data = UserData {
        id: save.id,
        user_login: slashed(&save.login),
        user_pass: slashed(&save.pass),
        user_email: slashed(&save.email),
        display_name: slashed(&save.display_name),
        first_name: slashed(&save.first_name),
        last_name: slashed(&save.last_name),
        nickname: slashed(&save.nickname),
        user_nicename: slashed(&save.nicename),
        description: slashed(&save.description),
        user_url: slashed(&save.url),
        locale: slashed(&save.locale),
        role: save.roles.as_ref().map(|_| Vec::new()),
        ..UserData::default()
    };
    let saved = match save.id {
        0 => wp_insert_user(write, &data).await,
        _ => match wp_update_user(write, &data).await {
            Ok(updated) => {
                let home = String::from_utf8_lossy(&ctx.home);
                queue_changes(write, &home, &updated, outbox).await?;
                Ok(updated.id)
            }
            Err(err) => Err(err),
        },
    };
    let id = match saved {
        Ok(id) => id,
        Err(Error::Refused(code)) => return Ok(Outcome::Refused(code)),
        Err(err) => return Err(err),
    };
    for role in save.roles.iter().flatten() {
        add_role(write, id, role).await?;
    }
    Ok(Outcome::Written(id))
}

/// The comment functions' context: the request's time and `home_url()`.
fn comments_context(ctx: &Context) -> hyscribe_write::comments::Context<'_> {
    hyscribe_write::comments::Context {
        now: ctx.now,
        home: &ctx.home,
        empty_trash_days: ctx.constants.empty_trash_days,
    }
}

/// A comment's create from `wp_allow_comment()` on, or update from
/// `wp_update_comment()` on, then `handle_status_param()`.
async fn comment_saved(
    write: &mut Write<'_>,
    save: &CommentSave,
    plan: &Plan,
    ctx: &Context,
) -> Result<Outcome, Error> {
    let context = comments_context(ctx);
    let text = |field: &Option<Vec<u8>>| field.clone().unwrap_or_default();
    let id = if save.id == 0 {
        let now = hyscribe_render::date::format_unix("Y-m-d H:i:s", ctx.now, Zone::Utc);
        let (date, date_gmt) = match &save.date {
            Some((date, gmt)) => (Some(date.clone()), gmt.clone()),
            None => (None, now),
        };
        let comment = NewComment {
            post_id: save.post_id.unwrap_or_default(),
            parent: save.parent.unwrap_or_default(),
            user_id: save.user_id.unwrap_or_default(),
            author: text(&save.author),
            author_email: text(&save.author_email),
            author_url: text(&save.author_url),
            author_ip: save.author_ip.clone(),
            agent: text(&save.agent),
            content: text(&save.content),
            date,
            date_gmt: Some(date_gmt),
            ..NewComment::default()
        };
        let commenter = Commenter {
            moderate_comments: save.moderate_comments,
            manage_options: save.skip_flood,
            unfiltered_html: plan.unfiltered_html,
        };
        match allow_and_insert_comment(write, context, comment, commenter).await? {
            Ok(id) => id,
            Err(Refused::Duplicate) => return Ok(Outcome::Refused("comment_duplicate")),
            Err(Refused::Flood) => return Ok(Outcome::Refused("comment_flood")),
        }
    } else {
        let edit = CommentEdit {
            id: save.id,
            post_id: save.post_id,
            author: save.author.clone(),
            author_email: save.author_email.clone(),
            author_url: save.author_url.clone(),
            author_ip: Some(save.author_ip.clone()),
            date: save.date.as_ref().map(|(date, _)| date.clone()),
            content: save.content.clone(),
            agent: save.agent.clone(),
            parent: save.parent,
            user_id: save.user_id,
            ..CommentEdit::default()
        };
        if update_comment(write, context, &edit, save.kses)
            .await?
            .is_none()
        {
            return Ok(Outcome::Refused("rest_comment_failed_edit"));
        }
        save.id
    };
    if let Some(status) = &save.status {
        status_changed(write, context, id, status).await?;
    }
    Ok(Outcome::Written(id))
}

/// `handle_status_param()`: the comment given `status`, unless it has it.
async fn status_changed(
    write: &mut Write<'_>,
    context: hyscribe_write::comments::Context<'_>,
    id: u64,
    status: &[u8],
) -> Result<(), Error> {
    // `wp_get_comment_status()`; the comment was just written.
    let approved = write.comment(id).await?.unwrap_or_default().approved;
    let old: &[u8] = match approved.as_str() {
        "1" => b"approved",
        "0" => b"unapproved",
        other => other.as_bytes(),
    };
    if old == status {
        return Ok(());
    }
    let changed = match status {
        b"approved" | b"approve" | b"1" => set_comment_status(write, id, "approve").await,
        b"hold" | b"0" => set_comment_status(write, id, "hold").await,
        b"spam" => spam_comment(write, context, id).await,
        b"trash" => trash_comment(write, context, id).await,
        b"unspam" | b"untrash" => restore_comment(write, id).await,
        _ => Ok(false),
    };
    changed.map(|_| ()).map_err(Error::Db)
}

/// The taxonomy named `name`, of those whose terms are written.
fn taxonomy_of(name: &str) -> Taxonomy {
    match name {
        "category" => Taxonomy::Category,
        _ => Taxonomy::PostTag,
    }
}

/// A term function's result: its database and unported errors stop the
/// write, the others are what PHP answers.
fn term_kept<T>(result: Result<T, TermError>) -> Result<Result<T, Outcome>, Error> {
    let refused = match result {
        Ok(value) => return Ok(Ok(value)),
        Err(TermError::EmptyName) => Outcome::Refused("empty_term_name"),
        Err(TermError::InvalidName) => Outcome::Refused("invalid_term_name"),
        Err(TermError::MissingParent) => Outcome::Refused("missing_parent"),
        Err(TermError::InvalidTerm) => Outcome::Refused("invalid_term"),
        Err(TermError::Exists(id)) => Outcome::Exists(id),
        Err(TermError::DuplicateSlug(slug)) => Outcome::Duplicate(slug),
        Err(err) => return Err(err.into()),
    };
    Ok(Err(refused))
}

/// `create_item()` or `update_item()` of a term from `wp_insert_term()` or
/// `wp_update_term()` on, its texts slashed as `wp_slash()` gives them.
async fn term_saved(write: &mut Write<'_>, save: &TermSave) -> Result<Outcome, Error> {
    let slashed = |text: &Option<Vec<u8>>| text.as_deref().map(addslashes);
    let (name, slug, description) = (
        slashed(&save.name),
        slashed(&save.slug),
        slashed(&save.description),
    );
    let args = TermArgs {
        name: name.as_deref(),
        slug: slug.as_deref(),
        description: description.as_deref(),
        parent: save.parent.map(|parent| parent as i64),
        ..TermArgs::default()
    };
    let taxonomy = taxonomy_of(save.taxonomy);
    let saved = match save.id {
        0 => wp_insert_term(write, taxonomy, args.name.unwrap_or_default(), &args).await,
        id => wp_update_term(write, taxonomy, id, &args).await,
    };
    Ok(match term_kept(saved)? {
        Ok(ids) => Outcome::Written(ids.term_id),
        Err(refused) => refused,
    })
}

/// The settings' update: each option sanitized and updated, or deleted;
/// the changed rows' values, as PHP's options cache keeps them.
async fn settings_saved(write: &mut Write<'_>, save: &SettingsSave) -> Result<Outcome, Error> {
    let mut cached = Vec::new();
    for Setting {
        option,
        value,
        default,
    } in &save.options
    {
        let Some(value) = value else {
            delete_option(write, option).await?;
            continue;
        };
        let missing = get_option(write, option).await? == Value::Bool(false);
        let given = value.clone();
        let mut value = sanitize_option(write, option, value.clone()).await?.value;
        // `sanitize_option()` falls back on `get_option()`, which reads the
        // default for an option not stored.
        if let Some(default) = default.as_ref().filter(|_| missing)
            && value == Value::Bool(false)
            && given != Value::Bool(false)
        {
            value = default.clone();
        }
        let changed = match default {
            Some(default) if missing && *default != value => {
                add_option(write, option, &value, None).await?
            }
            Some(_) if missing => false,
            _ => update_option(write, option, &value, None).await?,
        };
        if changed {
            cached.push((option.clone(), value));
        }
    }
    Ok(Outcome::Cached(cached))
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
        Err(Error::InvalidText(last_error)) => Ok(Outcome::InvalidText(last_error)),
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
