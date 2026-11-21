// SPDX-License-Identifier: Apache-2.0 OR MIT

//! The SDK inside a component: the generated bindings and WordPress's
//! functions over them. Built for `wasm32` only.

use alloc::string::String;
use alloc::vec::Vec;

use crate::{Plugin, Value};

/// The bindings wit-bindgen generates for the `plugin` world, or the
/// `theme` world with the `theme` feature. Guest code, inside the sandbox:
/// the one place the SDK allows `unsafe`.
#[doc(hidden)]
#[allow(unsafe_code, missing_docs, clippy::all)]
pub mod bindings {
    #[cfg(not(feature = "theme"))]
    wit_bindgen::generate!({
        world: "plugin",
        path: "../hyscribe-ext/wit",
        pub_export_macro: true,
        export_macro_name: "export",
        default_bindings_module: "::hyscribe_sdk::bindings",
    });
    #[cfg(feature = "theme")]
    wit_bindgen::generate!({
        world: "theme",
        path: "../hyscribe-ext/wit",
        pub_export_macro: true,
        export_macro_name: "export",
        default_bindings_module: "::hyscribe_sdk::bindings",
    });
}

use bindings::hyscribe::sdk::types::{self as wit, ValueKind};

pub use bindings::hyscribe::sdk::types::{ObjectKind, SdkError};

#[doc(hidden)]
pub use dlmalloc::GlobalDlmalloc as __Allocator;

impl Value {
    #[doc(hidden)]
    pub fn __from_wit((kind, int, float, text): wit::Value) -> Value {
        match kind {
            ValueKind::Null => Value::Null,
            ValueKind::Boolean => Value::Bool(int != 0),
            ValueKind::Int => Value::Int(int),
            ValueKind::Float => Value::Float(float),
            ValueKind::Text => Value::Text(text),
            ValueKind::Json => Value::Json(text),
        }
    }

    #[doc(hidden)]
    pub fn __to_wit(&self) -> wit::Value {
        match self {
            Value::Null => (ValueKind::Null, 0, 0.0, String::new()),
            Value::Bool(b) => (ValueKind::Boolean, i64::from(*b), 0.0, String::new()),
            Value::Int(n) => (ValueKind::Int, *n, 0.0, String::new()),
            Value::Float(f) => (ValueKind::Float, 0, *f, String::new()),
            Value::Text(text) => (ValueKind::Text, 0, 0.0, text.clone()),
            Value::Json(json) => (ValueKind::Json, 0, 0.0, json.clone()),
        }
    }
}

fn to_wit(values: &[Value]) -> Vec<wit::Value> {
    values.iter().map(Value::__to_wit).collect()
}

/// `guest.invoke` for a plugin: its values in and out.
#[doc(hidden)]
pub fn __invoke<P: Plugin>(callback: u32, args: Vec<wit::Value>) -> Result<wit::Value, String> {
    let args = args.into_iter().map(Value::__from_wit).collect();
    P::invoke(callback, args).map(|value| value.__to_wit())
}

