// SPDX-License-Identifier: Apache-2.0 OR MIT

//! The SDK inside a component: the generated bindings, which WordPress's
//! functions call, and the exports. Built for `wasm32` only.

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

use bindings::hyscribe::sdk::types as wit;

#[doc(hidden)]
pub use dlmalloc::GlobalDlmalloc as __Allocator;

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
