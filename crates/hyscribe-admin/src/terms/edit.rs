// SPDX-License-Identifier: AGPL-3.0-or-later

//! `term.php` and `edit-tag-form.php`: the edit screen of a category or tag.

use hyscribe_kses::{esc_attr, esc_html, esc_url, esc_url_raw};
use hyscribe_phpser::{Value, absint, add_query_arg, remove_query_arg, urldecode};

use super::{CATEGORY, Taxonomy, TermsSite, term_message};
use crate::admin_bar::BarSite;
use crate::list_table::{fill, wp_nonce_field};
use crate::menu::Screen;
use crate::page::{Page, Request, admin_page, query_strings, wp_admin_notice};
use crate::posts::{Answer, term_options};
use crate::profile::{ProfileSite, wp_get_referer};
use crate::text;

/// What the edit screen asks of the site: the lists' and redirects'.
pub trait TermEditSite: TermsSite + ProfileSite {}

impl<T: TermsSite + ProfileSite> TermEditSite for T {}

/// The edit screen's arguments.
pub struct TermEditArgs {
    /// `$_REQUEST['taxonomy']`.
    pub taxonomy: Taxonomy,
    /// `$_REQUEST['tag_ID']`, if given.
    pub tag_id: Option<String>,
    /// `$_REQUEST['wp_http_referer']`, the list to go back to, if given.
    pub back: Option<String>,
    /// `$_REQUEST['message']`, the notice of a save, if given.
    pub message: Option<String>,
    /// `$_REQUEST['error']` is set: the save failed.
    pub error: bool,
    /// The `Referer` header, if sent.
    pub referer: Option<String>,
}

impl TermEditArgs {
    /// The arguments of the query string `query` and the `Referer` sent;
    /// `None` without a taxonomy listed or with an argument not ported.
    pub fn parse(query: &str, referer: Option<String>) -> Option<TermEditArgs> {
        let (mut taxonomy, mut tag_id, mut back, mut message) = (None, None, None, None);
        let mut error = false;
        for (key, value) in query_strings(query)? {
            match key.as_str() {
                "taxonomy" => taxonomy = Some(Taxonomy::named(value.as_bytes())?),
                "tag_ID" => tag_id = Some(value),
                "post_type" if value == "post" => {}
                "wp_http_referer" => back = Some(value),
                "message" => message = Some(value),
                "error" => error = true,
                _ => return None,
            }
        }
        Some(TermEditArgs {
            taxonomy: taxonomy?,
            tag_id,
            back,
            message,
            error,
            referer,
        })
    }
}

const PAGE: &str = include_str!("edit.html");

/// The parent field of a category: every other category but those under it.
fn parent_field(site: &dyn TermEditSite, id: u64, parent: u64) -> String {
    let mut terms = site.terms(CATEGORY.name);
    // `exclude_tree`: the term and every term under it.
    let mut excluded = vec![id];
    while let Some(at) = terms
        .iter()
        .position(|t| excluded.contains(&t.parent) || t.id == id)
    {
        excluded.push(terms.remove(at).id);
    }
    let options = if terms.is_empty() {
        "\t<option value='-1' selected='selected'>None</option>\n".to_string()
    } else {
        let none = "\t<option value='-1'>None</option>\n";
        format!("{none}{}", term_options(&terms, &parent.to_string()))
    };
    format!(
        "\t\t<tr class=\"form-field term-parent-wrap\">\n\t\t\t<th scope=\"row\"><label for=\"parent\">Parent Category</label></th>\n\t\t\t<td>\n\t\t\t\t<select  name='parent' id='parent' class='postform' aria-describedby=\"parent-description\">\n{options}</select>\n\t\t\t\t\t\t\t\t\t<p class=\"description\" id=\"parent-description\">Categories, unlike tags, can have a hierarchy. You might have a Jazz category, and under that have children categories for Bebop and Big Band. Totally optional.</p>\n\t\t\t\t\t\t\t</td>\n\t\t</tr>\n"
    )
}

