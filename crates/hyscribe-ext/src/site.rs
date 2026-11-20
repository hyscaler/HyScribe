// SPDX-License-Identifier: AGPL-3.0-or-later

//! What the host calls reach once their grant is checked: the site the
//! request runs on, which the embedder provides, and the records they
//! carry, which cross as `Val`s. Nothing here checks a
//! grant; the host has, before it calls any of these.

use hyscribe_phpser::Value;
use wasmtime::component::{ComponentType, Lift};

/// The kinds of object WordPress keeps meta for (`types.object-kind`).
#[derive(ComponentType, Lift, Clone, Copy, Debug, PartialEq, Eq)]
#[component(enum)]
#[repr(u8)]
pub enum ObjectKind {
    /// `wp_postmeta`.
    #[component(name = "post")]
    Post,
    /// `wp_usermeta`.
    #[component(name = "user")]
    User,
    /// `wp_termmeta`.
    #[component(name = "term")]
    Term,
    /// `wp_commentmeta`.
    #[component(name = "comment")]
    Comment,
}

/// A post as `query.post` hands it to a component.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Post {
    /// `ID`.
    pub id: u64,
    /// `post_type`.
    pub post_type: String,
    /// `post_status`.
    pub status: String,
    /// `post_author`.
    pub author: u64,
    /// `post_parent`.
    pub parent: u64,
    /// `menu_order`.
    pub menu_order: i32,
    /// `post_name`.
    pub name: String,
    /// `post_title`.
    pub title: String,
    /// `post_content`.
    pub content: String,
    /// `post_excerpt`.
    pub excerpt: String,
    /// `post_date_gmt`.
    pub date_gmt: String,
    /// `post_modified_gmt`.
    pub modified_gmt: String,
}

/// A `WP_Query`'s posts, `found_posts` and `max_num_pages`.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Found {
    /// The posts of the page asked for.
    pub posts: Vec<Post>,
    /// `found_posts`.
    pub total: u64,
    /// `max_num_pages`.
    pub max_pages: u32,
}

/// `wp_mail()`'s arguments (`mail.message`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Message {
    /// `$to`.
    pub to: Vec<String>,
    /// `$subject`.
    pub subject: String,
    /// `$message`.
    pub body: String,
    /// `$headers`.
    pub headers: Vec<String>,
    /// `$attachments`: paths under a folder the manifest grants.
    pub attachments: Vec<String>,
}

/// A script or a style (`assets.asset-kind`).
#[derive(ComponentType, Lift, Clone, Copy, Debug, PartialEq, Eq)]
#[component(enum)]
#[repr(u8)]
pub enum AssetKind {
    /// `wp_register_script()`.
    #[component(name = "script")]
    Script,
    /// `wp_register_style()`.
    #[component(name = "style")]
    Style,
}

/// `wp_register_script()` or `wp_register_style()` (`assets.asset`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Asset {
    /// Script or style.
    pub kind: AssetKind,
    /// `$handle`.
    pub handle: String,
    /// A path inside the package's `public/`, empty for none.
    pub src: String,
    /// `$deps`.
    pub deps: Vec<String>,
    /// `$ver`.
    pub version: Option<String>,
    /// `in_footer` for a script, `media` for a style.
    pub placement: String,
}

/// The site a request runs on. Each call's grant is checked before it is
/// made; errors answer `sdk-error.failed`.
pub trait Site: Send + Sync {
    /// The time, in Unix seconds, for the fault record.
    fn now(&self) -> u64;
    /// `get_option( $name )`.
    fn get_option(&self, name: &str) -> Result<Option<Value>, String>;
    /// `update_option( $name, $value, $autoload )`.
    fn update_option(
        &self,
        name: &str,
        value: Value,
        autoload: Option<bool>,
    ) -> Result<bool, String>;
    /// `add_option( $name, $value, '', $autoload )`.
    fn add_option(&self, name: &str, value: Value, autoload: Option<bool>) -> Result<bool, String>;
    /// `delete_option( $name )`.
    fn delete_option(&self, name: &str) -> Result<bool, String>;
    /// `get_metadata( $kind, $id, $key )`, every value.
    fn get_meta(&self, kind: ObjectKind, id: u64, key: &str) -> Result<Vec<Value>, String>;
    /// `add_metadata()`: the new row's id, none when `unique` found one.
    fn add_meta(
        &self,
        kind: ObjectKind,
        id: u64,
        key: &str,
        value: Value,
        unique: bool,
    ) -> Result<Option<u64>, String>;
    /// `update_metadata()`.
    fn update_meta(
        &self,
        kind: ObjectKind,
        id: u64,
        key: &str,
        value: Value,
        previous: Option<Value>,
    ) -> Result<bool, String>;
    /// `delete_metadata()`.
    fn delete_meta(
        &self,
        kind: ObjectKind,
        id: u64,
        key: &str,
        value: Option<Value>,
    ) -> Result<bool, String>;
    /// Whether a post type is public, so any extension may read it.
    fn public_post_type(&self, post_type: &str) -> bool;
    /// `new WP_Query( $args )`; its post types are checked already.
    fn query(&self, args: &Value) -> Result<Found, String>;
    /// `get_post( $id )`.
    fn post(&self, id: u64) -> Result<Option<Post>, String>;
    /// `wp_schedule_event()`, or `wp_schedule_single_event()` without a
    /// recurrence.
    fn schedule(
        &self,
        timestamp: i64,
        recurrence: Option<&str>,
        hook: &str,
        args: &[Value],
    ) -> Result<(), String>;
    /// `wp_clear_scheduled_hook( $hook, $args )`: the events removed.
    fn unschedule(&self, hook: &str, args: &[Value]) -> Result<u32, String>;
    /// `wp_next_scheduled( $hook, $args )`.
    fn next_scheduled(&self, hook: &str, args: &[Value]) -> Option<i64>;
    /// `wp_mail()`.
    fn mail(&self, message: &Message) -> Result<bool, String>;
    /// `get_current_user_id()`.
    fn current_user(&self) -> u64;
    /// `current_user_can( $capability, ...$args )`: the user's own
    /// capabilities, never more.
    fn user_can(&self, capability: &str, args: &[Value]) -> bool;
    /// `wp_create_nonce( $action )`.
    fn create_nonce(&self, action: &str) -> String;
    /// `wp_verify_nonce( $nonce, $action )`: 1, 2, or 0 when it fails.
    fn verify_nonce(&self, nonce: &str, action: &str) -> u8;
    /// `_x( $text, $context, $domain )` in the extension's text domain, or
    /// `__()` without a context.
    fn translate(&self, domain: &str, text: &str, context: Option<&str>) -> String;
    /// `_nx( $single, $plural, $number, $context, $domain )`, or `_n()`.
    fn translate_plural(
        &self,
        domain: &str,
        forms: (&str, &str),
        number: u64,
        context: Option<&str>,
    ) -> String;
    /// `wp_register_script()` or `wp_register_style()` for the extension
    /// `slug`, its `src` a path inside the package's `public/`.
    fn register_asset(&self, slug: &str, asset: &Asset) -> Result<(), String>;
    /// `wp_enqueue_script( $handle )` or `wp_enqueue_style( $handle )`.
    fn enqueue(&self, kind: AssetKind, handle: &str) -> Result<(), String>;
    /// `wp_add_inline_script()` or `wp_add_inline_style()`.
    fn add_inline(
        &self,
        kind: AssetKind,
        handle: &str,
        data: &str,
        before: bool,
    ) -> Result<(), String>;
}