/// What every component needs: the allocator, panic handler and canonical
/// ABI allocator a component without `std` needs, so it imports nothing
/// from WASI, and its `guest`.
#[doc(hidden)]
#[macro_export]
macro_rules! __component {
    ($plugin:ty) => {
        #[global_allocator]
        static ALLOCATOR: $crate::__Allocator = $crate::__Allocator;

        #[panic_handler]
        fn panic(_: &::core::panic::PanicInfo) -> ! {
            ::core::arch::wasm32::unreachable()
        }

        // The canonical ABI's allocator, which `std` would otherwise export.
        #[unsafe(no_mangle)]
        unsafe extern "C" fn cabi_realloc(
            old: *mut u8,
            old_len: usize,
            align: usize,
            new_len: usize,
        ) -> *mut u8 {
            use ::core::alloc::{GlobalAlloc, Layout};
            if new_len == 0 {
                return align as *mut u8;
            }
            let ptr = unsafe {
                if old_len == 0 {
                    ALLOCATOR.alloc(Layout::from_size_align_unchecked(new_len, align))
                } else {
                    ALLOCATOR.realloc(
                        old,
                        Layout::from_size_align_unchecked(old_len, align),
                        new_len,
                    )
                }
            };
            if ptr.is_null() {
                ::core::arch::wasm32::unreachable()
            }
            ptr
        }

        // Without libc, the comparisons slices and strings lower to.
        #[unsafe(no_mangle)]
        unsafe extern "C" fn memcmp(a: *const u8, b: *const u8, n: usize) -> i32 {
            for i in 0..n {
                // Volatile reads keep the loop from becoming a call to memcmp.
                let (x, y) = unsafe { (a.add(i).read_volatile(), b.add(i).read_volatile()) };
                if x != y {
                    return i32::from(x) - i32::from(y);
                }
            }
            0
        }

        #[unsafe(no_mangle)]
        unsafe extern "C" fn bcmp(a: *const u8, b: *const u8, n: usize) -> i32 {
            unsafe { memcmp(a, b, n) }
        }

        struct Exported;

        impl $crate::bindings::exports::hyscribe::sdk::guest::Guest for Exported {
            fn init() -> ::core::result::Result<(), $crate::__String> {
                <$plugin as $crate::Plugin>::init()
            }

            fn activate() -> ::core::result::Result<(), $crate::__String> {
                <$plugin as $crate::Plugin>::activate()
            }

            fn deactivate() -> ::core::result::Result<(), $crate::__String> {
                <$plugin as $crate::Plugin>::deactivate()
            }

            fn invoke(
                callback: u32,
                args: $crate::__Vec<$crate::bindings::hyscribe::sdk::types::Value>,
            ) -> ::core::result::Result<
                $crate::bindings::hyscribe::sdk::types::Value,
                $crate::__String,
            > {
                $crate::__invoke::<$plugin>(callback, args)
            }
        }
    };
}

/// Exports a [`Plugin`] as the component's `guest`.
#[cfg(not(feature = "theme"))]
#[macro_export]
macro_rules! plugin {
    ($plugin:ty) => {
        $crate::__component!($plugin);
        $crate::bindings::export!(Exported with_types_in $crate::bindings);
    };
}

/// Exports a [`Theme`](crate::Theme) as the component's `guest` and
/// `theme-guest`.
#[cfg(feature = "theme")]
#[macro_export]
macro_rules! theme {
    ($theme:ty) => {
        $crate::__component!($theme);

        impl $crate::bindings::exports::hyscribe::sdk::theme_guest::Guest for Exported {
            fn render(
                file: $crate::__String,
            ) -> ::core::result::Result<$crate::__Vec<u8>, $crate::__String> {
                <$theme as $crate::Theme>::render(&file)
            }
        }

        $crate::bindings::export!(Exported with_types_in $crate::bindings);
    };
}

/// `add_action()`, `add_filter()`, `apply_filters()`, `do_action()`.
pub mod hooks {
    use alloc::string::String;
    use alloc::vec::Vec;

    use super::bindings::hyscribe::sdk::hooks::{self as wit, HookKind, Link};
    use super::{SdkError, to_wit};
    use crate::{Plugin, Value};

    /// `add_filter( $hook, $callback, $priority, $accepted_args )`, in `init`.
    pub fn on_filter(
        hook: &str,
        callback: impl Into<u32>,
        priority: i32,
        accepted_args: u32,
    ) -> Result<(), SdkError> {
        wit::add(
            hook,
            HookKind::Filter,
            callback.into(),
            priority,
            accepted_args,
        )
    }

    /// `add_action( $hook, $callback, $priority, $accepted_args )`, in `init`.
    pub fn on_action(
        hook: &str,
        callback: impl Into<u32>,
        priority: i32,
        accepted_args: u32,
    ) -> Result<(), SdkError> {
        wit::add(
            hook,
            HookKind::Action,
            callback.into(),
            priority,
            accepted_args,
        )
    }

