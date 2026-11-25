// SPDX-License-Identifier: AGPL-3.0-or-later

//! The template tags a classic theme's templates call (ADR 0023) for the
//! page being rendered, by WordPress's name: the main loop, the conditional
//! tags, and the tags that print or answer a value. `loop_start`,
//! `loop_end` and `loop_no_results` pass the query object and cross to no
//! extension yet.

use hyscribe_model::hooks::{apply_filters, do_action_printing};
use hyscribe_phpser::{Value, is_empty_string};

use crate::attachment::ImageOptions;
use crate::core_blocks::{custom_logo, logo};
use crate::media::Loading;
use crate::navigation::{NavMenu, NavMenuArgs, Printed, wp_nav_menu};
use crate::post::{body_classes, page_template_slug};
use crate::site::{Archive, Request, Site, Taxonomy};
use crate::theme::Theme;

mod tags;

/// A tag this host does not serve, or arguments it does not port.
#[derive(Debug, PartialEq, Eq)]
pub struct Unserved;

/// What the template tags read: the block theme whose context posts take,
/// the site, the page's request and `current_theme_supports()`.
pub struct Scope<'a> {
    /// The block theme loaded.
    pub theme: &'a Theme,
    /// The site's reads.
    pub site: &'a dyn Site,
    /// The page's request.
    pub request: &'a Request,
    /// `current_theme_supports()`.
    pub supports: &'a dyn Fn(&str) -> bool,
}

/// A classic theme's page: the main loop's place and what the page has
/// printed so far, which the template tags read.
#[derive(Clone)]
pub struct Classic {
    /// `get_template()` and `get_stylesheet()`.
    theme: (String, String),
    /// `$wp_query->current_post`, `None` for -1.
    current: Option<usize>,
    /// The global `$post`: the main query's post at this place.
    global: usize,
    /// Whether `setup_postdata()` has run, which makes `$authordata` the
    /// global post's author rather than the queried one.
    set_up: bool,
    in_the_loop: bool,
    loading: Loading,
    printed: Printed,
    /// Whether `_restore_wpautop_hook()` has put `wpautop()` back last.
    autop_restored: bool,
    /// The comments `comments_template()` set up.
    comments: Option<crate::core_blocks::Thread>,
}

impl Classic {
    /// The page of the theme `template` and `stylesheet`, before its loop.
    pub fn new((template, stylesheet): (&str, &str)) -> Classic {
        Classic {
            theme: (template.into(), stylesheet.into()),
            current: None,
            global: 0,
            set_up: false,
            in_the_loop: false,
            loading: Loading::default(),
            printed: Printed::default(),
            autop_restored: false,
            comments: None,
        }
    }

    /// `have_posts()`: past the last post the loop ends and rewinds.
    pub fn have_posts(&mut self, request: &Request) -> bool {
        let next = self.current.map_or(0, |current| current + 1);
        if next < request.posts.len() {
            return true;
        }
        self.current = None;
        self.in_the_loop = false;
        false
    }

    /// `the_post()`: the next post of the main query.
    pub fn the_post(&mut self) {
        self.in_the_loop = true;
        let current = self.current.map_or(0, |current| current + 1);
        self.current = Some(current);
        self.global = current;
        self.set_up = true;
    }

    /// `wp_reset_postdata()`: the global post is the main query's again,
    /// set up when there is one.
    pub fn reset_postdata(&mut self, request: &Request) {
        self.global = 0;
        self.set_up |= !request.posts.is_empty();
    }

    /// `rewind_posts()`.
    pub fn rewind_posts(&mut self) {
        self.current = None;
    }

    /// `in_the_loop()`.
    pub fn in_the_loop(&self) -> bool {
        self.in_the_loop
    }

