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

/// A role's capabilities, each granted or denied (`roles.capability`).
pub type Caps = Vec<(String, bool)>;

/// An extension's request (`http.send`), its host granted and its method
/// and headers checked.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Outgoing {
    /// The method, upper case.
    pub method: String,
    /// The URL.
    pub url: String,
    /// The headers it adds.
    pub headers: Vec<(String, String)>,
    /// The body.
    pub body: Vec<u8>,
    /// The timeout, in seconds.
    pub timeout: u32,
}

/// An answer: its status, its headers (names in lower case) and its body.
pub type Incoming = (u16, Vec<(String, Vec<u8>)>, Vec<u8>);

/// The template part functions (`template.part-kind`, SDK 0.3).
#[derive(ComponentType, Lift, Clone, Copy, Debug, PartialEq, Eq)]
#[component(enum)]
#[repr(u8)]
pub enum Part {
    /// `get_header()`.
    #[component(name = "header")]
    Header,
    /// `get_footer()`.
    #[component(name = "footer")]
    Footer,
    /// `get_sidebar()`.
    #[component(name = "sidebar")]
    Sidebar,
    /// `get_template_part()`.
    #[component(name = "template-part")]
    TemplatePart,
}

/// What `$wp_customize` adds (`customize.customize-kind`, SDK 0.3).
#[derive(ComponentType, Lift, Clone, Copy, Debug, PartialEq, Eq)]
#[component(enum)]
#[repr(u8)]
pub enum CustomizeKind {
    /// `add_panel()`.
    #[component(name = "panel")]
    Panel,
    /// `add_section()`.
    #[component(name = "section")]
    Section,
    /// `add_setting()`.
    #[component(name = "setting")]
    Setting,
    /// `add_control()`.
    #[component(name = "control")]
    Control,
    /// `selective_refresh->add_partial()`.
    #[component(name = "partial")]
    Partial,
}

/// A call of the theme world's `template`, `theme-setup` and `customize`
/// interfaces (SDK 0.3, ADR 0023), its arguments checked; a theme makes
/// them, so they reach the request's query and its theme state.
#[derive(Clone, Debug, PartialEq)]
pub enum ThemeCall {
    /// `have_posts()`: answers a boolean.
    HavePosts,
    /// `the_post()`.
    ThePost,
    /// `rewind_posts()`.
    RewindPosts,
    /// `wp_reset_postdata()`.
    ResetPostdata,
    /// `in_the_loop()`: answers a boolean.
    InTheLoop,
    /// A conditional tag by name, with its arguments: answers a boolean.
    Conditional(String, Vec<Value>),
    /// A template tag that prints: answers the bytes, as a string.
    Print(String, Vec<Value>),
    /// A template tag that answers a value.
    Get(String, Vec<Value>),
    /// `get_header()` and the others: answers the template file to
    /// render, null for none.
    Part(Part, String, Option<String>, Vec<Value>),
    /// `add_theme_support( $feature, ...$args )`.
    AddSupport(String, Vec<Value>),
    /// `remove_theme_support( $feature )`: answers a boolean.
    RemoveSupport(String),
    /// `register_nav_menus( $locations )`.
    NavMenus(Vec<(String, String)>),
    /// `register_sidebar( $args )`: answers its id.
    Sidebar(Value),
    /// `add_image_size( $name, $width, $height, $crop )`.
    ImageSize(String, u32, u32, Value),
    /// `set_post_thumbnail_size( $width, $height, $crop )`.
    ThumbnailSize(u32, u32, Value),
    /// `add_editor_style( $stylesheets )`, paths inside `public/`.
    EditorStyle(Vec<String>),
    /// `$content_width`.
    ContentWidth(u32),
    /// `register_block_style( $block_name, $style_properties )`.
    BlockStyle(String, Value),
    /// `register_block_pattern( $pattern_name, $pattern_properties )`.
    BlockPattern(String, Value),
    /// `get_theme_mod( $name, $default )`: answers the value, null for none.
    GetMod(String, Option<Value>),
    /// `set_theme_mod( $name, $value )`: answers a boolean.
    SetMod(String, Value),
    /// `remove_theme_mod( $name )`: answers a boolean.
    RemoveMod(String),
    /// `$wp_customize->add_*()`: its id, arguments and callbacks by number.
    Customize(CustomizeKind, String, Value, Vec<(String, u32)>),
    /// `$wp_customize->remove_*()`: answers a boolean.
    Uncustomize(CustomizeKind, String),
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
    /// `get_transient( $name )`, or `get_site_transient()`: none for a
    /// missing or expired one, which it deletes.
    fn get_transient(&self, name: &str, site: bool) -> Result<Option<Value>, String>;
    /// `set_transient( $name, $value, $expiration )`, or `set_site_transient()`.
    fn set_transient(
        &self,
        name: &str,
        value: Value,
        expiration: i64,
        site: bool,
    ) -> Result<bool, String>;
    /// `delete_transient( $name )`, or `delete_site_transient()`.
    fn delete_transient(&self, name: &str, site: bool) -> Result<bool, String>;
    /// `get_role( $role )->capabilities`: none for no such role.
    fn role(&self, role: &str) -> Result<Option<Caps>, String>;
    /// `add_role( $role, $display_name, $capabilities )`: false when it exists.
    fn add_role(
        &self,
        role: &str,
        display_name: &str,
        caps: &[(String, bool)],
    ) -> Result<bool, String>;
    /// `remove_role( $role )`: false for no such role.
    fn remove_role(&self, role: &str) -> Result<bool, String>;
    /// `WP_Role::add_cap( $cap, $grant )`, or `remove_cap( $cap )` without a
    /// grant: false for no such role.
    fn role_cap(&self, role: &str, cap: &str, grant: Option<bool>) -> Result<bool, String>;
    /// `wp_safe_remote_request()` of `request`, every redirect only to a
    /// host `allowed` accepts.
    fn fetch(
        &self,
        request: &Outgoing,
        allowed: &(dyn Fn(&str) -> bool + Sync),
    ) -> Result<Incoming, String>;
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
    /// A call of the theme `stylesheet` (SDK 0.3), answered as a PHP value.
    fn theme(&self, stylesheet: &str, call: &ThemeCall) -> Result<Value, String>;
    /// `settings_fields( $group )` on the admin page the request serves
    /// (SDK 0.4): its nonce is the current user's.
    fn settings_fields(&self, group: &str) -> Result<Vec<u8>, String>;
}