    /// `remove_filter( $hook, $callback, $priority )` of its own callback.
    pub fn remove(hook: &str, callback: impl Into<u32>, priority: i32) -> bool {
        wit::remove(hook, callback.into(), priority)
    }

    /// Runs `hook`'s chain: the plugin's own callbacks here, every other
    /// run by the host, in WordPress's order.
    fn run<P: Plugin>(hook: &str, mut args: Vec<Value>, filter: bool) -> Vec<Value> {
        let Ok((token, links)) = wit::chain(hook) else {
            return args;
        };
        for link in links {
            let answer = match link {
                Link::Mine(callback) => P::invoke(callback, args.clone()).ok(),
                Link::Others(step) => wit::run_step(token, step, &to_wit(&args))
                    .ok()
                    .map(Value::__from_wit),
            };
            if let (Some(answer), true, Some(first)) = (answer, filter, args.first_mut()) {
                *first = answer;
            }
        }
        args
    }

    /// `apply_filters( $hook, $value, ...$args )`.
    pub fn filter<P: Plugin>(hook: &str, value: Value, args: &[Value]) -> Value {
        let mut all = Vec::with_capacity(args.len() + 1);
        all.push(value);
        all.extend_from_slice(args);
        run::<P>(hook, all, true).swap_remove(0)
    }

    /// `do_action( $hook, ...$args )`.
    pub fn fire<P: Plugin>(hook: &str, args: &[Value]) {
        run::<P>(hook, args.to_vec(), false);
    }

    /// `current_filter()`.
    pub fn current() -> Option<String> {
        wit::current()
    }

    /// `did_action( $hook )`.
    pub fn did_action(hook: &str) -> u32 {
        wit::did_action(hook)
    }
}

/// `get_option()` and friends, for the option names the manifest grants.
pub mod options {
    use super::SdkError;
    use super::bindings::hyscribe::sdk::options as wit;
    use crate::Value;

    /// `get_option( $name )`: none when it does not exist.
    pub fn get(name: &str) -> Result<Option<Value>, SdkError> {
        wit::get(name).map(|value| value.map(Value::__from_wit))
    }

    /// `update_option( $name, $value, $autoload )`.
    pub fn set(name: &str, value: &Value, autoload: Option<bool>) -> Result<bool, SdkError> {
        wit::update(name, &value.__to_wit(), autoload)
    }

    /// `add_option( $name, $value, '', $autoload )`.
    pub fn add(name: &str, value: &Value, autoload: Option<bool>) -> Result<bool, SdkError> {
        wit::add(name, &value.__to_wit(), autoload)
    }

    /// `delete_option( $name )`.
    pub fn delete(name: &str) -> Result<bool, SdkError> {
        wit::delete(name)
    }
}

/// `get_metadata()` and friends, for the meta keys the manifest grants.
pub mod meta {
    use alloc::vec::Vec;

    use super::bindings::hyscribe::sdk::meta as wit;
    use super::{ObjectKind, SdkError};
    use crate::Value;

    /// `get_metadata( $kind, $id, $key )`: every value.
    pub fn get(kind: ObjectKind, id: u64, key: &str) -> Result<Vec<Value>, SdkError> {
        wit::get(kind, id, key).map(|values| values.into_iter().map(Value::__from_wit).collect())
    }

    /// `add_metadata()`: the new row's id, none when `unique` found one.
    pub fn add(
        kind: ObjectKind,
        id: u64,
        key: &str,
        value: &Value,
        unique: bool,
    ) -> Result<Option<u64>, SdkError> {
        wit::add(kind, id, key, &value.__to_wit(), unique)
    }

    /// `update_metadata()`, optionally only where it held `previous`.
    pub fn update(
        kind: ObjectKind,
        id: u64,
        key: &str,
        value: &Value,
        previous: Option<&Value>,
    ) -> Result<bool, SdkError> {
        let previous = previous.map(Value::__to_wit);
        wit::update(kind, id, key, &value.__to_wit(), previous.as_ref())
    }

