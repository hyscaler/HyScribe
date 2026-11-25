# 0022: The extension host

- Status: Accepted (2026-11-20)
- Date: 2026-11-20 (drafted 2026-11-18)

## Context

ADR 0002 chose WebAssembly components under wasmtime for plugins and themes; the plugin work was put on the road map on 2026-11-18 (MVP phase 11) with Elementor, Yoast SEO and Contact Form 7 as the reference plugins. This ADR is 11.1: how the host embeds wasmtime, the WIT world of SDK 0.1, how a hook crosses into a component and back in bounded time, what a capability grants, and what the three plugins need that 0.1 cannot give them. Risk R7 (the cost of a call into a component on hot filters such as `the_content`) is measured, not estimated.

The spike is `crates/hyscribe-ext`, kept as the seed of the runtime crate. It is a workspace member with its own tests but no dependency of the `hyscribe` binary yet. It loads a 39 KB guest component built from source for `wasm32-wasip2` (`crates/hyscribe-ext/guest`, built by its `build.sh` when the tests run), checks every hook the guest adds and every option and cron hook it touches against its grants, runs its `the_content` filter in a fresh instance, and shows a trap, an endless loop and a memory grab each stopped while the request keeps its value. Its options and cron are stand-ins kept in the instance, there to exercise the grant checks and the boundary, not storage.

The draft left ten questions, all decided on 2026-11-20; the decisions are written into the sections below.

## Decision

### wasmtime as a dependency

- `wasmtime = "=50.0.2"`, the newest release (2026-11-16), pinned exactly and moved to each patch release as it comes out, since wasmtime's patch releases carry its security fixes
- Features: `component-model`, `cranelift`, `runtime`, `std`, with `default-features = false`
- Off: Winch (never on by default), `async` and `component-model-async` (no fibers), `cache` (no compiled code on disk), `wat`, `gc` and its collectors, `threads`, `stack-switching`, `pooling-allocator`, `parallel-compilation`, `profiling`, `coredump`, `debug`, `debug-builtins`, `addr2line`, `demangle`, `wit-parser` and `compile-time-builtins`
- Off by leaving the crates out: `wasmtime-wasi` (no WASI preview 1 or 2 at all) and `wasmtime-wasi-http`
- `runtime` still compiles the Pulley interpreter crate, which wasmtime only selects on targets Cranelift does not support; x86_64 and aarch64 use Cranelift
- Components are compiled by Cranelift in the process when a package is loaded, and the machine code lives in memory only. No `.cwasm` is ever read back: deserialising precompiled code would be running a file from disk (SECURITY_MODEL.md)
- `hyscribe-ext` forbids `unsafe` like every crate; wasmtime's derive macros for component types compile under `forbid(unsafe_code)`, so the host side has none
- Fuzz target `ext_load` loads hostile bytes as an extension and runs what loads, so the host's own path around wasmtime's parser and compiler (refusals, linking, the grant checks of `init`, the limits) is fuzzed with them

### The supply chain

cargo deny passes without changes to `deny.toml`: every new crate is under an allowed licence (wasmtime and Cranelift are `Apache-2.0 WITH LLVM-exception`), and the multiple versions it warns about are the ones already there. For cargo vet, as ADR 0019 did for the TLS crates, no exemption is added:

- Covered by the imported Bytecode Alliance set: every `wasmtime-*`, `cranelift-*` and `pulley-*` crate, `wasmparser`, `wasm-encoder`, `wasmprinter`, `wit-parser`, `wit-component`, `wasm-metadata` and the rest of their tree that the set audits
- Trusted publishers, each trusted by the Bytecode Alliance's, Mozilla's or ISRG's imported sets (rust-lang-owner already trusted here for `cc`), ending 2027-11-18: David Tolnay (`anyhow`, `rustversion`), Nick Fitzgerald (`arbitrary`, `id-arena`), Philip Craig (`addr2line`, `gimli`, `object`), Dan Gohman (`target-lexicon`), Andrew Gallant (`termcolor`, `winapi-util`), rust-lang-owner (`libm`, `rustc-demangle`) and the `JohnTitor/mach2` team (`mach2`). `async-trait` needs no entry: lettre's audit of 0.1.92 covers it
- Audited by HyScribe rather than their publishers trusted, as no imported set trusts them: `itertools` 0.14.0 (14,505 lines, used by wasmtime's Cranelift glue; eight unsafe sites, all sound) and `cpp_demangle` 0.5.1 (about 13,200 lines, only behind wasmtime's `demangle` feature, which is off, but in the lockfile; two unsafe sites, sound, and its parsing bounded by recursion limits). The notes in `supply-chain/audits.toml` say what was read
- Also audited by HyScribe: the `memfd` 0.6.5 to 0.6.6 delta (packaging only) and the `rustc-hash` 2.1.1 to 2.1.3 delta (safe refactors)
- `regalloc2` is held at 0.15.2, in the fuzz lockfile too: 0.15.3 was published the day before the draft and no set had looked at it yet
- The guest fixture's own dependencies (`wit-bindgen`, `dlmalloc`) live in its own lockfile and are not in the binary; the SDK (11.2) brings `wit-bindgen` under cargo vet for the extensions HyScribe builds itself

### The WIT world of SDK 0.1

The interfaces are in `crates/hyscribe-ext/wit/sdk.wit`, licensed Apache-2.0 OR MIT, as the package `hyscribe:sdk@0.1.0`:

