// SPDX-License-Identifier: AGPL-3.0-or-later

//! The login/out block, `render_block_core_loginout()`, as a visitor sees
//! it: logged out, so `wp_loginout()`'s link or `wp_login_form()`. Both
//! point at `wp-login.php`, which HyScribe redirects to `/hs-login`.

use hyscribe_kses::esc_url;
use hyscribe_phpser::{add_query_arg, urlencode};

use super::{Block, Context};
use crate::permalinks::home_path;

/// `render_block_core_loginout()` for a visitor.
pub(super) fn loginout(context: &Context<'_>, block: &Block<'_>) -> Vec<u8> {
    let set = |name| block.attribute(name).is_some_and(|v| !v.is_empty());
    let current = current_url(context);
    let login = context.site_url(b"/wp-login.php");
    let (class, contents) = if set("displayLoginAsForm") {
        ("logged-out has-login-form", login_form(&login, &current))
    } else {
        let redirect = if set("redirectToCurrent") {
            add_query_arg(
                &[(&b"redirect_to"[..], Some(&urlencode(&current)[..]))],
                &login,
            )
        } else {
            login
        };
        let href = String::from_utf8_lossy(&esc_url(&redirect)).into_owned();
        ("logged-out", format!("<a href=\"{href}\">Log in</a>"))
    };
    format!(
        "<div {}>{contents}</div>",
        block.wrapper(context, class.as_bytes())
    )
    .into_bytes()
}

/// `wp_login_form()` with its defaults, the submit button given the block
/// theme's button classes.
fn login_form(login: &[u8], redirect: &[u8]) -> String {
    let action = String::from_utf8_lossy(&esc_url(login)).into_owned();
    let redirect = String::from_utf8_lossy(&esc_url(redirect)).into_owned();
    format!(
        "<form name=\"loginform\" id=\"loginform\" action=\"{action}\" method=\"post\">\
         <p class=\"login-username\">\n\t\t\t\t<label for=\"user_login\">Username or Email Address</label>\n\t\t\t\t\
         <input type=\"text\" name=\"log\" id=\"user_login\" autocomplete=\"username\" class=\"input\" value=\"\" size=\"20\" />\n\t\t\t</p>\
         <p class=\"login-password\">\n\t\t\t\t<label for=\"user_pass\">Password</label>\n\t\t\t\t\
         <input type=\"password\" name=\"pwd\" id=\"user_pass\" autocomplete=\"current-password\" spellcheck=\"false\" class=\"input\" value=\"\" size=\"20\" />\n\t\t\t</p>\
         <p class=\"login-remember\"><label><input name=\"rememberme\" type=\"checkbox\" id=\"rememberme\" value=\"forever\" /> Remember Me</label></p>\
         <p class=\"login-submit\">\n\t\t\t\t\
         <input type=\"submit\" name=\"wp-submit\" id=\"wp-submit\" class=\"button button-primary wp-block-button__link wp-element-button\" value=\"Log In\" />\n\t\t\t\t\
         <input type=\"hidden\" name=\"redirect_to\" value=\"{redirect}\" />\n\t\t\t</p></form>"
    )
}

/// `( is_ssl() ? 'https://' : 'http://' ) . $_SERVER['HTTP_HOST'] .
/// $_SERVER['REQUEST_URI']`, with the scheme and host of the home URL in
/// place of the `Host` header, which the client controls.
fn current_url(context: &Context<'_>) -> Vec<u8> {
    let home = String::from_utf8_lossy(&context.site.home_url()).into_owned();
    let origin = home.strip_suffix(home_path(&home)).unwrap_or(&home);
    [origin.as_bytes(), &context.request.uri].concat()
}

#[cfg(test)]
mod tests;