    /// `delete_metadata()`, optionally only where it holds `value`.
    pub fn delete(
        kind: ObjectKind,
        id: u64,
        key: &str,
        value: Option<&Value>,
    ) -> Result<bool, SdkError> {
        let value = value.map(Value::__to_wit);
        wit::delete(kind, id, key, value.as_ref())
    }

    /// `register_meta( $kind, $key, $args )`, in `init`, its arguments as JSON.
    pub fn register(
        kind: ObjectKind,
        subtype: &str,
        key: &str,
        args: &str,
    ) -> Result<(), SdkError> {
        wit::register(kind, subtype, key, args)
    }
}

/// `register_post_type()` and `register_taxonomy()`, in `init`.
pub mod content {
    use alloc::string::String;

    use super::SdkError;
    use super::bindings::hyscribe::sdk::content as wit;

    /// `register_post_type( $name, $args )`, its arguments as JSON.
    pub fn register_post_type(name: &str, args: &str) -> Result<(), SdkError> {
        wit::register_post_type(name, args)
    }

    /// `register_taxonomy( $name, $object_types, $args )`.
    pub fn register_taxonomy(
        name: &str,
        object_types: &[String],
        args: &str,
    ) -> Result<(), SdkError> {
        wit::register_taxonomy(name, object_types, args)
    }
}

/// `WP_Query` and `get_post()`.
pub mod query {
    use alloc::string::String;
    use alloc::vec::Vec;

    use super::SdkError;
    use super::bindings::hyscribe::sdk::query as wit;
    pub use super::bindings::hyscribe::sdk::query::{Found, Post};

    /// `new WP_Query( $args )`, its query variables built one by one.
    #[derive(Clone, Debug, Default)]
    pub struct Query {
        vars: Vec<(String, String)>,
    }

    impl Query {
        /// No variables yet: the latest posts.
        pub fn new() -> Query {
            Query::default()
        }

        /// Sets a query variable to a JSON value.
        pub fn var(mut self, name: &str, json: &str) -> Query {
            self.vars.push((name.into(), json.into()));
            self
        }

        /// Runs the query: its post types must be public or granted.
        pub fn posts(&self) -> Result<Found, SdkError> {
            let entries: Vec<(&str, &str)> = self
                .vars
                .iter()
                .map(|(name, json)| (name.as_str(), json.as_str()))
                .collect();
            wit::posts(&crate::json::object(&entries))
        }
    }

    /// `get_post( $id )`.
    pub fn get_post(id: u64) -> Result<Option<Post>, SdkError> {
        wit::get_post(id)
    }
}

/// `register_rest_route()`, in `init`.
pub mod rest {
    use alloc::string::String;

    use super::SdkError;
    use super::bindings::hyscribe::sdk::rest::{self as wit, Route};

    /// `register_rest_route( $namespace, $route, $args )`: requests reach
    /// `callback` as one JSON value once `permission` answered `true`.
    pub fn route(
        namespace: &str,
        path: &str,
        methods: &[&str],
        callback: impl Into<u32>,
        permission: impl Into<u32>,
        args: &str,
    ) -> Result<(), SdkError> {
        wit::register(&Route {
            namespace: namespace.into(),
            path: path.into(),
            methods: methods.iter().map(|method| String::from(*method)).collect(),
            callback: callback.into(),
            permission: permission.into(),
            args: args.into(),
        })
    }
}

/// `add_shortcode()` and `remove_shortcode()`.
pub mod shortcodes {
    use super::SdkError;
    use super::bindings::hyscribe::sdk::shortcodes as wit;

    /// `add_shortcode( $tag, $callback )`, in `init`: the callback gets the
    /// attributes as JSON, the content and the tag.
    pub fn register(tag: &str, callback: impl Into<u32>) -> Result<(), SdkError> {
        wit::add(tag, callback.into())
    }

    /// `remove_shortcode( $tag )` of its own shortcode.
    pub fn remove(tag: &str) -> bool {
        wit::remove(tag)
    }
}