    /// The conditional tag `name` with its arguments.
    pub fn is(
        &self,
        site: &dyn Site,
        request: &Request,
        name: &str,
        args: &[Value],
    ) -> Result<bool, Unserved> {
        let kind = request
            .posts
            .first()
            .filter(|_| request.singular)
            .map(|post| post.post_type.as_bytes());
        let term =
            |taxonomy| matches!(&request.archive, Some(Archive::Term(t, _)) if *t == taxonomy);
        let date = Dates::of(request);
        Ok(match name {
            "is_home" => request.home,
            "is_front_page" => request.front_page,
            "is_singular" => kind.is_some_and(|kind| named(args.first(), kind)),
            "is_single" => kind.is_some_and(|kind| kind != b"page"),
            "is_page" => kind == Some(b"page"),
            "is_attachment" => request.attachment,
            "is_archive" => request.archive.is_some(),
            "is_category" => term(Taxonomy::Category),
            "is_tag" => term(Taxonomy::Tag),
            "is_tax" => term(Taxonomy::Format),
            "is_date" => request.date,
            "is_year" => date.year,
            "is_month" => date.month,
            "is_day" => date.day,
            "is_time" => date.time,
            "is_search" => request.search.is_some(),
            "is_404" => request.not_found,
            "is_paged" => request.is_paged(),
            "is_preview" => request.preview,
            "is_feed" => request.feed,
            "is_embed" => request.embed,
            "is_privacy_policy" => {
                let option = site.option("wp_page_for_privacy_policy");
                let id = Value::String(option.unwrap_or_default()).to_int();
                id != 0 && u64::try_from(id).ok() == Some(request.queried_object_id)
            }
            "is_page_template" => kind.is_some() && page_template(site, request, args.first()),
            "in_the_loop" => self.in_the_loop,
            "has_custom_logo" => logo(site, &self.theme.1).is_some(),
            // On an author's archive, naming an author is not ported.
            "is_author" => match request.archive {
                Some(Archive::Author(..)) if args.first().is_some_and(|a| !a.is_empty()) => {
                    return Err(Unserved);
                }
                Some(Archive::Author(..)) => true,
                _ => false,
            },
            // The front end, in English.
            "is_rtl" => false,
            _ => return self.is_post(site, request, name, args),
        })
    }

    /// What the template tag `name` prints.
    pub fn print(
        &mut self,
        scope: &Scope<'_>,
        (name, args): (&str, &[Value]),
    ) -> Result<Vec<u8>, Unserved> {
        let (site, request) = (scope.site, scope.request);
        let supports = scope.supports;
        let theme = (self.theme.0.as_str(), self.theme.1.as_str());
        Ok(match name {
            "language_attributes" => language_attributes(site, args.first()),
            // `$css_class` is not ported.
            "body_class" if args.first().is_none_or(Value::is_empty) => {
                let classes = body_classes(site, request, theme, supports("responsive-embeds"));
                [&b"class=\""[..], &classes, b"\""].concat()
            }
            "wp_body_open" => do_action_printing("wp_body_open", &[], Vec::new()),
            // Its `loading` is given, so where it prints does not count.
            "the_custom_logo" => {
                let options = ImageOptions::default();
                custom_logo((site, request), theme.1, &self.loading, &options)
            }
            // The queried post's excerpt, the one field and post served.
            "get_post_field" => match args {
                [field, id] if field.to_php_string() == b"post_excerpt" => {
                    let post = request.posts.first().filter(|_| request.singular);
                    let post = post.filter(|post| id.to_int() == post.id as i64);
                    post.ok_or(Unserved)?.excerpt.clone()
                }
                _ => return Err(Unserved),
            },
            "wp_strip_all_tags" => match args {
                [text] => hyscribe_kses::wp_strip_all_tags(&text.to_php_string(), false),
                _ => return Err(Unserved),
            },
            _ => return self.print_tag(scope, name, args),
        })
    }

