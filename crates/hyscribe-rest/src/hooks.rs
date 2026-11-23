// SPDX-License-Identifier: AGPL-3.0-or-later

//! WordPress's REST hooks at their points (ADR 0013), through the hook
//! points every subsystem shares.

pub(crate) use hyscribe_model::hooks::{apply_filters, do_action};
