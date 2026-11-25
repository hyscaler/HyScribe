# Plugins and themes

## Requirements

- Extensions are Rust packages
- Installing or activating an extension must not require recompiling `hyscribe`
- The API should feel familiar to WordPress developers so that ports are mechanical
- A buggy or malicious extension must not be able to crash the server or read data it did not ask for
- One extension maps to one WordPress plugin or theme slug, so the `active_plugins` and `template`/`stylesheet` options keep their meaning

## Runtime options considered

| Option | Pros | Cons |
| --- | --- | --- |
| Compile-time crates (Cargo features) | Fastest, simplest | Every install needs a rebuild; breaks the single-binary promise |
| Native dynamic libraries (`cdylib`) | Fast, hot-loadable | Rust has no stable ABI; crashes take the server down; no sandbox |
| **WebAssembly components (wasmtime)** | Sandboxed, hot-loadable, portable across OS/arch, typed interfaces via WIT, still written in Rust | Call overhead per hook; host API must be designed carefully |
| Embedded scripting (Lua, Rhai) | Easy hot reload | Not Rust packages; slower |

**Proposal (ADR 0002):** extensions are Rust crates compiled to `wasm32-wasip2` components and loaded by wasmtime. First-party core features can still be compiled in natively behind the same hook API. The host's design, SDK 0.1's WIT world, limits and package format: ADR 0022.

## Package format

Source crate:

```
contact-form-7/
  Cargo.toml               package name hyscribe-plugin-contact-form-7
  hyscribe.ext.toml        manifest
  src/lib.rs
  public/                  css, js, images, at the same relative paths as the original plugin
  templates/               templates (engine chosen by the classic theme ADR, M7.2)
  languages/               .mo / .json translations
```

