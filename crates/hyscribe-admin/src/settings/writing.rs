// SPDX-License-Identifier: AGPL-3.0-or-later

//! `options-writing.php`.

use hyscribe_kses::{esc_attr, esc_html, esc_textarea};
use hyscribe_phpser::{Value, is_empty_string};
use hyscribe_render::post::POST_FORMATS;

use super::{
    Form, SettingsSite, checks, form_option, option, settings_fields, settings_page, text,
};
use crate::list_table::{fill, selected};
use crate::page::{HelpTab, Request};
use crate::posts::{Answer, term_options};

const CONTENT: &str = include_str!("writing.html");

const HELP: [HelpTab; 3] = [
    HelpTab {
        id: "overview",
        title: "Overview",
        content: "<p>You can submit content in several different ways; this screen holds the settings for all of them. The top section controls the editor within the dashboard, while the rest control external publishing methods. For more information on any of these methods, use the documentation links.</p><p>You must click the Save Changes button at the bottom of the screen for new settings to take effect.</p>",
    },
    HelpTab {
        id: "options-postemail",
        title: "Post Via Email",
        content: "<p>Post via email settings allow you to send your HyScribe installation an email with the content of your post. You must set up a secret email account with POP3 access to use this, and any mail received at this address will be posted, so it&#8217;s a good idea to keep this address very secret.</p>",
    },
    HelpTab {
        id: "options-services",
        title: "Update Services",
        content: "<p>If desired, HyScribe will automatically alert various services of your new posts.</p>",
    },
];

const SERVICES_DOC: &str =
    "https://developer.wordpress.org/advanced-administration/wordpress/update-services/";

/// `wp_dropdown_categories()` of `name`, every term of `taxonomy` by name
/// as a tree, the one the option `name` holds chosen.
fn categories(site: &dyn SettingsSite, name: &str, taxonomy: &str) -> String {
    format!(
        "<select  name='{name}' id='{name}' class='postform'>\n{}</select>\n",
        term_options(&site.terms(taxonomy), &option(site, name))
    )
}

/// The Formatting options of sites installed before WordPress 4.3.
pub(super) fn formatting(site: &dyn SettingsSite) -> String {
    let old = site
        .option("initial_db_version")
        .is_none_or(|version| Value::String(version).to_float() < 32453.0);
    if !old {
        return String::new();
    }
    format!(
        "\t<tr>\n<th scope=\"row\">Formatting</th>\n<td><fieldset><legend class=\"screen-reader-text\"><span>Formatting</span></legend>\n<label for=\"use_smilies\">\n<input name=\"use_smilies\" type=\"checkbox\" id=\"use_smilies\" value=\"1\" {} />\n\tConvert emoticons like <code>:-)</code> and <code>:-P</code> to graphics on display</label><br />\n<label for=\"use_balanceTags\"><input name=\"use_balanceTags\" type=\"checkbox\" id=\"use_balanceTags\" value=\"1\" {} /> HyScribe should correct invalidly nested XHTML automatically</label>\n</fieldset></td>\n</tr>\n",
        checks(site, "use_smilies", "1"),
        checks(site, "use_balanceTags", "1")
    )
}

/// `options-writing.php`.
pub fn writing(site: &dyn SettingsSite, request: &Request) -> Answer {
    let format = option(site, "default_post_format");
    let formats: String = POST_FORMATS[1..]
        .iter()
        .map(|(slug, name)| {
            format!(
                "\t\t<option{} value=\"{}\">{}</option>\n",
                selected(format == *slug),
                esc_attr(slug.as_bytes()),
                esc_html(name.as_bytes())
            )
        })
        .collect();
    let link = if is_empty_string(option(site, "link_manager_enabled").as_bytes()) {
        String::new()
    } else {
        format!(
            "<tr>\n<th scope=\"row\"><label for=\"default_link_category\">Default Link Category</label></th>\n<td>\n\t{}</td>\n</tr>\n",
            categories(site, "default_link_category", "link_category")
        )
    };
    let services = if option(site, "blog_public") == "1" {
        format!(
            "\n\t<p><label for=\"ping_sites\">\n\t\tWhen you publish a new post, HyScribe automatically notifies the following site update services. For more about this, see the <a href=\"{SERVICES_DOC}\">Update Services</a> documentation article. Separate multiple service URLs with line breaks.\t</label></p>\n\n\t<textarea name=\"ping_sites\" id=\"ping_sites\" class=\"large-text code\" rows=\"3\">{}</textarea>\n\n\t",
            text(&esc_textarea(option(site, "ping_sites").as_bytes()))
        )
    } else {
        format!(
            "\n\t<p>\n\t\tHyScribe is not notifying any <a href=\"{SERVICES_DOC}\">Update Services</a> because of your site&#8217;s <a href=\"options-reading\">visibility settings</a>.\t</p>\n\n\t"
        )
    };
    let content = fill(
        CONTENT,
        &[
            ("fields", settings_fields(site, "writing", request.uri)),
            ("formatting", formatting(site)),
            ("category", categories(site, "default_category", "category")),
            ("formats", formats),
            ("link", link),
            ("key1", site.random_key()),
            ("key2", site.random_key()),
            ("key3", site.random_key()),
            ("mailserver_url", form_option(site, "mailserver_url")),
            ("mailserver_port", form_option(site, "mailserver_port")),
            ("mailserver_login", form_option(site, "mailserver_login")),
            ("mailserver_pass", form_option(site, "mailserver_pass")),
            (
                "email_category",
                categories(site, "default_email_category", "category"),
            ),
            ("services", services),
        ],
    );
    let form = Form {
        file: "options-writing.php",
        title: "Writing Settings",
        help: &HELP,
        content: &content,
        scripts: &["user-profile"],
        ..Form::default()
    };
    settings_page(site, &form, request)
}