    /// What the template tag `name` answers.
    pub fn get(
        &mut self,
        scope: &Scope<'_>,
        name: &str,
        args: &[Value],
    ) -> Result<Value, Unserved> {
        let (site, request) = (scope.site, scope.request);
        match name {
            // With `echo` false, whatever the arguments say.
            "wp_nav_menu" => {
                let args = NavMenuArgs::parse(args.first().unwrap_or(&Value::Null));
                match wp_nav_menu(site, request, &args, &mut self.printed) {
                    NavMenu::Printed(markup) => Ok(Value::String(markup)),
                    NavMenu::False => Ok(Value::Bool(false)),
                    NavMenu::NotPorted => Err(Unserved),
                }
            }
            "get_queried_object_id" if args.is_empty() => {
                Ok(Value::Int(request.queried_object_id as i64))
            }
            _ => self.get_tag(scope, name, args),
        }
    }
}

/// Whether `kind` is among the post types `is_singular( $post_types )`
/// names: any when it names none.
fn named(post_types: Option<&Value>, kind: &[u8]) -> bool {
    let types = match post_types {
        Some(Value::Array(types)) => types.iter().map(|(_, t)| t.to_php_string()).collect(),
        Some(one) if !one.is_empty() => vec![one.to_php_string()],
        _ => return true,
    };
    types.is_empty() || types.iter().any(|t| t == kind)
}

/// `is_page_template( $template )` of the queried post shown on its own.
fn page_template(site: &dyn Site, request: &Request, template: Option<&Value>) -> bool {
    let slug = page_template_slug(site, request.queried_object_id);
    let default = |slug: &Option<Vec<u8>>, name: &[u8]| name == b"default" && slug.is_none();
    match template {
        Some(Value::Array(names)) => names.iter().any(|(_, name)| {
            let name = name.to_php_string();
            default(&slug, &name) || slug.as_deref() == Some(&name[..])
        }),
        Some(name) if !name.is_empty() => {
            let name = name.to_php_string();
            default(&slug, &name) || slug.as_deref() == Some(&name[..])
        }
        _ => slug.is_some(),
    }
}

/// Which date archive `WP_Query::parse_query()` makes of the query vars.
#[derive(Default)]
struct Dates {
    year: bool,
    month: bool,
    day: bool,
    time: bool,
}

impl Dates {
    fn of(request: &Request) -> Dates {
        let mut dates = Dates::default();
        if !request.date {
            return dates;
        }
        let var = |name| request.vars.get_str(name).map_or(0, Value::to_int);
        dates.time = var("hour") != 0 || var("minute") != 0 || var("second") != 0;
        let mut dated = dates.time;
        for (name, flag) in [
            ("day", &mut dates.day),
            ("monthnum", &mut dates.month),
            ("year", &mut dates.year),
        ] {
            if var(name) != 0 && !dated {
                *flag = true;
                dated = true;
            }
        }
        let m = request.vars.get_str("m").map(Value::to_php_string);
        let digits = m.map_or(0, |m| m.iter().filter(|b| b.is_ascii_digit()).count());
        match digits {
            0 => {}
            10.. => dates.time = true,
            8..=9 => dates.day = true,
            6..=7 => dates.month = true,
            _ => dates.year = true,
        }
        dates
    }
}

/// `get_language_attributes( $doctype )` in English, as `language_attributes()`
/// prints it.
fn language_attributes(site: &dyn Site, doctype: Option<&Value>) -> Vec<u8> {
    let doctype = doctype.map_or(b"html".to_vec(), Value::to_php_string);
    let html_type = site.option("html_type");
    let text_html = html_type.as_deref() == Some(b"text/html");
    let mut attributes: Vec<&[u8]> = Vec::new();
    if text_html || doctype == b"html" {
        attributes.push(b"lang=\"en-US\"");
    }
    let other = html_type.is_some_and(|t| !is_empty_string(&t) && t != b"text/html");
    if other || doctype == b"xhtml" {
        attributes.push(b"xml:lang=\"en-US\"");
    }
    let output = attributes.join(&b' ');
    apply_filters("language_attributes", output, &[&doctype])
}

#[cfg(test)]
mod tests;