/// `register_block_type()` from the package's `block.json`.
pub mod blocks {
    use super::SdkError;
    use super::bindings::hyscribe::sdk::blocks as wit;

    /// `register_block_type( $path, array( 'render_callback' => ... ) )`, in `init`.
    pub fn register(path: &str, render: Option<u32>) -> Result<(), SdkError> {
        wit::register(path, render)
    }
}

/// `wp_register_script()`, `wp_enqueue_style()` and friends.
pub mod assets {
    use super::SdkError;
    use super::bindings::hyscribe::sdk::assets as wit;
    pub use super::bindings::hyscribe::sdk::assets::{Asset, AssetKind};

    /// `wp_register_script()` or `wp_register_style()`, its `src` a file of
    /// the package's `public/`.
    pub fn register(asset: &Asset) -> Result<(), SdkError> {
        wit::register(asset)
    }

    /// `wp_enqueue_script( $handle )` or `wp_enqueue_style( $handle )`.
    pub fn enqueue(kind: AssetKind, handle: &str) -> Result<(), SdkError> {
        wit::enqueue(kind, handle)
    }

    /// `wp_add_inline_script()` or `wp_add_inline_style()`.
    pub fn add_inline(
        kind: AssetKind,
        handle: &str,
        data: &str,
        before: bool,
    ) -> Result<(), SdkError> {
        wit::add_inline(kind, handle, data, before)
    }
}

/// `wp_schedule_event()` and friends, for hooks the manifest declares.
pub mod cron {
    use super::bindings::hyscribe::sdk::cron as wit;
    use super::{SdkError, to_wit};
    use crate::Value;

    /// `wp_schedule_event( $timestamp, $recurrence, $hook, $args )`, or
    /// `wp_schedule_single_event()` without a recurrence.
    pub fn schedule(
        timestamp: i64,
        recurrence: Option<&str>,
        hook: &str,
        args: &[Value],
    ) -> Result<(), SdkError> {
        wit::schedule(timestamp, recurrence, hook, &to_wit(args))
    }

    /// `wp_clear_scheduled_hook( $hook, $args )`: the events removed.
    pub fn unschedule(hook: &str, args: &[Value]) -> Result<u32, SdkError> {
        wit::unschedule(hook, &to_wit(args))
    }

    /// `wp_next_scheduled( $hook, $args )`.
    pub fn next(hook: &str, args: &[Value]) -> Option<i64> {
        wit::next(hook, &to_wit(args))
    }
}

/// `wp_mail()`, when the manifest grants mail.
pub mod mail {
    use super::SdkError;
    use super::bindings::hyscribe::sdk::mail as wit;
    pub use super::bindings::hyscribe::sdk::mail::Message;

    /// `wp_mail( $to, $subject, $message, $headers, $attachments )`.
    pub fn send(message: &Message) -> Result<bool, SdkError> {
        wit::send(message)
    }
}

/// The current user, `current_user_can()` and nonces.
pub mod auth {
    use alloc::string::String;

    use super::bindings::hyscribe::sdk::auth as wit;
    use super::to_wit;
    use crate::Value;

    /// `get_current_user_id()`.
    pub fn current_user() -> u64 {
        wit::current_user()
    }

    /// `current_user_can( $capability, ...$args )`.
    pub fn can(capability: &str, args: &[Value]) -> bool {
        wit::user_can(capability, &to_wit(args))
    }

    /// `wp_create_nonce( $action )`.
    pub fn create_nonce(action: &str) -> String {
        wit::create_nonce(action)
    }

    /// `wp_verify_nonce( $nonce, $action )`: 1, 2, or 0 when it fails.
    pub fn verify_nonce(nonce: &str, action: &str) -> u8 {
        wit::verify_nonce(nonce, action)
    }
}

/// `__()`, `_x()`, `_n()` and `_nx()` in the extension's text domain.
pub mod i18n {
    use alloc::string::String;

    use super::bindings::hyscribe::sdk::i18n as wit;

