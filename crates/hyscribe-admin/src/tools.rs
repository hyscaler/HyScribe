// SPDX-License-Identifier: AGPL-3.0-or-later

//! The Tools screen, `tools.php`.

use crate::admin_bar::Browser;
use crate::menu::{Screen, link};
use crate::page::{HelpTab, Page, PageSite, admin_page};

const HELP: &str = "<p>Categories have hierarchy, meaning that you can nest sub-categories. Tags do not have hierarchy and cannot be nested. Sometimes people start out using one on their posts, then later realize that the other would work better for their content.</p><p>The Categories and Tags Converter link on this screen will take you to the Import screen, where that Converter is one of the plugins you can install. Once that plugin is installed, the Activate Plugin &amp; Run Importer link will take you to a screen where you can choose to convert tags into categories or vice versa.</p>";

const SIDEBAR: &str = "<p><strong>For more information:</strong></p><p><a href=\"https://wordpress.org/documentation/article/tools-screen/\">Documentation on Tools</a></p><p><a href=\"https://wordpress.org/support/forums/\">Support forums</a></p>";

/// The Tools screen at `canonical`, requested from `origin`, or `None`
/// when the user may not see it.
pub fn tools(
    site: &dyn PageSite,
    canonical: &str,
    origin: &str,
    browser: &Browser,
) -> Option<String> {
    let mut content = String::from("<div class=\"wrap\">\n<h1>Tools</h1>\n");
    // Categories and tags both need `manage_categories`.
    if site.can("import") && site.can("manage_categories") {
        content.push_str(&format!(
            "\t\t<div class=\"card\">\n\t\t\t<h2 class=\"title\">Categories and Tags Converter</h2>\n\t\t\t<p>\n\t\t\tIf you want to convert your categories to tags (or vice versa), use the <a href=\"{}\">Categories and Tags Converter</a> available from the Import screen.\t\t\t</p>\n\t\t</div>\n\t\t",
            link("import.php")
        ));
    }
    content.push_str("</div>\n");
    let help = [HelpTab {
        id: "converter",
        title: "Categories and Tags Converter",
        content: HELP,
    }];
    let page = Page {
        screen: Screen {
            pagenow: "tools.php",
            typenow: "",
            parent_file: None,
            submenu_file: None,
        },
        id: "tools",
        title: "Tools",
        help: &help,
        sidebar: SIDEBAR,
        content: &content,
        canonical,
        origin,
    };
    admin_page(site, &page, browser)
}

#[cfg(test)]
mod tests;
