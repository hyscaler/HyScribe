// SPDX-License-Identifier: AGPL-3.0-or-later

//! An extension's admin page (SDK 0.4): `admin.php` asked with `page`,
//! under the menu file PHP links it from, its body what the page's
//! callback printed where `do_action( $page_hook )` runs.

use hyscribe_kses::esc_attr;

use crate::list_table::wp_nonce_field;
use crate::menu::Screen;
use crate::page::{Page, PageSite, Request, admin_page};

/// The page asked as `page` under `pagenow`, whose hook is `hook`: `title`
/// its page title, `printed` what its callback printed; `None` when
/// `user_can_access_admin_page()` turns the user away.
pub fn extension_page(
    site: &dyn PageSite,
    (pagenow, page, hook): (&str, &str, &str),
    (title, printed): (&str, &[u8]),
    request: &Request,
) -> Option<String> {
    let content = String::from_utf8_lossy(printed);
    let page = Page {
        screen: Screen {
            pagenow,
            typenow: "",
            taxonomy: "",
            parent_file: None,
            submenu_file: None,
            plugin_page: Some(page),
        },
        id: hook,
        title,
        help: &[],
        sidebar: "",
        screen_options: "",
        styles: &[],
        scripts: &[],
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
        view: None,
    };
    admin_page(site, &page, request.browser)
}

/// `settings_fields( $group )` on the page `request_uri` asked for.
pub fn settings_fields(site: &dyn PageSite, group: &str, request_uri: &str) -> String {
    format!(
        "<input type='hidden' name='option_page' value='{}' /><input type=\"hidden\" name=\"action\" value=\"update\" />{}",
        esc_attr(group.as_bytes()),
        wp_nonce_field(site, &format!("{group}-options"), "_wpnonce", request_uri)
    )
}

#[cfg(test)]
mod tests;