Built and packaged as `contact-form-7-0.1.0.hyx` (a zip of the component, manifest and folders). Installing unpacks it into `hyscribe/plugins/contact-form-7/` (themes into `hyscribe/themes/<slug>/`), the folder named after the WordPress slug. See the full layout in [DEPLOYMENT.md](DEPLOYMENT.md#folder-layout).

`public/` keeps the original plugin's relative paths, because URLs such as `/wp-content/plugins/contact-form-7/includes/css/styles.css` may already be stored in content and caches; HyScribe maps that URL to `hyscribe/plugins/contact-form-7/public/includes/css/styles.css`.

### Manifest

```toml
[extension]
kind = "plugin"                         # or "theme"
id = "contact-form-7"                  # WordPress slug; also the install folder name
version = "0.1.0"
sdk = "0.1"
replaces = "contact-form-7/wp-contact-form-7.php"   # the WordPress slug it stands in for
replaces_versions = ">=5.8"
compatibility = "partial"
license = "AGPL-3.0-or-later"          # original plugin must be GPL-2.0-or-later (ADR 0004)
name = "Contact Form 7"                 # the plugin's headers, as the Plugins screen shows them
description = "Just another contact form plugin. Simple but flexible."
author = "Takayuki Miyoshi"
author_uri = "https://ideasilo.wordpress.com/"
plugin_uri = "https://contactform7.com/"

[capabilities]
options = ["wpcf7"]                     # option names it may read and write
post_types = ["wpcf7_contact_form"]
mail = true
filesystem = ["uploads/wpcf7_uploads"]  # folders whose files mail may attach

[meta]
keys = ["_form", "_mail", "_messages"]  # a name, or a prefix ending in *

[taxonomies]
read = []                               # taxonomies it reads beyond its own
write = []                              # taxonomies it registers

[rest]
namespaces = ["contact-form-7/v1"]

[transients]                            # SDK 0.2
names = ["wpcf7_*"]                     # a name, or a prefix ending in *
site = []                               # site transients, the same way

[roles]                                 # SDK 0.2
names = []                              # roles it adds and removes, never WordPress's own
capabilities = []                       # capabilities it gives or takes from any role

[http]                                  # SDK 0.2
hosts = ["www.google.com"]              # hosts its requests and their redirects may reach

[hooks]
actions = ["init", "wp_enqueue_scripts"]
filters = ["the_content"]
shortcodes = ["contact-form-7", "contact-form"]
sensitive = ["map_meta_cap"]           # security sensitive hooks: a grant of their own, shown apart (ADR 0022)

[limits]                                # optional: more than the defaults, up to the ceiling (ADR 0022)
call_ms = 100                           # 50 to 500
request_ms = 500                        # 250 to 2000
memory_mib = 32                         # 16 to 64
```

The host only grants what the manifest declares, and the admin shows the capability list before activation. The manifest is read strictly (ADR 0022's 11.3 amendment): a TOML subset, every table and key known, so a key this page does not show, such as `db_tables` for plugin owned tables, refuses the package until the SDK brings it. `sdk = "0.2"` brings `[transients]`, `[roles]` and `[http]` (ADR 0022's 11.4 amendment); a component's imports must be of the SDK its manifest names.

A classic theme's manifest (ADR 0023) says `kind = "theme"`, its `id` the stylesheet slug, with `version`, `sdk` (0.3 or later, the first with the `theme` world), `replaces_versions`, `compatibility`, `license` and the same grant tables; it has no `replaces`, and no `name`, `description`, `author`, `author_uri` or `plugin_uri`, as the Themes screen reads those from the package's `public/style.css` as PHP reads a theme's headers. A component loads as the kind its manifest says: a plugin importing the theme world's interfaces is refused, and a theme without the `theme-guest` export is.

## SDK surface (WordPress equivalents)

`hyscribe-sdk` (`crates/hyscribe-sdk`, Apache-2.0 OR MIT) is a `no_std` crate over the WIT world of ADR 0022: 0.1 since MVP 11.2, 0.2 (11.4) adding transients, roles and outbound requests, 0.3 (10.5) the `theme` world of ADR 0023 for classic themes, and 0.4 (11.4) admin pages and the settings API. A plugin implements `Plugin` (`init`, `activate`, `deactivate`, `invoke`), numbers its callbacks with `callbacks!` and exports itself with `plugin!`, which also gives the component its allocator and comparisons without `std`, so it imports nothing from WASI. The test extension in `crates/hyscribe-ext/guest` drives every call below.

| WordPress | HyScribe SDK |
| --- | --- |
| `add_action`, `add_filter`, `remove_filter`, `do_action`, `apply_filters`, `current_filter`, `did_action` | `hooks::on_action`, `hooks::on_filter` with priority and accepted args (in `init`), `hooks::remove`, `hooks::fire`, `hooks::filter`, `hooks::current`, `hooks::did_action` |
| `add_shortcode`, `remove_shortcode` | `shortcodes::register`, `shortcodes::remove` |
| `register_block_type` | `blocks::register` (a folder of the package, with an optional render callback) |
| `register_post_type`, `register_taxonomy` | `content::register_post_type`, `content::register_taxonomy` (arguments as JSON) |
| `get_option`, `update_option`, `add_option`, `delete_option` | `options::get`, `options::set`, `options::add`, `options::delete` (PHP-serialized under the hood) |
| `get_transient`, `set_transient`, `delete_transient` and the site forms | `transients::get`, `transients::set`, `transients::delete`, `transients::get_site`, `transients::set_site`, `transients::delete_site` (SDK 0.2) |
| `get_metadata`, `add_metadata`, `update_metadata`, `delete_metadata`, `register_meta` | `meta::get`, `meta::add`, `meta::update`, `meta::delete`, `meta::register` |
| `WP_Query`, `get_post` | `query::Query` builder, `query::get_post` |
| `$wpdb` | `db::query` limited to declared tables (SDK 0.2) |
| `register_rest_route` | `rest::route` (the permission callback is required and runs first); the routes are recorded but not served yet |
| `add_menu_page`, `add_submenu_page`, `add_options_page` and its kin, `register_setting`, `add_settings_section`, `add_settings_field`, `settings_fields`, `do_settings_sections`, `submit_button` | `admin::add_menu_page` and the rest by WordPress's names, `admin::SUBMIT_BUTTON` (SDK 0.4, in `init`) |
| `wp_schedule_event`, `wp_clear_scheduled_hook`, `wp_next_scheduled` | `cron::schedule`, `cron::unschedule`, `cron::next` |
| `wp_register_script`, `wp_enqueue_script`, `wp_add_inline_script` and the styles' | `assets::register`, `assets::enqueue`, `assets::add_inline` |
| `wp_mail` | `mail::send` |
| `__()`, `_x()`, `_n()`, `_nx()` | `i18n::translate`, `i18n::translate_with_context`, `i18n::plural`, `i18n::plural_with_context` in the plugin's text domain |
| `get_current_user_id`, `current_user_can`, `wp_create_nonce`, `wp_verify_nonce` | `auth::current_user`, `auth::can`, `auth::create_nonce`, `auth::verify_nonce` |
| `get_role`, `add_role`, `remove_role`, `WP_Role::add_cap`, `WP_Role::remove_cap` | `roles::get`, `roles::add`, `roles::remove`, `roles::add_cap`, `roles::remove_cap` (SDK 0.2) |
| `wp_safe_remote_request` | `http::send` to granted hosts, through the client of ADR 0019 (SDK 0.2) |
| `wp_json_encode` of query variables and arguments | `json::string`, `json::object` |
| `have_posts`, `the_post`, `rewind_posts`, `wp_reset_postdata`, `in_the_loop` | `template::have_posts` and the rest (SDK 0.3, themes) |
| The conditional tags, the template tags that print and those that answer a value | `template::is`, `template::print`, `template::get`, by WordPress's name (`template::print("wp_head", &[])`) |
| `get_header`, `get_footer`, `get_sidebar`, `get_template_part` | `template::get_header` and the rest: the template file the host found, which the theme renders itself |
| `add_theme_support`, `remove_theme_support`, `register_nav_menus`, `register_sidebar`, `add_image_size`, `set_post_thumbnail_size`, `add_editor_style`, `$content_width`, `register_block_style`, `register_block_pattern` | `theme::add_support` and the rest |
| `get_theme_mod`, `set_theme_mod`, `remove_theme_mod` | `theme::get_mod`, `theme::set_mod`, `theme::remove_mod`, over its own `theme_mods_<stylesheet>` |
| `$wp_customize->add_*()` and `remove_*()` | `customize::add`, `customize::remove`, callbacks by number |
| The template files a theme has | `theme::templates`, in `init`; the host renders one through the `render` export of `Theme`, exported with `theme!` |

A classic theme takes the SDK with its `theme` feature, which builds the `theme` world's bindings in place of the plugin world's: the plugin world's interfaces plus `template`, `theme-setup` (ADR 0023's `theme` interface, named apart from the world) and `customize`, and the `theme-guest` export. Template tags are named rather than one WIT function each, so the interface grows by name without a type change; the host answers `invalid` for a name it does not serve. The test theme in `crates/hyscribe-ext/guest/theme` drives every call of the theme world.