| Interface | WordPress | Calls |
| --- | --- | --- |
| `types` | PHP values | `value` (its kind, then an integer, a float and a text field, the one its kind reads; arrays and objects as JSON text), `sdk-error` (denied, invalid, failed), `object-kind` |
| `hooks` | `add_action`, `add_filter`, `remove_*`, `apply_filters`, `do_action`, `current_filter`, `did_action` | `add` (hook name, kind, callback number, priority, accepted args), `remove`, `chain` and `run-step`, `current`, `did-action` |
| `options` | `get_option` and friends | `get`, `update`, `add`, `delete` |
| `meta` | `get_metadata` and friends, `register_meta` | for posts, users, terms and comments |
| `content` | `register_post_type`, `register_taxonomy` | arguments as JSON |
| `query` | `WP_Query`, `get_post` | query variables as JSON in, typed `post` records out |
| `rest` | `register_rest_route` | a route with its callback, a required permission callback and its `args` schema |
| `shortcodes` | `add_shortcode`, `remove_shortcode` | |
| `blocks` | `register_block_type` | from the package's `block.json`, with an optional render callback |
| `assets` | `wp_register_script`, `wp_enqueue_style`, `wp_add_inline_script` | files of the package's `public/` folder |
| `cron` | `wp_schedule_event`, `wp_unschedule_hook`, `wp_next_scheduled` | |
| `mail` | `wp_mail` | attachments only from granted folders |
| `auth` | `get_current_user_id`, `current_user_can`, `wp_create_nonce`, `wp_verify_nonce` | |
| `i18n` | `__`, `_x`, `_n`, `_nx` | in the extension's own text domain, from its `languages/` folder |
| `guest` (export) | the plugin itself | `init`, `activate`, `deactivate`, `invoke( callback, args )` |

- The `plugin` world imports these interfaces and exports `guest`. It imports nothing else: no WASI, so a component has no clock, file, socket, environment or random source but what these interfaces hand it. A component that imports anything else fails to link and is refused at load
- The `theme` world is the plugin world for now; phase 10's classic theme ADR (10.4) adds what `functions.php` logic needs
- The `spike` world (hooks, options and cron) is what the spike's guest is built against
- `value` is a tuple of a kind and three fields rather than a WIT variant: one layout for every kind, so the host lowers a string where it lies without copying it, and wasmtime lifts it with its own tuple code. The SDK wraps it in a Rust enum
- `esc_html()`, `esc_attr()`, `sanitize_*()` and the other pure functions are Rust in the SDK, run inside the component, never host calls; `wp_kses()` is a host call in 11.2, since its allowed HTML depends on the site and its filters
- Versioning: the package version is the SDK version. A component names the version it was built against; the host links every 0.1.x component against its 0.1 interfaces, a new minor version only adds interfaces or functions, and a breaking change is a new major version linked beside the old one. The manifest's `sdk = "0.1"` must match the component's imports, or the package is refused
- An older major version keeps linking for 12 months after the release that ships the new one. Throughout those 12 months the Plugins screen shows a deprecation notice on each extension built against it, with the date it stops loading; after that, such a package is refused at load like any other that does not link, and the screen says why

### How a hook crosses into a component and back

