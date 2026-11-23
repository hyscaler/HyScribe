// SPDX-License-Identifier: AGPL-3.0-or-later

//! What a manifest grants (ADR 0022): nothing unless named. Every host call
//! checks its grant here before it touches anything.

use std::collections::BTreeSet;

/// The manifest's `[capabilities]`, `[hooks]`, `[meta]`, `[taxonomies]`,
/// `[rest]` and SDK 0.2's `[transients]`, `[roles]` and `[http]`,
/// once the administrator approved them.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Grants {
    /// `[hooks] actions`: actions it adds callbacks to, cron hooks among them.
    pub actions: BTreeSet<String>,
    /// `[hooks] filters`.
    pub filters: BTreeSet<String>,
    /// `[hooks] shortcodes`: the tags it adds.
    pub shortcodes: BTreeSet<String>,
    /// `[hooks] sensitive`: the security sensitive hooks it adds callbacks
    /// to, actions and filters alike.
    pub sensitive: BTreeSet<String>,
    /// Option names it reads and writes.
    pub options: BTreeSet<String>,
    /// Meta keys it reads and writes: a name, or a prefix ending in `*`.
    pub meta: BTreeSet<String>,
    /// Post types it registers or reads beyond the public ones.
    pub post_types: BTreeSet<String>,
    /// `[taxonomies] write`: the taxonomies it registers.
    pub taxonomies: BTreeSet<String>,
    /// `[taxonomies] read`: the taxonomies it reads beyond its own; SDK 0.1
    /// has no call that reads terms, so it grants nothing yet.
    pub read_taxonomies: BTreeSet<String>,
    /// Its REST namespaces, such as `contact-form-7/v1`.
    pub rest: BTreeSet<String>,
    /// `wp_mail()`.
    pub mail: bool,
    /// Folders under `uploads/` whose files it may attach to mail.
    pub filesystem: BTreeSet<String>,
    /// `[transients] names`: transients it reads and writes, a name or a
    /// prefix ending in `*`.
    pub transients: BTreeSet<String>,
    /// `[transients] site`: site transients, the same way.
    pub site_transients: BTreeSet<String>,
    /// `[roles] names`: the roles it adds and removes, never WordPress's.
    pub roles: BTreeSet<String>,
    /// `[roles] capabilities`: the capabilities it gives or takes from any
    /// role, its own included.
    pub capabilities: BTreeSet<String>,
    /// `[http] hosts`: the hosts it sends requests to, redirects included.
    pub hosts: BTreeSet<String>,
}

/// WordPress's own roles, which no extension removes or adds anew.
const CORE_ROLES: [&str; 5] = [
    "administrator",
    "editor",
    "author",
    "contributor",
    "subscriber",
];

/// The hooks that change who is logged in, what they may do, what is
/// filtered, or that see passwords, keys or mail: a grant of their own.
const SENSITIVE: [&str; 19] = [
    "after_password_reset",
    "authenticate",
    "determine_current_user",
    "map_meta_cap",
    "password_reset",
    "pre_wp_mail",
    "rest_authentication_errors",
    "retrieve_password_key",
    "upload_mimes",
    "user_has_cap",
    "wp_kses_allowed_html",
    "wp_mail",
    "wp_mail_charset",
    "wp_mail_content_type",
    "wp_mail_failed",
    "wp_mail_from",
    "wp_mail_from_name",
    "wp_mail_succeeded",
    "wp_set_password",
];

/// The option filters that are sensitive for an option outside the
/// extension's own options grant.
const OPTION_HOOKS: [&str; 3] = ["pre_option_", "option_", "pre_update_option_"];

/// Hooks no extension attaches to: the stand-in for `phpmailer_init`
/// replaces the transport (ADR 0022 refuses its object).
const REFUSED: [&str; 1] = ["phpmailer_init"];

impl Grants {
    /// Whether `hook` needs the sensitive grant for this extension.
    pub fn is_sensitive(&self, hook: &str) -> bool {
        SENSITIVE.contains(&hook)
            || OPTION_HOOKS.iter().any(|prefix| {
                hook.strip_prefix(prefix)
                    .is_some_and(|option| !self.options.contains(option))
            })
    }

    /// What it asks for, as the Plugins screen lists it before activation:
    /// each grant that names anything, with its names; the sensitive hooks
    /// apart.
    pub fn listed(&self) -> Vec<(&'static str, Vec<String>)> {
        let mail = self.mail.then(|| "wp_mail()".to_string());
        let lists = [
            ("Options", self.options.iter().cloned().collect()),
            ("Post types", self.post_types.iter().cloned().collect()),
            ("Meta keys", self.meta.iter().cloned().collect()),
            (
                "Taxonomies it registers",
                self.taxonomies.iter().cloned().collect(),
            ),
            (
                "Taxonomies it reads",
                self.read_taxonomies.iter().cloned().collect(),
            ),
            ("REST namespaces", self.rest.iter().cloned().collect()),
            ("Actions", self.actions.iter().cloned().collect()),
            ("Filters", self.filters.iter().cloned().collect()),
            ("Shortcodes", self.shortcodes.iter().cloned().collect()),
            ("Mail", mail.into_iter().collect()),
            (
                "Mail attachments from",
                self.filesystem.iter().cloned().collect(),
            ),
            ("Transients", self.transients.iter().cloned().collect()),
            (
                "Site transients",
                self.site_transients.iter().cloned().collect(),
            ),
            ("Roles it adds", self.roles.iter().cloned().collect()),
            (
                "Capabilities it gives roles",
                self.capabilities.iter().cloned().collect(),
            ),
            ("Sends requests to", self.hosts.iter().cloned().collect()),
        ];
        lists
            .into_iter()
            .filter(|(_, names): &(_, Vec<String>)| !names.is_empty())
            .collect()
    }