/// The notice of `args`' message, with a link back to the list it came from.
fn notice(site: &dyn TermEditSite, args: &TermEditArgs) -> String {
    let taxonomy = args.taxonomy;
    let Some(message) = term_message(taxonomy, args.message.as_deref()) else {
        return String::new();
    };
    let mut html = format!("<p><strong>{message}</strong></p>");
    let back = esc_url_raw(args.back.as_deref().unwrap_or_default().as_bytes());
    let back = remove_query_arg(&[b"action", b"message", b"tag_ID"], &back);
    if !back.is_empty() {
        let fallback = BarSite::admin_url(site, &format!("term.php?taxonomy={}", taxonomy.name));
        let valid = site.validate_redirect(&esc_url_raw(&back), &fallback);
        html.push_str(&format!(
            "<p><a href=\"{}\">{}</a></p>",
            text(&esc_url(&valid)),
            esc_html(format!("&larr; Go to {}", taxonomy.plural).as_bytes())
        ));
    }
    let kind = if args.error { "error" } else { "success" };
    wp_admin_notice(&html, "message", &format!("notice-{kind}"), false, "")
}

/// `term.php` for `args`: the form to edit a term, refused to a user who
/// may not edit it.
pub fn term_edit(site: &dyn TermEditSite, args: &TermEditArgs, request: &Request) -> Answer {
    let taxonomy = args.taxonomy;
    let given = args.tag_id.as_deref().unwrap_or_default();
    if Value::String(given.into()).is_empty() {
        let list = BarSite::admin_url(site, "edit-tags.php");
        let to = add_query_arg(
            &[(b"taxonomy", Some(taxonomy.name.as_bytes()))],
            list.as_bytes(),
        );
        return Answer::Redirect(text(&to));
    }
    let id = absint(&Value::String(given.into()));
    let terms = site.terms(taxonomy.name);
    let Some(term) = terms.iter().find(|term| term.id == id) else {
        return Answer::Died(
            "You attempted to edit an item that does not exist. Perhaps it was deleted?",
        );
    };
    if !site.can_for("edit_term", id) {
        return Answer::Denied(
            "<h1>You need a higher level of permission.</h1><p>Sorry, you are not allowed to edit this item.</p>",
        );
    }
    let parent = if taxonomy == CATEGORY {
        parent_field(site, id, term.parent)
    } else {
        String::new()
    };
    let delete = if site.can_for("delete_term", id) {
        let link = BarSite::admin_url(
            site,
            &format!(
                "edit-tags.php?action=delete&taxonomy={}&tag_ID={id}&_wpnonce={}",
                taxonomy.name,
                site.nonce(&format!("delete-tag_{id}"))
            ),
        );
        format!(
            "\t\t\t<span id=\"delete-link\">\n\t\t\t<a class=\"delete\" href=\"{}\">Delete</a>\n\t\t</span>\n",
            text(&esc_url(link.as_bytes()))
        )
    } else {
        String::new()
    };
    let original = wp_get_referer(site, args.referer.as_deref(), request.uri);
    let title = format!("Edit {}", taxonomy.singular);
    let content = fill(
        PAGE,
        &[
            ("title", title.clone()),
            ("notice", notice(site, args)),
            ("id", id.to_string()),
            ("taxonomy", taxonomy.name.into()),
            (
                "original",
                esc_attr(original.unwrap_or_default().as_bytes()),
            ),
            (
                "nonce_field",
                wp_nonce_field(site, &format!("update-tag_{id}"), "_wpnonce", request.uri),
            ),
            ("name", esc_attr(&term.name)),
            ("slug", esc_attr(&urldecode(&term.slug))),
            ("parent", parent),
            ("description", esc_html(&term.description)),
            ("delete", delete),
            (
                "focus",
                if request.browser.is_mobile() {
                    String::new()
                } else {
                    "<script>\ntry{document.forms.edittag.name.focus();}catch(e){}\n</script>\n\t"
                        .into()
                },
            ),
        ],
    );
    let screen_id = format!("edit-{}", taxonomy.name);
    let (view, link) = (
        format!("View {}", taxonomy.singular),
        site.term_link(term, taxonomy.name),
    );
    let submenu = format!("edit-tags.php?taxonomy={}", taxonomy.name);
    let page = Page {
        screen: Screen {
            pagenow: "term.php",
            typenow: "post",
            taxonomy: taxonomy.name,
            parent_file: Some("edit.php"),
            submenu_file: Some(&submenu),
        },
        id: &screen_id,
        title: &title,
        help: &[],
        sidebar: "",
        screen_options: "",
        styles: &[],
        scripts: &["admin-tags"],
        late_scripts: &[],
        script_data: &[],
        script_after: &[],
        late_styles: &[],
        admin_head: "",
        admin_footer: "",
        footer_scripts: "",
        content: &content,
        canonical: request.canonical,
        origin: request.origin,
        view: Some((&view, &link)),
    };
    Answer::from_page(admin_page(site, &page, request.browser))
}

#[cfg(test)]
mod tests;
