// SPDX-License-Identifier: AGPL-3.0-or-later

//! The users' writes (MVP 4.3): `create_item()`, `update_item()` and
//! `delete_item()` of `WP_REST_Users_Controller`, and of `me`. A request
//! whose checks pass gives the [`Plan`] `hyscribe-http` writes with
//! `wp_insert_user()` or `wp_update_user()` and `WP_User::add_role()`, or
//! `wp_delete_user()`. `meta`, and a new password of one's own, which logs
//! one in again, are not ported; the mails PHP sends are not sent yet.

use hyscribe_model::UserQuery;
use hyscribe_phpser::{Key, Value};
use hyscribe_render::formatting::sanitize_user_strict;

use super::{ITEM, ME, ROUTE, as_me, get_user, prepare};
use crate::Context;
use crate::posts::write::{Outcome, Plan, Write, Writing, invalid};
use crate::request::{Arg, Params, Sanitize, arg, force_arg, slug, typed};
use crate::response::{Response, RestError, text};
use crate::schema::{Checked, Refusal};

/// What a create or update saves, unslashed: `$prepared_user` where the
/// request gives it.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct UserSave {
    /// The user to update, 0 to create one.
    pub id: u64,
    /// `user_login`.
    pub login: Option<Vec<u8>>,
    /// `user_pass`, the plain password.
    pub pass: Option<Vec<u8>>,
    /// `user_email`.
    pub email: Option<Vec<u8>>,
    /// `display_name`.
    pub display_name: Option<Vec<u8>>,
    /// `first_name`.
    pub first_name: Option<Vec<u8>>,
    /// `last_name`.
    pub last_name: Option<Vec<u8>>,
    /// `nickname`.
    pub nickname: Option<Vec<u8>>,
    /// `user_nicename`.
    pub nicename: Option<Vec<u8>>,
    /// `description`.
    pub description: Option<Vec<u8>>,
    /// `user_url`.
    pub url: Option<Vec<u8>>,
    /// `locale`.
    pub locale: Option<Vec<u8>>,
    /// `role` false, then each of these roles added.
    pub roles: Option<Vec<Vec<u8>>>,
}

/// The ported write of the route `regex`'s endpoint `index`, if any.
pub(crate) fn writing(regex: &str, index: usize) -> Option<Writing> {
    let plan = match (regex, index) {
        (ROUTE, 1) => plan_create,
        (ITEM | ME, 1) => plan_update,
        (ITEM | ME, 2) => plan_delete,
        _ => return None,
    };
    Some(Writing::plain(plan, finish))
}