    /// `__( $text )`.
    pub fn translate(text: &str) -> String {
        wit::translate(text, None)
    }

    /// `_x( $text, $context )`.
    pub fn translate_with_context(text: &str, context: &str) -> String {
        wit::translate(text, Some(context))
    }

    /// `_n( $single, $plural, $number )`.
    pub fn plural(single: &str, plural: &str, number: u64) -> String {
        wit::translate_plural(single, plural, number, None)
    }

    /// `_nx( $single, $plural, $number, $context )`.
    pub fn plural_with_context(single: &str, plural: &str, number: u64, context: &str) -> String {
        wit::translate_plural(single, plural, number, Some(context))
    }
}

/// `get_transient()` and friends, and their site forms, for the transient
/// names the manifest grants (SDK 0.2).
pub mod transients {
    use super::SdkError;
    use super::bindings::hyscribe::sdk::transients as wit;
    use crate::Value;

    /// `get_transient( $name )`: none when missing or expired.
    pub fn get(name: &str) -> Result<Option<Value>, SdkError> {
        wit::get(name, false).map(|value| value.map(Value::__from_wit))
    }

    /// `set_transient( $name, $value, $expiration )`, 0 for no expiry.
    pub fn set(name: &str, value: &Value, expiration: i64) -> Result<bool, SdkError> {
        wit::set(name, &value.__to_wit(), expiration, false)
    }

    /// `delete_transient( $name )`.
    pub fn delete(name: &str) -> Result<bool, SdkError> {
        wit::delete(name, false)
    }

    /// `get_site_transient( $name )`.
    pub fn get_site(name: &str) -> Result<Option<Value>, SdkError> {
        wit::get(name, true).map(|value| value.map(Value::__from_wit))
    }

    /// `set_site_transient( $name, $value, $expiration )`.
    pub fn set_site(name: &str, value: &Value, expiration: i64) -> Result<bool, SdkError> {
        wit::set(name, &value.__to_wit(), expiration, true)
    }

    /// `delete_site_transient( $name )`.
    pub fn delete_site(name: &str) -> Result<bool, SdkError> {
        wit::delete(name, true)
    }
}

/// `get_role()`, `add_role()`, `remove_role()` and a role's `add_cap()`
/// and `remove_cap()`, for the roles and capabilities the manifest grants
/// (SDK 0.2).
pub mod roles {
    use alloc::string::String;
    use alloc::vec::Vec;

    use super::SdkError;
    use super::bindings::hyscribe::sdk::roles as wit;

    /// `get_role( $role )->capabilities`: none for no such role.
    pub fn get(role: &str) -> Result<Option<Vec<(String, bool)>>, SdkError> {
        wit::get(role)
    }

    /// `add_role( $role, $display_name, $capabilities )`: false when the
    /// role exists.
    pub fn add(role: &str, display_name: &str, caps: &[(&str, bool)]) -> Result<bool, SdkError> {
        let caps: Vec<(String, bool)> = caps
            .iter()
            .map(|(cap, grant)| (String::from(*cap), *grant))
            .collect();
        wit::add(role, display_name, &caps)
    }

    /// `remove_role( $role )`: false for no such role.
    pub fn remove(role: &str) -> Result<bool, SdkError> {
        wit::remove(role)
    }

    /// `get_role( $role )->add_cap( $cap, $grant )`: false for no such role.
    pub fn add_cap(role: &str, cap: &str, grant: bool) -> Result<bool, SdkError> {
        wit::add_cap(role, cap, grant)
    }

    /// `get_role( $role )->remove_cap( $cap )`: false for no such role.
    pub fn remove_cap(role: &str, cap: &str) -> Result<bool, SdkError> {
        wit::remove_cap(role, cap)
    }
}

/// `wp_safe_remote_request()` to the hosts the manifest grants (SDK 0.2).
pub mod http {
    use alloc::string::String;
    use alloc::vec::Vec;

    use super::SdkError;
    use super::bindings::hyscribe::sdk::http as wit;

