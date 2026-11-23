// SPDX-License-Identifier: AGPL-3.0-or-later

//! `hyscribe plugin install <name>[@<version>]` and `hyscribe plugin update`
//! (ADR 0024): the hosted registry at `EXTENSION_REGISTRY`, fetched from
//! whatever `HTTP_OUTBOUND` says, as the operator asked for it.

use std::collections::HashMap;
use std::path::Path;

use hyscribe_ext::manifest;
use hyscribe_fetch::Client;
use hyscribe_registry::install::{self, Allow, Site, Update};
use hyscribe_registry::remote::Remote;

use crate::{Reply, configured, failed, signed};

/// What the command asks of the registry.
pub(crate) enum Asked<'a> {
    /// `install <name>[@<version>]`.
    Install(&'a str),
    /// `update`.
    Update(Update<'a>),
}

/// Whether `install`'s argument names a file rather than a plugin.
pub(crate) fn is_file(target: &str) -> bool {
    target.ends_with(".hyx") || target.contains('/')
}

/// Runs `asked` against the registry through `client`.
pub(crate) fn command(
    home: &Path,
    env: &HashMap<String, String>,
    client: &Client,
    asked: Asked<'_>,
    allow: Allow,
) -> Result<Reply, String> {
    let base = configured(home, env)?.extension_registry.ok_or(
        "EXTENSION_REGISTRY is not set: the hosted registry has no address yet, so set it to a registry's https: URL",
    )?;
    let remote = Remote::new(client, &base)?;
    let roots = signed::roots();
    let site = Site {
        home,
        roots: &roots,
        now: signed::now(),
    };
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .map_err(failed("cannot start the runtime".into()));
    runtime.and_then(|runtime| {
        runtime.block_on(async {
            match asked {
                Asked::Install(target) => {
                    let named = name(target)?;
                    let done = install::install(&remote, &site, named, allow).await;
                    done.map(Reply::text)
                }
                Asked::Update(which) => {
                    let (text, failed) = install::update(&remote, &site, which, allow).await?;
                    Ok(Reply {
                        text,
                        code: u8::from(failed),
                    })
                }
            }
        })
    })
}

/// `<name>[@<version>]`, a slug and a version.
fn name(target: &str) -> Result<(&str, Option<&str>), String> {
    let (name, version) = match target.split_once('@') {
        Some((name, version)) => (name, Some(version)),
        None => (target, None),
    };
    if manifest::slug(name) && version.is_none_or(manifest::version) {
        Ok((name, version))
    } else {
        Err(format!(
            "{target:?} is neither a package file (ending in .hyx) nor a plugin's name: lowercase letters, digits, - and _, then @ and a version if one is asked for"
        ))
    }
}