- Registration: `guest.init` runs once when a package is loaded, in an instance of its own, and adds hooks with `hooks.add`. The host keeps the registrations in the extension's place in each hook's chain, by priority and order added, beside core's own callbacks and other extensions'. `hooks.add` called after `init` is refused (the spike's `adds_hooks_in_init_only`)
- Callbacks are numbers. The SDK's hook macros give each Rust function a fixed number at compile time, so a fresh instance can run any callback without running `init` again
- Firing: when core runs `apply_filters( 'the_content', $content )` and an extension has a callback there, the host lowers the value into the extension's instance and calls `guest.invoke( callback, [ value ] )`, passing at most the callback's accepted args, and lifts the answer as the new value
- An extension firing a hook (`apply_filters( 'wpcf7_form_elements', ... )`) cannot have its own callbacks called back by the host while it is running: a component instance cannot be entered twice. `hooks.chain` therefore gives it the hook's chain with its own callbacks by number and every other run of callbacks as one step; the SDK runs its own callbacks itself and hands each other step to `hooks.run-step`, so priorities interleave exactly as in WordPress
- Values: scalars and strings cross as they are (`value`); arrays and objects as JSON (`wp_json_encode()`'s shape), objects such as `WP_Post` as their fields. A string crossing is copied and checked as UTF-8 each way, which is most of the cost at large sizes (below)

### Bounded time and memory

- Epoch interruption, not fuel. A ticker thread advances the engine's epoch every millisecond, and each call into a component is given a deadline in epochs. Fuel counts instructions and costs on every block executed; epochs cost one load and compare at loop headers and function entries, and bound wall clock time, which is what a request needs
- The defaults: 50 ms a call (50 to 51 ms with the tick), 250 ms an extension a request (the sum of its calls in that request; once spent, the extension runs nothing more in the request, as after a timeout), and 16 MiB of linear memory an instance, refused past it by a resource limiter on each store; tables are capped at 10,000 elements
- Wasm stack: 512 KiB a call, wasmtime's default, set explicitly so the limits hold it in one place. The stack and the table cap cannot be asked for
- Calls are synchronous and run where the request already runs its work, on a `spawn_blocking` thread (rendering, REST and the admin already do); without the `async` feature there are no fibers. A deadline bounds how long one thread is held

### Asking for more: the manifest's `[limits]` and their ceiling

An extension that needs more than the defaults asks in its manifest, and the administrator approves the request at activation like any grant:

```toml
[limits]
call_ms = 200        # 50 (the default) to 500
request_ms = 1000    # 250 (the default) to 2000
memory_mib = 32      # 16 (the default) to 64
```

- The fixed ceiling is 500 ms a call, 2,000 ms an extension a request and 64 MiB an instance: ten, eight and four times the defaults. It is a constant of the host, not a setting
- Each value is an integer from its default to its ceiling. A value past the ceiling, under the default, not an integer, or an unknown key refuses the package at install, with the reason; nothing is clamped, so an extension never runs with less than it was built to expect without saying so
- The Plugins screen shows the requested limits beside the grants before activation; until they are approved the extension does not run at all, and an update that raises any of them needs approval again, as a capability increase does (SECURITY_MODEL.md)

Why this ceiling:

- Request time: `REQUEST_TIME_LIMIT` ends a request at 30 seconds. At 2 seconds each, the three reference plugins at their ceilings cost at most 6 seconds of a request, and even ten extensions at theirs leave a third of the limit to core; 2 seconds is also already a page a visitor sees as broken, so a port that needs more has a design problem a larger budget would hide
- A call's ceiling is a quarter of the request's, so one runaway callback cannot spend an extension's whole budget at its first hook. 500 ms is far above the measured costs below (4.4 µs for 100 KiB through `the_content`), and leaves room for work that is heavy by nature, such as building a large page's CSS on its first view
- Memory: what instances can hold is the requests in flight times the active extensions times the limit, as nothing is shared between requests. 64 MiB is the size the host already allows for one hostile input elsewhere (one entry of a `.hyx` package below, the text the block parser and the Interactivity API copy), so an extension can hold any input the host accepts at least once; a 100 KiB `_elementor_data` and its widget tree fit in the default many times over. At 64 MiB, 16 requests in flight through one such extension hold at most 1 GiB, of which only the pages touched are resident

### When an extension traps or overruns

- The call ends with a trap: the guest's own (`unreachable`, a panic, a bad memory access), the epoch deadline (`Trap::Interrupt`), the memory limit, or the extension's request budget running out
- The value stays as it was before the failing callback; the instance is dropped and the extension runs nothing more in this request; core's callbacks and other extensions' carry on, and the request answers as if the extension had no further callbacks
- The fault is logged with the extension, the hook, the callback and its kind (trap, timeout, memory), and counted. A callback that answers an error of its own (`Err` from `invoke`) is logged and keeps the value, but the instance carries on and it does not count
- After 3 counted faults within 10 minutes the extension is disabled: it is not instantiated again until an administrator re-enables it from the Plugins screen, which shows why
- The spike tests each case: `a_trap_ends_the_request_and_enough_disable`, `a_loop_is_stopped_at_its_deadline` (a 20 tick deadline on an endless loop), `memory_stops_at_its_limit` (a 64 MiB allocation against 16 MiB), `keeps_the_value_when_the_guest_answers_an_error`, and `a_guest_that_traps_while_loading_is_refused`. The spike counts faults in memory without the 10 minute window, and has no request budget or fault file; the runtime (11.2) adds the three

### Where the fault state lives: `faults.txt`

The state is kept in the extension's own folder, `hyscribe/plugins/<slug>/faults.txt` (`hyscribe/themes/<slug>/faults.txt` for a theme), never in the database: `active_plugins` is untouched, so PHP WordPress keeps the original plugin active on a switch back, and nothing PHP would not write is written (ADR 0003).

```
fault 1795161600 timeout the_content
fault 1795161642 trap wpcf7_form_elements
fault 1795161700 memory the_content
disabled 1795161700
```

- UTF-8 text, one record a line, each line ended by a line feed, fields separated by one space. `fault <unix seconds> <trap|timeout|memory> <hook>` records a counted fault; `disabled <unix seconds>` records when the extension was disabled
- It keeps only the last 3 faults, oldest first, since only they decide; `disabled` follows them when the third came within 600 seconds of the first. The file is absent while the extension has no counted fault, and re-enabling from the Plugins screen (`activate_plugins`, a nonce, a log line) deletes it
- It is at most 4 lines and 4 KiB; a hook is a name the manifest grants (printable ASCII, no space, at most 255 bytes), and a time is an integer of at most 20 digits
- Written whole, never appended: a new file in the same folder with mode `0600`, flushed and renamed over the old one, under the extension's lock, so a crash leaves the old state or the new one. The host reads it once when it loads the extension and keeps the count in memory after that
- A file that does not read as above counts as disabled: the host fails closed, logs it, and the Plugins screen says the fault record is unreadable and offers to re-enable. Its reader gets a fuzz target (`ext_faults`) when it lands in 11.2
- The package format (below) allows nothing else at the top of a package, so no package can carry a `faults.txt`; installing an update keeps the file, and a disabled extension stays disabled until an administrator re-enables it

### Instance lifetime and concurrency

- One `Engine` per process. Each package is compiled once into an `InstancePre`, linked against the host functions once
- One fresh instance per extension per request, made the first time a hook reaches that extension in the request, dropped at the end of it; no instance is pooled or reused across requests. Nothing in an instance's memory survives a request, so nothing one visitor's request leaves there can reach another's, as PHP's share nothing model behaves; state across requests goes through options, meta and the database. Making one costs 5.3 µs (below)
- Requests run concurrently, each with its own stores; an instance is never shared between threads. Memory in use is at most the requests in flight times the active extensions times their memory limits, which is why the default is small
- `init`, `activate` and `deactivate` each run in an instance of their own
- Updating a package swaps its `InstancePre`; requests in flight finish on the old one (EXTENSIONS.md)

### Capabilities

- The manifest's `[capabilities]`, `[hooks]` and `[limits]` are the grants, read when the package is installed and shown before activation; a grant the administrator has not approved does not exist
- Deny by default: every host function checks its grant in the store's state before it touches anything, and answers `sdk-error.denied` otherwise. Nothing is checked once and cached in the guest
- What each grant covers: hooks by name (`[hooks]` actions, filters, shortcodes); option names; meta keys (exact, or a prefix ending in `*` such as `_yoast_wpseo_*`); post types and taxonomies it registers, and any others it reads; its own REST namespace; cron hooks; mail; folders under `uploads/`; outbound hosts (0.2). Capability checks inside the extension (`current_user_can`) are the user's capabilities, never more
- Security sensitive hooks have a grant of their own: `authenticate`, `determine_current_user`, `user_has_cap`, `map_meta_cap`, `rest_authentication_errors`, `wp_kses_allowed_html`, `upload_mimes`, `wp_mail` and its filters, and `pre_option_*`, `option_*` and `pre_update_option_*` for any option outside the extension's own options grant. The host keeps this list; a manifest names them under `[hooks] sensitive`, and one named only under `actions` or `filters` refuses the package. The Plugins screen lists them apart, with a warning that the extension can change who is logged in, what they may do or what is filtered, before activation; an update that adds one needs approval again
- The spike's grants are hooks (cron hooks among them) and options by name: an empty grant refuses the guest's first `hooks.add`, a grant missing one refuses that one, and `init`'s error refuses the package (`grants_nothing_by_default`); `options.update( 'siteurl' )` and `cron.schedule` on `wp_version_check` answer `denied` (`options_are_checked_against_their_grant`, `cron_hooks_are_checked_against_their_grant`)

### The cost per call, measured (R7)

`cargo test --release -p hyscribe-ext --test cost -- --ignored --nocapture`, through the gates. The native column is the same filter (a clone and an append) as a Rust closure; the component column includes the clone, lowering and lifting the string, the epoch deadline and the type check of `invoke`:

| `the_content` input | Native, per call | Component, per call | 10,000 calls |
| --- | --- | --- | --- |
| 0 bytes | 0.01 µs | 0.18 µs | 1.8 ms |
| 1 KiB | 0.04 µs | 0.27 µs | 2.7 ms |
| 10 KiB | 0.12 µs | 0.62 µs | 6.2 ms |
| 100 KiB | 0.85 µs | 4.37 µs | 44 ms |

- A fresh instance per request: 5.3 µs
- A hook the extension has no callback on: 0.01 µs (the host's own lookup, no crossing)
- A page runs `the_content` once per post shown, a few dozen times at most: well under a millisecond with a component on it. R7 is not a risk at this size; no batching is needed for 0.1
- The cost grows with the bytes crossing (copy and UTF-8 check both ways), so the hooks that would hurt are those called thousands of times a page: `gettext` and its variants (every translated string), `esc_html` and `attribute_escape`, `option_*`, `get_*_metadata`. Translation is done by the host from the package's `languages/` (i18n), so the SDK never fires `gettext` into components unless one declares it; at 0.2 µs a call, 5,000 `gettext` calls would cost 1 ms
- Batching, if a later measurement needs it: an `invoke-many( callback, list<list<value>> )` export for filters core runs over a list in one loop (`the_title` over a query's posts), and the SDK's `chain` handing the guest a run of its own callbacks at once. Neither changes WordPress's order

### The spike's guest is built from source

- The guest is never committed as a `.wasm`. `crates/hyscribe-ext/guest/build.sh` builds it with `cargo build --release --target wasm32-wasip2 --locked` into `spike-guest/` under the target folder and prints the component's path; the host's tests run it once per test binary and read what it built, so a test always runs against the source it sits beside
- The script clears the host build's flags and wrappers (`RUSTFLAGS` and its encoded form, which carry coverage instrumentation and the host's linker) so the guest builds the same under `cargo llvm-cov`, cargo-mutants and the gate runs
- The `wasm32-wasip2` target is on every machine that runs the tests: `scripts/ci-setup.sh` adds it in CI (docs/TESTING.md)
- The guest is `no_std` with `dlmalloc` and its own `cabi_realloc`, so its component imports nothing from WASI; that one `unsafe` function is guest code, inside the sandbox and outside the workspace, under Apache-2.0 OR MIT like the SDK

### The package format `.hyx`

The parser and its fuzzing are 11.3; the format is fixed here:

- A zip archive: stored or deflate entries only, no encryption, no zip64, no symbolic links, UTF-8 names, no duplicate names, no absolute paths, no `..`, no `\`, no names differing only in case
- Limits: at most 10,000 entries, 256 MiB uncompressed in total, 64 MiB for one entry, a compression ratio of at most 100 to 1 for any entry; each checked from the central directory and again while inflating
- Layout: `hyscribe.ext.toml` (the manifest), `extension.wasm` (the component, absent for a block theme that is only data), `public/`, `languages/`, `blocks/`, `templates/`, `LICENSE`; nothing else at the top
- The manifest is TOML, read with unknown keys refused: `[extension]` (`kind`, `id` as the WordPress slug, `version`, `sdk`, `replaces`, `replaces_versions`, `compatibility`, `license`), `[capabilities]` and `[hooks]` as EXTENSIONS.md shows (with `sensitive` above), and `[limits]` as above. Its parser gets a fuzz target with the package's (11.3)
- `license` must be GPL compatible for a port (ADR 0004); a GPL-2.0-only original is refused
- Signing and where packages come from are decided separately

### The reference plugins on SDK 0.1

Read from their wordpress.org releases (Contact Form 7 6.2.5, Yoast SEO 28.9, Elementor 4.4.1). None is GPL-2.0-only: Contact Form 7 is GPLv2 or later, Yoast SEO and Elementor GPLv3 or later.

**Contact Form 7** (36k lines, 94 actions and 49 filters added, about 140 of its own `wpcf7_*` hooks fired). Its post type `wpcf7_contact_form`, its option `wpcf7`, its post meta (`_form`, `_mail`, `_messages`), its REST namespace `contact-form-7/v1` (the form's submission is `POST .../feedback`), the shortcodes `contact-form-7` and `contact-form`, its block, assets, `wp_mail` with attachments and its `wpcf7_*` meta capabilities through `map_meta_cap` (a sensitive hook, so its own grant) all map onto 0.1. Missing from 0.1:

- Uploaded files: multipart request bodies, and writing them into `uploads/wpcf7_uploads/<random>/` with cleanup
- Outbound HTTP for reCAPTCHA, Turnstile, Akismet and its mail list integrations (optional modules)
- Its admin screens (5 pages) and 3 `wp_ajax_*` handlers
- `phpmailer_init`, which hands over the PHPMailer object; its uses become `wp_mail` filters

Verdict: portable on 0.1 plus uploads, admin pages and an outbound allowlist.

**Yoast SEO** (209k lines, 415 actions and 181 filters added, about 420 hooks of its own). Its options (`wpseo`, `wpseo_titles`, `wpseo_social`), its post meta (`_yoast_wpseo_*`), 71 REST routes under `yoast/v1`, cron and its blocks map onto 0.1. Missing:

- Its own tables (`wp_yoast_indexable`, `wp_yoast_indexable_hierarchy`, `wp_yoast_migrations`, `wp_yoast_primary_term`, `wp_yoast_seo_links`), built by 27 migrations and read through an ORM; the head it prints comes from them, so they are not optional
- Rewrite rules and query variables for its sitemaps (`sitemap_index.xml`, `*-sitemap.xml`), answered from `template_redirect`
- Output buffering of the whole page (`ob_start` on `template_redirect`, rewritten on `wp_footer`) for its forced title rewrite, which HyScribe refuses (below), so that option does nothing in the port
- Removing core's `wp_head` callbacks by name (`rel_canonical`, `_wp_render_title_tag`, `noindex`), `wp_robots` and `pre_get_document_title` at extreme priorities: hooks, so 0.1, once core's callbacks are named in the chain
- Roles and capabilities it adds (`wpseo_manager`, `wpseo_editor`, `add_cap`), and `user_has_cap` filters (sensitive)
- Writing `llms.txt` into the site root; outbound HTTP to MyYoast, Wincher and Semrush; 18 admin pages and about 19 `wp_ajax_*` handlers

Verdict: not portable on 0.1. It needs plugin owned tables with typed schemas and migrations, rewrite rules and query variables, roles, and admin pages.

**Elementor** (331k lines and 69 MB of assets, 619 actions and 281 filters added, about 520 hooks of its own). Its post types (`elementor_library`, `e-landing-page`, `e-floating-buttons`), taxonomies, the page as JSON in post meta (`_elementor_data`, `_elementor_edit_mode`, `_elementor_page_settings`, `_elementor_css`), about 40 options, 58 REST routes, its `the_content` filter at priority 9 and assets map onto 0.1: the page data is ordinary meta, and a 100 KiB `_elementor_data` crosses in about 4 µs. Missing:

- The editor: a full screen at `post.php?action=elementor` (`admin_action_elementor`), which empties `wp_head` and rebuilds it, and a JavaScript app talking to `wp_ajax_elementor_ajax` and 17 other `wp_ajax_*` handlers. SDK 0.1 has no admin screen and no admin-ajax
- Widgets rendered by a PHP class hierarchy (36 classic and 24 atomic widgets) that add-ons extend by subclassing: inside one component the port can keep a Rust trait hierarchy, but third party Elementor add-ons cannot plug into it, so the add-on ecosystem does not carry over
- Generated CSS written to `uploads/elementor/css/` (avoidable: its `elementor_css_print_method` option `internal` prints the CSS in the page instead)
- Page templates through `template_include` (`elementor_canvas`, `elementor_header_footer`)
- Its table `wp_e_events`, direct SQL for its URL replacement tool, 37 `ob_start` calls, a must-use plugin written by safe mode, 255 outbound HTTP calls to its library, connect and AI services

Verdict and order: the front end comes first, rendering pages from `_elementor_data` on 0.1 plus page templates. The editor follows: its JavaScript app is served as is from the package's `public/`, and HyScribe answers its `admin-ajax.php` actions (`elementor_ajax` and the other 17) as PHP answers them, checked against PHP like every other admin action, on SDK 0.2's admin pages and admin-ajax handlers.

### SDK 0.2

What SDK 0.1 lacks across the three plugins comes in 0.2 in this order:

1. Admin pages and the settings API, with admin-ajax handlers
2. Uploads folders and multipart request bodies, writing only into granted `uploads/` folders
3. Outbound HTTP to granted hosts through `hyscribe-fetch` (ADR 0019)
4. Page templates (`template_include`)
5. Rewrite rules and query variables
6. Plugin owned tables with typed schemas and migrations (never string-built SQL)
7. Roles and capabilities (`add_role`, `add_cap`)
8. Transients

Refused rather than added: output buffering of the whole page (it turns off Yoast's forced title rewrite, the only use among the three that has no hook to replace it), `phpmailer_init`'s object, and writing outside `hyscribe/`.

## Consequences

- `hyscribe-ext` becomes the runtime of 11.2; the binary does not depend on it until then. 11.2 adds the per request budget, the 10 minute window, `faults.txt` and its fuzz target, the `[limits]` ceiling and the sensitive hooks' grant to what the spike shows
- A component can do nothing outside its grants and the SDK's interfaces, and a misbehaving one costs a request at most its deadline, its request budget and its memory limit, none of them past the fixed ceiling
- A disabled extension leaves the database as PHP WordPress expects it; switching back to PHP brings the original plugin back active
- Hooks into components are cheap enough (0.2 to 0.6 µs for common sizes) that no hook point needs a native fast path for 0.1
- WordPress semantics that rely on PHP's shared process (output buffering, globals, subclassing another plugin's classes, writing PHP files) do not carry over; ports replace them or go without, and the compatibility report says which
- The Bytecode Alliance's crates arrive under their imported audits, 13 publishers are trusted until 2027-11-18 and need renewing, `itertools` and `cpp_demangle` need a new audit (or a delta) at each version wasmtime moves them to, and every wasmtime release is followed
- An extension built against an old SDK major keeps working for 12 months after a new major ships, which bounds how long the host carries two sets of interfaces
- Building an extension, and running `hyscribe-ext`'s tests, needs the `wasm32-wasip2` target; the SDK must keep components free of WASI imports, as the spike's guest is

## Amendment: the runtime and SDK 0.1 as built (MVP 11.2, 2026-11-20)

- `Host` (the engine, the linked SDK, the ticker), `Host::load` (the grants checked, `faults.txt` read, the component compiled and its `init` run once), `Extensions` (the active set and each hook's callbacks by priority, then load order, then the order added) and `Request` (a fresh instance per extension, made at its first callback). A call's deadline is the smaller of its `call_ms` and what is left of the extension's `request_ms`; a call cut by either is a timeout, and counts
- Registrations (`hooks.add`, post types, taxonomies, `register_meta`, REST routes, shortcodes, blocks) are taken in `init` only and refused after it; every other host call answers `failed` in `init`, which has no request, and needs its grant in a request. `hooks.remove` and `shortcodes.remove` remove the extension's own for the rest of the request
- `hooks.run-step` takes the answer of a callback added as a filter and not of one added as an action, as the host cannot tell whether the extension runs `apply_filters()` or `do_action()`; an extension is never entered twice, so a chain fired while it runs skips its callbacks. Firing a sensitive hook from an extension needs its sensitive grant
- The grants carry meta keys, taxonomies, REST namespaces and the sensitive hooks beside EXTENSIONS.md's manifest keys; the manifest's names for them are the lifecycle's (11.3). A query names its post types (`post` when it names none): public ones and granted ones are read, `any` is refused. A REST route's permission callback runs first, and its callback only on `true`
- The records `rest.route`, `assets.asset`, `mail.message`, `query.post` and `query.found` cross as wasmtime `Val`s rather than through its record derives, whose error paths no WIT shape reaches and which the coverage gate would count; enums and variants keep the derives
- The spike world and its guest are gone: the test extension `crates/hyscribe-ext/guest` is built on `hyscribe-sdk` by `build.sh` in a clean environment. `plugin!` gives a component `memcmp` and `bcmp`, which `core` needs without libc
- The subsystems' hook points cross within `hyscribe_model::hooks::scoped`, the request as the dispatcher, for the values EXTENSIONS.md lists; hooks of object values (`WP_Post`, `WP_REST_Response` and the others) wait for their PHP shapes, checked against PHP

## Amendment: the lifecycle as built (MVP 11.3, 2026-11-21)

- The manifest is read by the host's own reader of a strict TOML subset (`hyscribe_ext::manifest`): tables of bare keys whose values are basic or literal strings on one line, decimal integers, booleans or arrays of strings, and comments; at most 64 KiB of UTF-8. Dotted keys, inline tables, arrays of tables, floats, dates and multi-line strings are refused with their line, as is any table or key but those below. No TOML crate is taken: `toml_edit` and its parser crates are in the tree only as build tools, audited `safe-to-run`, and a subset this small is easier to bound and fuzz than to audit them for `safe-to-deploy`
- `[extension]` also names what the Plugins screen shows of the plugin, as PHP shows its headers: `name` (required), `description`, `author`, `author_uri` and `plugin_uri`. A plugin's `replaces` is required and is a file in its own folder, `<id>/<file>.php`: the entry activation writes into `active_plugins`. `version` holds letters, digits, `.`, `+`, `_` and `-` only, as it names the package kept. A port (one that `replaces`) under `GPL-2.0-only` is refused (ADR 0004)
- The grants set: `[meta] keys`, `[taxonomies] read` and `write` (registering is `write`; SDK 0.1 has no call that reads terms, so `read` grants nothing yet) and `[rest] namespaces`, beside `[capabilities]` (`options`, `post_types`, `mail`, `filesystem`) and `[hooks]`. SDK 0.2's keys (`db_tables`, `http_outbound`) are refused until 0.2 brings them
- The package is read by the host's own zip reader (`hyscribe_ext::hyx`) over `miniz_oxide` (raw deflate) and `crc32fast`, both already in the tree and audited `safe-to-deploy`: no archive comment, no second disk, every size and count of the central directory checked against the limits above before anything is inflated, each entry checked against its local header, inflated to exactly its size and its CRC. Folder entries are allowed; a name that is both a file and a folder is refused. Fuzz targets `ext_manifest` and `ext_package`
- `hyscribe plugin install <file>` takes a local file only (distribution is open): the package is read, its component loaded once with its grants and its `init` run, so a package that does not load changes nothing; then it is kept as `packages/<id>-<version>.hyx` and unpacked into `plugins/.<id>.new`, which is swapped in for `plugins/<id>/`, keeping the older one's `faults.txt` and approval. A signature check, when one is decided, goes before the package is read
- Approval lives in the plugin's folder, never in the database (ADR 0003): `approved.ext.toml`, the manifest as the administrator approved it. Activation writes it and deletes `faults.txt` (activating again is how a disabled extension is re-enabled); deactivation deletes it. An extension whose installed grants or limits differ from those approved (an update asking for more) is listed as needing approval, and does not run until it is activated again; the package format allows nothing at the top of a package that could carry this file
- Activation and deactivation write `active_plugins` and `recently_activated` as PHP's `plugins.php` does, row for row (write oracle `admin-plugins-*`): the `replaces` file added and the list sorted, or removed with the other entries' keys kept; a view of the Plugins screen keeps `recently_activated` to the last week, as `prepare_items()` writes it. Switched back, PHP loads the plugin the extension stands in for where it is installed, skips one it does not have, and the site's own plugins stay active (switch-back suite)
- The Plugins screen lists the installed extensions as PHP lists the plugins they stand in for, with HyScribe's own part of a row, where `after_plugin_row_meta` prints, saying what an extension asks for until its activation approves it. Deleting is refused as PHP refuses it on a site where files never change from the browser (`delete_plugins` is never granted, docs/ADMIN.md); a plugin is removed from the command line, `hyscribe plugin delete <id>` (2026-11-21): refused while `active_plugins` names one of its files, otherwise its folder is renamed aside (`plugins/.<id>.deleting`, which never loads) and removed, then its kept packages (those of `packages/<id>-*.hyx` that name this id), links removed themselves and never followed; a failure is reported after the rest is done and running the command again finishes it. It only reads `active_plugins`. PHP's `delete_plugins()` writes `uninstall_plugins` and the `update_plugins` transient only for the PHP plugin it deletes, and that plugin stays in the PHP tree for the switch back, so HyScribe writes nothing, and `recently_activated` is not touched by PHP either
- The embedder enters the hook scope for each request: `hyscribe_model::hooks::within` sets the request's dispatcher on the thread for each poll of its future only, so tasks sharing a worker thread keep their own, and `hooks::carry` takes it into each blocking task the request hands work to. The request's extensions are those `active_plugins` names that are installed, approved and load, in its order, from `hyscribe_ext::Runtime`: it makes the host at the first load, loads each extension once per installed version (the device, inode, size and time of its manifest, its approval and its component, so an install or an approval is a new version) with the grants and limits of `approved.ext.toml`, never of the installed manifest, and keeps the set until one of them changes. Measured on the CI machine: about 70 ms to load the test extension, then 1.1 µs a request to find the set unchanged and make its request; nothing is compiled per request
- The runtime's `Site` over the database serves options through `hyscribe-write`'s `get_option()`, `add_option()`, `update_option()` and `delete_option()`: a page view's calls each in a transaction of their own, committed at once; an action's in the action's own transaction, the extensions running on a blocking thread while the action runs the database work they send, so the action's rollback undoes theirs too and no request waits on a lock of its own. Without a writer (shadow mode, ADR 0010) they answer `failed`. Meta, queries, cron, mail and assets answer `failed` until they are served, the current user is no one with no capabilities and no nonce, and translations are the text as given, as no `languages/` folder is loaded yet
- Activation runs PHP's `activate_plugin()` at its places: the fault record cleared (activating again enables a disabled extension), `activate_plugin`, then the extension's `activate` export where `activate_{$plugin}` fires, in a request over the active extensions with the activated one last, loaded with the grants its activation approves; then `active_plugins`, `activated_plugin`, `recently_activated` and the approval. A component that does not load, or an export that traps or answers an error, leaves the plugin inactive as a fatal error in its activation hook leaves a PHP plugin: what it wrote before stays, `active_plugins`, `recently_activated` and the approval are untouched, and the screen answers PHP's redirect to `plugins.php?error=true&plugin=...&_error_nonce=...`, whose notice it shows (the frame PHP adds to show the error is taken out by the notice's kses, so `error_scrape` is never asked for and stays 501)
- Deactivation runs `deactivate_plugin`, the `deactivate` export where `deactivate_{$plugin}` fires and `deactivated_plugin` while the plugin is still listed active, then writes `active_plugins`. A deactivate export that traps is counted and logged, and the plugin is deactivated all the same: PHP would answer the fatal error and keep the plugin active, which would leave an administrator no way to stop a broken extension from the screen, so this is a deliberate difference (confirmed 2026-11-22)
- Faults met in a request, loading included, are logged on the error output with the extension, the hook, the callback and the fault, as the database error page's errors are
- New hook points that cross: `body_class` (the escaped classes and an empty `$css_class`, the answer made unique and escaped again joined, as `body_class()` prints it) and `admin_footer_text` (hs-admin's thanks as its look words them); the test extension's filters on them are checked against the plugin it stands in for, `reference/plugins/hyscribe-test`, which does what the extension does
- Not yet: hooks of object values, the rest of the runtime's `Site` above, bulk actions and clearing the recently active list (501)

## Amendment: SDK 0.2 as built so far (MVP 11.4, 2026-11-21)

- The WIT package is `hyscribe:sdk@0.2.0`. 0.2 only adds interfaces, and the host links each 0.1 interface under both `@0.1.0` and `@0.2.0`, so a component built against 0.1 keeps linking. The manifest's `sdk` is `0.1` or `0.2`; a component whose imports name another SDK (any patch version of the right one is taken) is refused at load, and its export is `guest@<sdk>.0`
- Time inside host calls is the host's, as ADR 0023's question 5 decided: every host function is timed, the epoch deadline's callback lets a call run until its own time, host calls left out, reaches its budget, and the extension's request budget counts the same time only; `REQUEST_TIME_LIMIT` bounds the rest of a request, and no call runs past 30 seconds of wall time, its host calls included. A request of 40 ms in a call of 20 ms no longer stops it (`host_calls_are_left_out_of_the_budget`)
- New grant tables, read like the earlier grant tables and shown on the Plugins screen before activation: `[transients] names` and `site` (a name, or a prefix ending in `*`; `*` alone is refused), `[roles] names` (roles it adds and removes; one of WordPress's five refuses the package) and `capabilities` (capabilities it gives or takes from any role), and `[http] hosts` (exact host names, case ignored). `[capabilities] db_tables` stays refused until plugin owned tables come
- `transients`: `get`, `set`, `delete` of a transient or a site transient, names at most 172 and 167 characters so their timeout options fit `option_name`. Served by `hyscribe-write`'s `get_transient()` (a transient not autoloaded and past its timeout is deleted on read, as PHP deletes it), `set_transient()`, `delete_transient()` and their single site `_site_` forms, whose `update_core`, `update_plugins` and `update_themes` never expire on read (write oracle `transient-expired`, `site-transient-delete`). The `pre_transient_*`, `transient_*` and `set_transient_*` hooks are not fired into extensions yet
- `roles`: `get` (any role, no grant), `add`, `remove`, `add-cap`, `remove-cap`, written as `WP_Roles` writes `{prefix}user_roles`, and `default_role` back to `subscriber` when it named a removed role (write oracle `roles-add`, `roles-caps`, `roles-remove`). An extension may give a granted capability to any role, WordPress's included, but adds and removes only its own roles
- Meta is now served: `get_metadata()`, `add_metadata()`, `update_metadata()` and `delete_metadata()` of `hyscribe-write`, values unslashed as PHP's functions unslash them
- `http`: `send( method, url, headers, body, timeout )` to a granted host, answering the status, headers and body. The method is one of GET, POST, PUT, PATCH, DELETE, HEAD and OPTIONS; a header's name is a token and not one the client sets itself (`Host`, `Connection`, `Content-Length`, `Transfer-Encoding`, `Accept-Encoding` and the other hop headers), its value without line breaks or NUL. `hyscribe-fetch`'s `send_within` sends it under every rule of ADR 0019 (the URL checked, every address of every hop public, 30 seconds, 4 MiB), following redirects for GET and HEAD only and only to granted hosts; the user agent is WordPress's unless the extension sets one, and its own headers and body are all it adds, its headers to the origin it named only (a redirect to another host, or from HTTPS to HTTP, sends none of them): the site's cookies and credentials are never sent. With `HTTP_OUTBOUND` off, and in admin actions (activation and deactivation), it answers `failed`
- Confirmed (2026-11-22): an extension may give or take a granted capability on any role, WordPress's five included, but adds and removes only its own roles; and its requests may carry its own headers, `Authorization` and `Cookie` included, to its granted hosts, never on a redirect to another origin
- Left for the rest of 11.4: admin pages and the settings API with admin-ajax, uploads folders and multipart bodies, page templates and `template_include` (11.7, and ADR 0023's plugin rendering instance), rewrite rules and query variables, plugin owned tables and migrations, the transient hooks, and the SDK test host ADR 0023 expects for theme ports (10.5)

## Amendment: SDK 0.4, admin pages and the settings API (MVP 11.4, 2026-11-24)

- The WIT package is `hyscribe:sdk@0.4.0`: the `admin` interface is new, so the minor version moves, and the host links 0.1, 0.2 and 0.3's interfaces under 0.4 too. The `plugin` world imports it, and the `theme` world through it. The test extension's manifest says `sdk = "0.4"`
- `admin.add-page` (`add_menu_page()`, `add_submenu_page()`; the SDK adds `add_options_page()` and the other pages under a core menu) is a registration, in `init` only. A slug is lower case letters, digits, `_` and `-`, which keeps it from naming a file, a path or a query; the parent is a core menu's file or one of the extension's own top level pages; the icon a `dashicons-*` class of the same characters, which the menu prints as a class, or none; a slug added twice is refused
- No new grant: a page reaches nothing its callback could not reach through the grants it has, and it is shown only to users with its capability, which the host checks as `user_can_access_admin_page()` does before the callback runs, refusing with PHP's 403 otherwise; as in `admin.php`, the callback runs only when it is on the page's hook for the user, so a top level page asked under another menu the user may reach, without the page's capability, answers PHP's "Cannot load" 500. Its titles and output are the extension's markup, as a printing action's are (ADR 0023's amendment); the menu is built for every hs-admin screen, so an active extension's pages are in every page's menu, as in PHP
- `register_setting()` needs the option in the `options` grant, its group may not be one of core's Settings groups, and its arguments are `type`, `label` and `description`; a sanitize callback is added as a filter on `sanitize_option_{$option}` without a hook grant of its own, the option grant covering it. Sections and fields may not go on core's Settings pages, which print core's fields only
- `admin.sections` answers `do_settings_sections()` in pieces, the host's markup and the extension's own callbacks with their argument, which the SDK runs itself, as `hooks.chain` does, since a component cannot be entered twice; `wp_unique_id()`'s count is the request's. `admin.settings-fields` answers the hidden fields of a group the extension registered, with the current user's nonce, made by hs-admin for the page it serves; another extension's group is refused
- Not yet: the save through `options.php` (501), `add_users_page()`, hidden pages, other query arguments, `add_settings_error()`, a setting's `default`, other extensions' callbacks on a page's hook (`load-{$page_hook}` included), the current user and nonces through `auth` on a page, and admin-ajax handlers

## Amendment: hooks around the checks (2026-11-25)

- "A hook never skips a check" is enforced by the grants, not by which values happen to cross: a hook around a check is a sensitive hook beside `rest_authentication_errors` and the rest. These are the hooks that can answer a REST request before its permission check or replace what the checks let through (`rest_pre_dispatch`, `rest_request_before_callbacks`, `rest_dispatch_request`, `rest_request_after_callbacks`, `rest_post_dispatch`, `rest_pre_serve_request`, `rest_pre_echo_response`, and `rest_endpoints`, which holds the routes' permission callbacks), those that change a save or an upload after its checks (`rest_pre_insert_{$post_type}`, by prefix as the option filters are but with no name of the extension's own, and `wp_handle_upload`), and those whose answer decides a check (`rest_allow_anonymous_comments`, `post_password_required`, `registration_errors`, `lostpassword_errors`, `lostpassword_user_data`, `password_reset_expiration`, `user_request_key_expiration`)
- Sensitive rather than refused: plugins use these hooks for real work (REST caches, spam checks on registration, longer reset links), as they use `map_meta_cap`, and the sensitive grant is this ADR's answer to a hook that changes who may do what: the administrator approves it apart, with the Plugins screen's warning, before activation and again when an update adds one. Refusal stays for what cannot be bounded (`phpmailer_init`'s transport)
- Today no callback runs on the REST hooks among them, whose values (a response, a save, the request) do not cross yet; the grant does not depend on that, so porting a value's PHP shape opens nothing a plain filter grant could reach
- A hook newly fired before a check, or whose answer decides one, joins the list in the same change; other hooks' answers go through the checks the value they replace goes through