    /// An answer: its status, its headers (names in lower case) and its body.
    pub type Response = (u16, Vec<(String, Vec<u8>)>, Vec<u8>);

    /// `wp_safe_remote_request( $url, array( 'method' => $method, 'headers'
    /// => $headers, 'body' => $body, 'timeout' => $timeout ) )`.
    pub fn send(
        method: &str,
        url: &str,
        headers: &[(&str, &str)],
        body: &[u8],
        timeout: u32,
    ) -> Result<Response, SdkError> {
        let headers: Vec<(String, String)> = headers
            .iter()
            .map(|(name, value)| (String::from(*name), String::from(*value)))
            .collect();
        wit::send(method, url, &headers, body, timeout)
    }
}

/// The loop and the template tags a classic theme's templates call (SDK
/// 0.3, ADR 0023), by WordPress's names.
#[cfg(feature = "theme")]
pub mod template {
    use alloc::string::String;
    use alloc::vec::Vec;

    use super::bindings::hyscribe::sdk::template::{self as wit, PartKind};
    use super::{SdkError, to_wit};
    use crate::Value;

    /// `have_posts()`.
    pub fn have_posts() -> bool {
        wit::have_posts()
    }

    /// `the_post()`.
    pub fn the_post() {
        wit::the_post();
    }

    /// `rewind_posts()`.
    pub fn rewind_posts() {
        wit::rewind_posts();
    }

    /// `wp_reset_postdata()`.
    pub fn wp_reset_postdata() {
        wit::reset_postdata();
    }

    /// `in_the_loop()`.
    pub fn in_the_loop() -> bool {
        wit::in_the_loop()
    }

    /// A conditional tag by name: `is( "is_singular", &[] )` is `is_singular()`.
    pub fn is(name: &str, args: &[Value]) -> Result<bool, SdkError> {
        wit::conditional(name, &to_wit(args))
    }

    /// A template tag that prints, by name: the bytes PHP prints.
    pub fn print(name: &str, args: &[Value]) -> Result<Vec<u8>, SdkError> {
        wit::print(name, &to_wit(args))
    }

    /// A template tag that answers a value, by name.
    pub fn get(name: &str, args: &[Value]) -> Result<Value, SdkError> {
        wit::get(name, &to_wit(args)).map(Value::__from_wit)
    }

    /// `get_header( $name, $args )`: the file to render, if any.
    pub fn get_header(name: Option<&str>, args: &[Value]) -> Option<String> {
        wit::part(PartKind::Header, "header", name, &to_wit(args))
    }

    /// `get_footer( $name, $args )`.
    pub fn get_footer(name: Option<&str>, args: &[Value]) -> Option<String> {
        wit::part(PartKind::Footer, "footer", name, &to_wit(args))
    }

    /// `get_sidebar( $name, $args )`.
    pub fn get_sidebar(name: Option<&str>, args: &[Value]) -> Option<String> {
        wit::part(PartKind::Sidebar, "sidebar", name, &to_wit(args))
    }

    /// `get_template_part( $slug, $name, $args )`.
    pub fn get_template_part(slug: &str, name: Option<&str>, args: &[Value]) -> Option<String> {
        wit::part(PartKind::TemplatePart, slug, name, &to_wit(args))
    }
}

/// What a classic theme's `functions.php` sets up, and its theme mods (SDK
/// 0.3, ADR 0023).
#[cfg(feature = "theme")]
pub mod theme {
    use alloc::string::String;
    use alloc::vec::Vec;

    use super::bindings::hyscribe::sdk::theme_setup as wit;
    pub use super::bindings::hyscribe::sdk::theme_setup::TemplateFile;
    use super::{SdkError, to_wit};
    use crate::Value;

    /// The template files the theme has, in `init`.
    pub fn templates(files: &[TemplateFile]) -> Result<(), SdkError> {
        wit::templates(files)
    }

    /// `add_theme_support( $feature, ...$args )`.
    pub fn add_support(feature: &str, args: &[Value]) -> Result<(), SdkError> {
        wit::add_support(feature, &to_wit(args))
    }