    /// The checks the manifest passes before anything loads: a sensitive
    /// hook named only as an action or filter, a meta or transient grant of
    /// every name, a role grant of WordPress's own roles, or a hook no
    /// extension may have refuses the package.
    pub fn check(&self) -> Result<(), String> {
        let named = self.actions.iter().chain(&self.filters);
        if let Some(hook) = named.clone().find(|hook| self.is_sensitive(hook)) {
            return Err(format!(
                "{hook} is a sensitive hook: name it under [hooks] sensitive"
            ));
        }
        if let Some(hook) = named
            .chain(&self.sensitive)
            .find(|hook| REFUSED.contains(&hook.as_str()))
        {
            return Err(format!("{hook} is not a hook extensions attach to"));
        }
        if self.meta.contains("*") {
            return Err("a meta grant names its keys or their prefix".into());
        }
        if self.transients.contains("*") || self.site_transients.contains("*") {
            return Err("a transient grant names its transients or their prefix".into());
        }
        if let Some(role) = self
            .roles
            .iter()
            .find(|role| CORE_ROLES.contains(&role.as_str()))
        {
            return Err(format!(
                "{role} is WordPress's own role: [roles] names its own"
            ));
        }
        Ok(())
    }

    /// An action or filter callback on `hook`.
    pub(crate) fn hook(&self, hook: &str, action: bool) -> Result<(), String> {
        let granted = if self.is_sensitive(hook) {
            &self.sensitive
        } else if action {
            &self.actions
        } else {
            &self.filters
        };
        allow(granted.contains(hook), || {
            format!("{hook} is not in the manifest")
        })
    }

    /// A cron event on `hook`, which fires it as an action.
    pub(crate) fn cron(&self, hook: &str) -> Result<(), String> {
        self.hook(hook, true)
    }

    /// The option `name`.
    pub(crate) fn option(&self, name: &str) -> Result<(), String> {
        allow(self.options.contains(name), || {
            format!("option {name} is not in the manifest")
        })
    }

    /// The meta key `key`, by name or by a prefix ending in `*`.
    pub(crate) fn meta(&self, key: &str) -> Result<(), String> {
        allow(named(&self.meta, key), || {
            format!("meta key {key} is not in the manifest")
        })
    }

    /// The transient `name`, or the site transient, by name or prefix.
    pub(crate) fn transient(&self, name: &str, site: bool) -> Result<(), String> {
        let (granted, what) = if site {
            (&self.site_transients, "site transient")
        } else {
            (&self.transients, "transient")
        };
        allow(named(granted, name), || {
            format!("{what} {name} is not in the manifest")
        })
    }

    /// The role `name`, to add or remove.
    pub(crate) fn role(&self, name: &str) -> Result<(), String> {
        allow(self.roles.contains(name), || {
            format!("role {name} is not in the manifest")
        })
    }

    /// The capability `name`, to give or take from a role.
    pub(crate) fn capability(&self, name: &str) -> Result<(), String> {
        allow(self.capabilities.contains(name), || {
            format!("capability {name} is not in the manifest")
        })
    }

    /// A request to, or a redirect through, `host`, its case ignored.
    pub(crate) fn host(&self, host: &str) -> bool {
        self.hosts
            .iter()
            .any(|granted| granted.eq_ignore_ascii_case(host))
    }

    /// The post type `name`, to register or read.
    pub(crate) fn post_type(&self, name: &str) -> Result<(), String> {
        allow(self.post_types.contains(name), || {
            format!("post type {name} is not in the manifest")
        })
    }

    /// The taxonomy `name`, to register.
    pub(crate) fn taxonomy(&self, name: &str) -> Result<(), String> {
        allow(self.taxonomies.contains(name), || {
            format!("taxonomy {name} is not in the manifest")
        })
    }

    /// A REST route in `namespace`.
    pub(crate) fn rest(&self, namespace: &str) -> Result<(), String> {
        allow(self.rest.contains(namespace), || {
            format!("REST namespace {namespace} is not in the manifest")
        })
    }

    /// A shortcode `tag`.
    pub(crate) fn shortcode(&self, tag: &str) -> Result<(), String> {
        allow(self.shortcodes.contains(tag), || {
            format!("shortcode {tag} is not in the manifest")
        })
    }

    /// `wp_mail()` with `attachments`, each a path under a granted folder.
    pub(crate) fn mail(&self, attachments: &[String]) -> Result<(), String> {
        allow(self.mail, || "mail is not in the manifest".into())?;
        let outside = attachments.iter().find(|path| {
            !relative(path)
                || !self.filesystem.iter().any(|folder| {
                    path.strip_prefix(folder.as_str())
                        .is_some_and(|rest| rest.starts_with('/'))
                })
        });
        outside.map_or(Ok(()), |path| {
            Err(format!("{path} is not in a folder the manifest grants"))
        })
    }
}

/// A path inside a folder: relative, no `..` or `.` step, no empty step,
/// no backslash, colon or NUL.
pub(crate) fn relative(path: &str) -> bool {
    !path.contains(['\\', ':', '\0'])
        && path.split('/').all(|step| !matches!(step, "" | "." | ".."))
}

/// Whether `grants` names `name`, or a prefix of it ending in `*`.
fn named(grants: &BTreeSet<String>, name: &str) -> bool {
    grants.iter().any(|grant| match grant.strip_suffix('*') {
        Some(prefix) => name.starts_with(prefix),
        None => grant == name,
    })
}

fn allow(granted: bool, why: impl FnOnce() -> String) -> Result<(), String> {
    if granted { Ok(()) } else { Err(why()) }
}

#[cfg(test)]
mod tests;