/// `get_endpoint_args_for_item_schema()` of a create (`required` names)
/// or an update; `meta` is not ported.
pub(super) fn save_args(create: bool) -> Vec<Arg> {
    let required = |schema: &str| match create {
        true => schema.replacen('{', r#"{"required":true,"#, 1),
        false => schema.to_string(),
    };
    let string = r#"{"type":"string"}"#;
    let field = |name| arg(name, string, true, Sanitize::TextField);
    vec![
        arg(
            "username",
            &required(string),
            true,
            Sanitize::Site(username),
        ),
        field("name"),
        field("first_name"),
        field("last_name"),
        arg(
            "email",
            &required(r#"{"type":"string","format":"email"}"#),
            true,
            Sanitize::Value,
        ),
        arg(
            "url",
            r#"{"type":"string","format":"uri"}"#,
            true,
            Sanitize::Value,
        ),
        arg("description", string, true, Sanitize::Value),
        arg(
            "locale",
            r#"{"type":"string","enum":["","en_US"]}"#,
            true,
            Sanitize::Value,
        ),
        field("nickname"),
        arg("slug", string, true, Sanitize::Site(slug)),
        arg(
            "roles",
            r#"{"type":"array","items":{"type":"string"}}"#,
            true,
            Sanitize::Value,
        ),
        arg(
            "password",
            &required(string),
            true,
            Sanitize::Site(password),
        ),
        arg("meta", r#"{"type":"object"}"#, false, Sanitize::NotPorted),
    ]
}

/// A user's delete: `force`, false unless given, and `reassign`, required.
pub(super) fn delete_args(me: bool) -> Vec<Arg> {
    let mut args = Vec::new();
    if !me {
        args.push(typed("id", r#"{"type":"integer"}"#));
    }
    args.push(force_arg());
    let reassign = r#"{"type":"integer","required":true}"#;
    args.push(arg(
        "reassign",
        reassign,
        false,
        Sanitize::Site(reassign_param),
    ));
    args
}

/// A parameter's own error, as an argument's sanitizer gives it.
fn refused<T>(code: &'static str, message: &str) -> Checked<T> {
    Err(Refusal(Some(RestError::status(code, message, 400))))
}

/// `check_username()`: `validate_username()`, with no login made illegal.
fn username(_: &Context, value: &Value) -> Checked<Value> {
    let username = value.to_php_string();
    // `validate_username()`, whose `empty()` refuses `0` too.
    if Value::String(username.clone()).is_empty() || sanitize_user_strict(&username) != username {
        let message = "This username is invalid because it uses illegal characters. Please enter a valid username.";
        return refused("rest_user_invalid_username", message);
    }
    Ok(text(username))
}

/// `check_user_password()`: not empty, without a backslash.
fn password(_: &Context, value: &Value) -> Checked<Value> {
    let password = value.to_php_string();
    if Value::String(password.clone()).is_empty() {
        return refused("rest_user_invalid_password", "Passwords cannot be empty.");
    }
    if password.contains(&b'\\') {
        let message = "Passwords cannot contain the \"\\\" character.";
        return refused("rest_user_invalid_password", message);
    }
    Ok(text(password))
}

/// `check_reassign()`: a number as given, false for none.
fn reassign_param(_: &Context, value: &Value) -> Checked<Value> {
    if value.is_numeric() {
        return Ok(value.clone());
    }
    if value.is_empty() || value.to_php_string() == b"false" {
        return Ok(Value::Bool(false));
    }
    refused("rest_invalid_param", "Invalid user parameter(s).")
}

/// The roles the request gives, as strings.
fn roles(params: &Params) -> Option<Vec<Vec<u8>>> {
    let roles = params.get("roles")?.as_array().unwrap_or_default();
    Some(roles.iter().map(|(_, role)| role.to_php_string()).collect())
}

/// `check_role_update()`: each role exists, and one's own roles keep
/// `edit_users`; every role is editable.
fn check_roles(context: &Context, user: Option<u64>, roles: &[Vec<u8>]) -> Result<(), RestError> {
    let prefix = context.site.table_prefix();
    let stored = context.option(&format!("{prefix}user_roles"));
    let defined = crate::unserialized(&stored.unwrap_or_default());
    for role in roles {
        let Some(found) = defined.get(&Key::from_php_string(role.clone())) else {
            let role = String::from_utf8_lossy(role);
            return Err(invalid(
                "rest_user_invalid_role",
                &format!("The role {role} does not exist."),
            ));
        };
        let edits = found
            .get_path(&["capabilities", "edit_users"])
            .is_some_and(|cap| !cap.is_empty());
        if user == Some(context.user) && !edits {
            let message = "Sorry, you are not allowed to give users that role.";
            return Err(context.forbidden("rest_user_invalid_role", message));
        }
    }
    Ok(())
}

/// `prepare_item_for_database()`.
fn prepared(params: &Params, id: u64) -> UserSave {
    let given = |name| params.get(name).map(Value::to_php_string);
    UserSave {
        id,
        login: given("username"),
        pass: given("password"),
        email: given("email"),
        display_name: given("name"),
        first_name: given("first_name"),
        last_name: given("last_name"),
        nickname: given("nickname"),
        nicename: given("slug"),
        description: given("description"),
        url: given("url"),
        locale: given("locale"),
        roles: roles(params),
    }
}

/// `create_item()` up to `wp_insert_user()`.
fn plan_create(context: &Context, params: &Params) -> Result<Plan, RestError> {
    if params.get("id").is_some_and(|id| !id.is_empty()) {
        return Err(invalid("rest_user_exists", "Cannot create existing user."));
    }
    let roles = roles(params).unwrap_or_default();
    check_roles(context, None, &roles)?;
    let save = prepared(params, 0);
    Ok(Plan::new(context, Write::User(Box::new(save)), Value::Null))
}

/// `update_item()` up to `wp_update_user()`, `me` as the current user.
fn plan_update(context: &Context, params: &Params) -> Result<Plan, RestError> {
    let params = &as_me_if(context, params);
    // The permission check found the user.
    let user = get_user(context, params).unwrap_or_default();
    if let Some(Value::String(email)) = params.get("email") {
        // `email_exists()`, whose query compares without case.
        let query = UserQuery {
            search: Some((
                vec!["user_email".into()],
                String::from_utf8_lossy(email).into_owned(),
            )),
            ..UserQuery::default()
        };
        let users = context.site.users(&query);
        let owner = users
            .iter()
            .find(|other| other.email.eq_ignore_ascii_case(email));
        if owner.is_some_and(|owner| owner.id != user.id) {
            return Err(invalid("rest_user_invalid_email", "Invalid email address."));
        }
    }
    let filled = |name| {
        params
            .get(name)
            .filter(|value| !value.is_empty())
            .map(Value::to_php_string)
    };
    if filled("username").is_some_and(|login| login != user.login) {
        return Err(invalid(
            "rest_user_invalid_argument",
            "Username is not editable.",
        ));
    }
    if let Some(slug) = filled("slug").filter(|slug| *slug != user.nicename) {
        let query = UserQuery {
            nicenames: vec![slug],
            ..UserQuery::default()
        };
        if !context.site.users(&query).is_empty() {
            return Err(invalid("rest_user_invalid_slug", "Invalid slug."));
        }
    }
    let roles = roles(params).filter(|roles| !roles.is_empty());
    check_roles(context, Some(user.id), &roles.unwrap_or_default())?;
    // `wp_update_user()` logs one in again with a new password.
    if user.id == context.user && params.get("password").is_some() {
        return Err(RestError::not_ported());
    }
    let save = prepared(params, user.id);
    Ok(Plan::new(context, Write::User(Box::new(save)), Value::Null))
}

/// The parameters of `me` as the current user's, others as they are.
fn as_me_if(context: &Context, params: &Params) -> Params {
    match params.route {
        ME => as_me(context, params),
        _ => params.clone(),
    }
}

/// `delete_item()` up to `wp_delete_user()`: deleted only with `force`.
fn plan_delete(context: &Context, params: &Params) -> Result<Plan, RestError> {
    let params = &as_me_if(context, params);
    // The permission check found the user.
    let user = get_user(context, params).unwrap_or_default();
    let reassign = match params.get("reassign") {
        Some(Value::Bool(false)) => None,
        reassign => Some(hyscribe_phpser::absint(reassign.unwrap_or(&Value::Null))),
    };
    if params.get("force").is_none_or(Value::is_empty) {
        let message = "Users do not support trashing. Set 'force=true' to delete.";
        return Err(RestError::status("rest_trash_not_supported", message, 501));
    }
    if let Some(heir) = reassign.filter(|&heir| heir != 0)
        && (heir == user.id || context.site.user(heir).is_none())
    {
        let message = "Invalid user ID for reassignment.";
        return Err(invalid("rest_user_invalid_reassign", message));
    }
    let mut edit = params.clone();
    edit.set("context", text("edit"));
    let previous = prepare(context, &edit, &user).data;
    Ok(Plan::new(
        context,
        Write::DeleteUser(user.id, reassign),
        previous,
    ))
}

/// The message of a `WP_Error` `wp_insert_user()` gives.
fn insert_error(code: &str) -> &'static str {
    match code {
        "empty_user_login" => "Cannot create a user with an empty login name.",
        "user_login_too_long" => "Username may not be longer than 60 characters.",
        "existing_user_login" => "Sorry, that username already exists!",
        "empty_user_nicename" => "Cannot create a user with an empty nicename.",
        "user_nicename_too_long" => "Nicename may not be longer than 50 characters.",
        "existing_user_email" => "Sorry, that email address is already used!",
        "user_url_too_long" => "User URL may not be longer than 100 characters.",
        _ => "Invalid user ID.",
    }
}

/// What PHP answers where it refused a write: `wp_insert_user()`'s
/// error, or for a delete `rest_cannot_delete`.
fn refusal(plan: &Plan, outcome: Outcome) -> RestError {
    match (outcome, &plan.write) {
        (Outcome::Refused(code), Write::User(_)) => RestError {
            code,
            message: insert_error(code).into(),
            data: Value::Null,
        },
        _ => {
            let message = "The user cannot be deleted.";
            RestError::status("rest_cannot_delete", message, 500)
        }
    }
}

/// The answer once written: the user, a new one with `201`; a deleted
/// user's answer as it was; or PHP's error where it refused.
fn finish(
    context: &Context,
    params: &mut Params,
    plan: Plan,
    outcome: Outcome,
) -> Result<Response, RestError> {
    let Outcome::Written(id) = outcome else {
        return Err(refusal(&plan, outcome));
    };
    if let Write::DeleteUser(..) = plan.write {
        return Ok(plan.deleted());
    }
    params.set("context", text("edit"));
    let user = context.site.user(id).unwrap_or_default();
    let mut response = prepare(context, params, &user);
    if matches!(&plan.write, Write::User(save) if save.id == 0) {
        response.status = 201;
        response.header("Location", &context.rest_url(&format!("{ROUTE}/{id}")));
    }
    Ok(response)
}