    /// `remove_theme_support( $feature )`.
    pub fn remove_support(feature: &str) -> bool {
        wit::remove_support(feature)
    }

    /// `register_nav_menus( $locations )`: each location and its description.
    pub fn register_nav_menus(locations: &[(&str, &str)]) -> Result<(), SdkError> {
        let locations: Vec<(String, String)> = locations
            .iter()
            .map(|(location, description)| (String::from(*location), String::from(*description)))
            .collect();
        wit::register_nav_menus(&locations)
    }

    /// `register_sidebar( $args )`, its arguments as JSON: its id.
    pub fn register_sidebar(args: &str) -> Result<String, SdkError> {
        wit::register_sidebar(args)
    }

    /// `add_image_size( $name, $width, $height, $crop )`.
    pub fn add_image_size(
        name: &str,
        width: u32,
        height: u32,
        crop: &Value,
    ) -> Result<(), SdkError> {
        wit::add_image_size(name, width, height, &crop.__to_wit())
    }

    /// `set_post_thumbnail_size( $width, $height, $crop )`.
    pub fn set_post_thumbnail_size(width: u32, height: u32, crop: &Value) -> Result<(), SdkError> {
        wit::set_post_thumbnail_size(width, height, &crop.__to_wit())
    }

    /// `add_editor_style( $stylesheets )`, files of the package's `public/`.
    pub fn add_editor_style(files: &[&str]) -> Result<(), SdkError> {
        let files: Vec<String> = files.iter().map(|file| String::from(*file)).collect();
        wit::add_editor_style(&files)
    }

    /// `$content_width = $width`.
    pub fn set_content_width(width: u32) -> Result<(), SdkError> {
        wit::set_content_width(width)
    }

    /// `register_block_style( $block_name, $style_properties )`, as JSON.
    pub fn register_block_style(block: &str, args: &str) -> Result<(), SdkError> {
        wit::register_block_style(block, args)
    }

    /// `register_block_pattern( $pattern_name, $pattern_properties )`, as JSON.
    pub fn register_block_pattern(name: &str, args: &str) -> Result<(), SdkError> {
        wit::register_block_pattern(name, args)
    }

    /// `get_theme_mod( $name, $default )`.
    pub fn get_mod(name: &str, default: Option<&Value>) -> Result<Option<Value>, SdkError> {
        let default = default.map(Value::__to_wit);
        let value = wit::get_mod(name, default.as_ref());
        value.map(|value| value.map(Value::__from_wit))
    }

    /// `set_theme_mod( $name, $value )`.
    pub fn set_mod(name: &str, value: &Value) -> Result<bool, SdkError> {
        wit::set_mod(name, &value.__to_wit())
    }

    /// `remove_theme_mod( $name )`.
    pub fn remove_mod(name: &str) -> Result<bool, SdkError> {
        wit::remove_mod(name)
    }
}

/// `customize_register`'s `$wp_customize` (SDK 0.3, ADR 0023).
#[cfg(feature = "theme")]
pub mod customize {
    use alloc::string::String;
    use alloc::vec::Vec;

    use super::SdkError;
    use super::bindings::hyscribe::sdk::customize as wit;
    pub use super::bindings::hyscribe::sdk::customize::CustomizeKind;

    /// `$wp_customize->add_panel()` and the others: its id, its arguments
    /// as JSON and its callbacks (`sanitize_callback` and the rest) by number.
    pub fn add(
        kind: CustomizeKind,
        id: &str,
        args: &str,
        callbacks: &[(&str, u32)],
    ) -> Result<(), SdkError> {
        let callbacks: Vec<(String, u32)> = callbacks
            .iter()
            .map(|(role, callback)| (String::from(*role), *callback))
            .collect();
        wit::add(kind, id, args, &callbacks)
    }

    /// `$wp_customize->remove_panel()` and the others.
    pub fn remove(kind: CustomizeKind, id: &str) -> bool {
        wit::remove(kind, id)
    }
}