### A classic theme's templates

A port's templates are askama templates compiled into its component (ADR 0023); `hyscribe_sdk::page` ties them to WordPress's output. `page::Esc` is the escaper a port names in its `askama.toml` for `.html` templates: `esc_html()`'s behaviour, written for the SDK and checked against PHP's answers (ADR 0023's decisions), so every value prints escaped. Markup the host prints (`page::print("wp_head", &[])`) is `page::Html`, the only value that prints as it is; `page::Text` is host text, printed through the escaper and as nothing when it is not UTF-8. `page::render` renders a template into bytes: `Html`'s bytes are kept aside behind markers and spliced back (`page::splice`, fuzzed by `sdk_template_splice`), so a page is the bytes PHP prints even when they are not UTF-8. A port's tests call `page::check_port` on its folder, which refuses askama's `safe` filter and a marker character in `templates/`, and askama's `Safe`, `HtmlSafe`, `MaybeSafe` or a template's own `escape` in `src/`.

### The SDK's test host

Built natively rather than for `wasm32`, the SDK's functions answer from the test host (`hyscribe_sdk::test`, ADR 0023) instead of the bindings, so a port's own Rust code is built and tested without wasm and the coverage and mutation gates reach it (docs/TESTING.md). `test::Host::<MyPlugin>::new()` gives the thread a fresh site in memory, whose `State` a test fills and reads: options, meta, transients, roles, posts and the loop, queries' answers, theme mods, the template tags' answers, and what the code registered, enqueued, scheduled, mailed or requested. `Host::init` runs the plugin's `init`, registrations taken there only and other calls refused there, as the host does; `Host::filter` and `Host::action` fire a hook as core does, over the plugin's callbacks and those `Host::add` gives core or other extensions; `Host::render` renders a theme's template file. `get_template_part()` and the other part functions find `{slug}-{name}.php` then `{slug}.php` among the files the theme listed, as `locate_template()` does. A call named in `State::fails` answers `SdkError::Failed`, for a port's error paths. Grants are the host's to check, in the golden suites, not the test host's.

### Admin pages and the settings API (SDK 0.4)

- Pages are registered in `init`: a slug of lower case letters, digits, `_` and `-`, under a core menu's file (`options-general.php` for `add_options_page()`, `tools.php`, `themes.php` and the rest) or under one of the extension's own top level pages. hs-admin's menu shows them where PHP's `_wp_menu_output()` puts them, under the URLs PHP gives them without `.php` (`/hs-admin/options-general?page=<slug>`, `/hs-admin/admin?page=<slug>`), and the host checks the page's capability as `user_can_access_admin_page()` does before the callback runs
- The page prints the string its callback answers where PHP runs `do_action( $page_hook )`, inside hs-admin's frame, as an action that prints prints it. Titles and the page's own output are the extension's markup, as in PHP, and escaping what it prints is its own; the markup of `settings_fields()` and `do_settings_sections()` is the host's, the latter handed over in pieces so the SDK runs the extension's section and field callbacks itself
- `register_setting()` needs the option in the manifest's `options` grant, takes a group of the extension's own (none of core's Settings groups) and `type`, `label` and `description`; its sanitize callback is a filter on `sanitize_option_{$option}`. The settings page prints the group's form; its save through `options.php` (the group's nonce, `manage_options`, the sanitize filter, PHP's rows) follows in the next slice and answers 501 until then
- Not yet: `add_users_page()` (its parent depends on the user), hidden pages without a parent, query arguments beside `page`, `add_settings_error()`, a setting's `default`, sections on core's Settings screens, other extensions' callbacks on a page's hook, and the current user and nonces through `auth` on a page

## WordPress hook points in every subsystem

Decision, 2026-11-16:

- Every subsystem exposes WordPress's filter and action points under their WordPress names, with WordPress's argument order, so an existing plugin converts to a HyScribe extension by attaching to the same hook names
- With no extension attached they are plain calls with no cost. Within `hyscribe_model::hooks::scoped`, with the runtime's request as the dispatcher (MVP 11.2), a hook an extension attached to crosses to it as PHP values; the embedder enters the scope for each request it serves, its blocking tasks included (MVP 11.3)
- Security and performance still come first: a hook can never skip a check, and hooks run in bounded time. The grants enforce the first (ADR 0022's 2026-11-25 amendment): a hook that can answer a REST request before its checks or replace what they let through (`rest_pre_dispatch` and the other REST dispatch hooks), that changes a save or an upload after its checks (`rest_pre_insert_{$post_type}`, `wp_handle_upload`), or whose answer decides a check (`registration_errors`, `lostpassword_errors`, `post_password_required` and the like) needs the sensitive grant, which the administrator approves apart; named as a plain action or filter it refuses the package. A hook newly fired before a check joins the host's list in the same change, and any other hook's answer goes through the checks the value it replaces goes through
- Decided so far: images (ADR 0015), mail (ADR 0016), cron (ADR 0017), login (ADR 0018) and REST (ADR 0013)

What crosses in 11.2: a hook whose value and arguments are booleans, integers, floats, strings, PHP values or lists of strings, which covers every `apply_filters()` and `do_action()` call of the login, lost password, registration, personal data and oEmbed paths with such values, and of the subsystems' hook traits `wp_mail_from`, `wp_mail_from_name`, `wp_mail_content_type`, `wp_mail_charset`, `wp_editor_set_quality`, `jpeg_quality`, `image_make_intermediate_size`, `wp_generate_attachment_metadata`, `add_attachment`, `oembed_fetch_url`, `embed_oembed_html`, `wp_feed_cache_transient_lifetime`, `rss_enclosure`, `atom_enclosure`, `img_caption_shortcode`, `img_caption_shortcode_width`, `post_gallery`, `use_default_gallery_style`, `gallery_style`, `wp_{$type}_shortcode_override`, `wp_{$type}_extensions`, `wp_{$type}_shortcode_library`, `wp_{$type}_shortcode_class`, `wp_mediaelement_fallback`, `wp_navigation_should_create_fallback`, `nav_menu_attr_title` and `nav_menu_description`, and since 11.3 `body_class` on the front end and `admin_footer_text` in hs-admin. A hook whose value or arguments are objects (`WP_Post`, `WP_REST_Request`, `WP_REST_Response`, `WP_User`, `WP_Error`, a query's arguments, a mail's arguments, a size list, a term, a menu item, an HTTP request's arguments, an uploaded file, the `authenticate` result) does not cross yet: its value stays as it was until its PHP shape is ported and checked against PHP.

Actions that print (SDK 0.3, ADR 0023's 2026-11-23 amendment): a callback on `wp_head`, `wp_footer` or `wp_body_open` prints the string it answers where the action fires, among core's callbacks by priority (core's first at the same priority, as core adds them first), as a PHP callback's `echo` does; any other answer prints nothing.

## Themes

| Theme type | HyScribe approach |
| --- | --- |
| Block theme | Mostly data: `theme.json`, `templates/*.html`, `parts/*.html`, `patterns/`. HyScribe can render many block themes directly with little or no Rust code; a component is only needed for `functions.php` logic |
| Classic theme | PHP templates are ported to templates (engine to be chosen in its own ADR) following the same template hierarchy; `functions.php` logic becomes a Rust component |
| Child theme | `stylesheet` points to the child, `template` to the parent; lookup falls back child then parent, as in WordPress |

Theme assets (`style.css`, images, fonts, JS) are copied from the original theme into the package's `public/` folder, so an installed theme is self-contained in `hyscribe/themes/<slug>/`. For existing sites, files missing from the package fall back to the original `wp-content/themes/<slug>/`.

## Lifecycle

- `hyscribe plugin install <file>`: checks the signature `<file>.minisig` beside a local `.hyx` package against the hosted registry's key list kept in `registry/` or a key in `EXTENSION_TRUSTED_KEYS` (ADR 0024; without one it needs `--unsigned`), checks the package and loads its component once, keeps the archive as `packages/<slug>-<version>.hyx` with its signature, and unpacks it into `plugins/<slug>/` with `origin.txt` (MVP 11.3). A version the registry revokes, or one signed by a revoked key, gets a `revoked` line in its `origin.txt` and stops loading. `hyscribe theme install` follows in 10.7
- `hyscribe plugin delete <id>`: removes `plugins/<id>/` (files, approval and `faults.txt`) and the plugin's kept packages; a plugin `active_plugins` names is refused with a note to deactivate it first, as the Plugins screen refuses to delete an active plugin, and nothing changes. The Plugins screen itself never deletes (`delete_plugins` is never granted). The command writes nothing to the database: PHP's `delete_plugins()` touches only the PHP plugin's own `uninstall_plugins` entry and update transient, which belong to the plugin that stays installed for a switch back
- The Plugins screen lists the installed extensions as PHP lists the plugins they stand in for, and shows what each asks for before activation, where an unsigned or operator-signed package came from, and, from the kept registry index, an update row in PHP's markup with the command that installs the update, or an error row for a revoked version (ADR 0024); activating approves it (`approved.ext.toml` in its folder), deactivating takes the approval back, and an update that asks for more needs activating again
- Activation writes to the same WordPress options (`active_plugins`, `template`, `stylesheet`) so that PHP WordPress sees the original plugin or theme as active when switched back: `active_plugins` gets the manifest's `replaces` file, as PHP writes it
- Activation and deactivation hooks map to the plugin's own `activate` / `deactivate` exports, run where PHP fires `activate_{$plugin}` and `deactivate_{$plugin}`, between `activate_plugin` and `activated_plugin` (and their deactivation pairs), their database work in the action's transaction; an activation whose component does not load or whose export fails leaves the plugin inactive, as a fatal error leaves a PHP plugin (ADR 0022's 11.3 amendment)
- Each request runs the extensions `active_plugins` names that are installed and approved, with the grants of their approval, then the active theme's component (ADR 0023): the one installed in `themes/<stylesheet>/` and approved there, a child running only with its parent (`template`) installed beside it; each is compiled once per installed version. A theme renders its templates in an instance of its own, beside the one answering its callbacks, both under its one budget
- Upgrades swap the component atomically; requests in flight finish on the old instance
