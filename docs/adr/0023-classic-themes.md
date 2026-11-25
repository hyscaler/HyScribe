# 0023: The classic theme model

- Status: Accepted (2026-11-21)
- Date: 2026-11-21

## Context

MVP 10.4. Block themes are data and HyScribe renders them with no theme specific Rust (10.1 to 10.3). A classic theme is PHP: template files that print a page while calling template tags (`header.php`, `index.php`, `template-parts/...`), and a `functions.php` that registers hooks, theme features, menu locations, widget areas and Customizer settings. HyScribe never runs PHP (GOALS non-goals, docs/SECURITY_MODEL.md), so a classic theme has to be ported. ADR 0001 left the template engine open, ADR 0022 left the `theme` world equal to the `plugin` world until this ADR, and docs/EXTENSIONS.md reserved a package's `templates/` folder for "the engine chosen by the classic theme ADR".

The first two classic themes are fixed: Hello Elementor (10.5), the theme Elementor's pages are built on, and Twenty Twenty-One (10.6), the last bundled classic theme. Both were read for this ADR:

- **Hello Elementor 3.5.2** (the latest on wordpress.org on 2026-11-21, GPL-3.0-or-later, archive SHA-256 `f3f8b0f8503c46af264bfe5a1079836ee9d8eb18c34ddef47ed7b053e2b1d4c8`, not pinned yet): one routing template, `index.php`, which picks `template-parts/single`, `archive`, `search` or `404` by conditional tags; `header.php` and `footer.php` with two menu locations (`menu-1`, `menu-2`) printed with `wp_nav_menu()`, `the_custom_logo()`, `comments.php`; `add_theme_support()` in `after_setup_theme`; a Customizer section of links; its own admin page (`add_menu_page`, a JavaScript app over REST routes under `elementor-hello-elementor/v1`); and a page view write: on an admin request, `after_setup_theme` writes the `hello_theme_version` option when it is missing or older. A bundled notifications package fetches remote notices
- **Twenty Twenty-One 2.9** (pinned, GPL-2.0-or-later, about 4,400 lines of PHP): the template hierarchy (`index`, `single`, `page`, `archive`, `search`, `404`, `image`), template parts by post format, one widget area (`sidebar-1`), two menu locations (`primary`, `footer`), theme mods read in templates and in `after_setup_theme` (`background_color` decides `dark-editor-style`), a dark mode toggle, custom background and logo, block patterns and styles, starter content, Customizer settings with custom controls and selective refresh partials, and filters whose arguments are objects (`walker_nav_menu_start_el` and `nav_menu_item_args` get the menu item `WP_Post` and the `wp_nav_menu()` arguments, `wp_get_attachment_image_attributes` an attachment, `the_password_form` a post)

The default install fixture has `fresh_site` at 1, so opening the Customizer with Twenty Twenty-One active in PHP imports its starter content.

What the code already has for classic pieces: the legacy widget block and the classic core widgets (`hyscribe-render` `widgets`), classic menus as the navigation fallback converts them, with `wp_get_nav_menu_items()`'s item setup (`navigation::classic`), the `custom_logo` theme mod read by the site logo block, `theme_mods_<stylesheet>` read and written, and `retrieve_widgets()` and `_wp_menus_changed()` for a switch to a theme that registers no sidebars and no menus (`hyscribe-write` `themes`), `comments_template()` with Walker_Comment and the classic comment form (10.1), and the REST sidebars and widgets routes' reads. Every Customizer request answers 501, and the admin actions refuse the Customizer's flags (`wp_customize`, `customize_changeset_uuid`).

## Decision

### Classic themes are components on the extension runtime

- A classic theme port is a Rust crate built into a WebAssembly component like a plugin (ADR 0002, ADR 0022), its manifest saying `kind = "theme"`. Its templates and its `functions.php` logic are both inside the component, so all of a theme's code runs in the sandbox, under its grants and limits
- Hello Elementor and Twenty Twenty-One are ported this way, as the reference ports for themes that Contact Form 7, Yoast SEO and Elementor are for plugins; the alternative of porting them natively into the host, which ADR 0002 allows for core features, would prove nothing about how a theme author ports a theme (question 2)

### The template engine: askama, compiled into the theme's component

Candidates compared:

| Approach | Where templates run | Security rules | Parity with PHP | Porting |
| --- | --- | --- | --- | --- |
| **askama** (0.16.1, MIT OR Apache-2.0), templates compiled into the component | Inside the component, as Rust the template was compiled to | No template parser on the server: askama parses at build time, in its derive macro; the template's logic runs in the sandbox under the epoch deadline and the grants; no `unsafe` in the port; auto-escaping by default with a custom escaper | Exact: literal text is kept byte for byte (whitespace preserved by default), and every value goes through the SDK's ported escaping | Each PHP template becomes a template file of the same name and shape: `<?php echo esc_html( $x ); ?>` becomes `{{ x }}`, `if`, `foreach` and `while ( have_posts() )` become tags; easy to read beside the original |
| minijinja (3.1.0, Apache-2.0) or tera (2.4.0, MIT) interpreting the package's template files at run time in the host | In the host, outside the sandbox | An interpreter in the host running files read from the package, which SECURITY_MODEL.md's "no execution of any file from disk" is there to prevent; its parser needs a fuzz target and a `safe-to-deploy` audit of a large crate; its functions would reach core outside the grants; bounded only by its own fuel, not the epoch | Jinja's whitespace and escaping differ from PHP's and need work in every template | Templates readable, but `functions.php` logic still needs a component, so a port is two languages |
| minijinja inside the component | Inside the component | Sandboxed, but every request parses the templates again in a fresh instance (ADR 0022) | As above | As above, plus the parse cost on every page |
| Hand-ported Rust writing bytes | Inside the component | Sandboxed and with no dependency, but escaping is by hand at every write, against "templates auto-escape output by default" | Exact | Unreadable beside the original; reviews cannot compare a template with its PHP file |
| sailfish (0.11.9, MIT), compiled like askama, `<% %>` close to PHP's tags | Inside the component | As askama | As askama | Close to PHP's syntax, but a much smaller project, `std` only, and in no imported audit set |
| maud (0.27.0, MIT OR Apache-2.0), a Rust macro language | Inside the component | As askama | Emits no whitespace of its own, so PHP's whitespace is lost | Nothing like the original file |

Decision (question 1):

- askama with `default-features = false` and `alloc` and `derive` (askama supports `no_std` with `alloc`, as the SDK is), templates in the theme crate's `templates/` folder at the original file's path (`template-parts/content/content-single.html` for `template-parts/content/content-single.php`), compiled into the component
- The SDK, not each port, ties askama to WordPress's output rules:
  - **Bytes, not text.** Content in the database need not be UTF-8 and PHP prints it as it is, while askama writes into a Rust `String`. The SDK's values (what host calls return, always bytes) are written into askama's output as private markers with their bytes kept aside, and spliced back after rendering, so a page is bytes exactly as PHP prints them. A template's own literal text is checked at build time to hold no marker character, so a marker can only come from the SDK. The splicer is a parser of the guest's own output and gets a fuzz target (`sdk_template_splice`)
  - **Escaping.** The default escaper is the SDK's port of `esc_html()` (no double encoding of entities, `&#039;`); `esc_attr`, `esc_url`, `esc_textarea` and `wp_kses_post` are filters; a value printed without escaping must be of the SDK's `Html` type, which only host template tags return (`the_content()`, `wp_nav_menu()`, `wp_head()`). askama's own `|safe` is refused in ports by a build check, so raw output always has the reviewed marker SECURITY_MODEL.md asks for
  - **Whitespace.** PHP drops one line feed straight after `?>`. A port keeps askama's default of preserving whitespace and removes that line feed in the template source itself, which is exact where askama's trimming markers are not (they trim all the whitespace next to a tag)
- Nothing is parsed on the server, so no fuzz target is needed for templates; the port's Rust is tested natively like any port (below)
- Supply chain: for the themes HyScribe builds itself, `askama` and `itoa` need `safe-to-deploy` audits; `askama_macros`, `askama_derive`, `askama_parser` and `winnow` run only at build time and need `safe-to-run` (`winnow` 1.0.4 is already exempted at that level; exemptions only shrink, so the rest are audited, not exempted). A third party theme's own dependencies are its package's business, inside the sandbox
- The package format does not change: a classic theme's `.hyx` carries no `templates/`, as its templates are in `extension.wasm`; `templates/` stays the block templates' folder

### The `theme` world

The SDK's `theme` world is the `plugin` world plus three interfaces and one export, in the SDK minor version that ships with 10.5 (a minor version only adds, ADR 0022):

| Interface | WordPress | What it carries |
| --- | --- | --- |
| `template` | the loop and template tags | `have_posts`, `the_post`, `rewind_posts`, `wp_reset_postdata`, `in_the_loop`; the conditional tags (`is_home`, `is_singular`, `is_front_page`, `is_archive`, `is_search`, `is_404`, `is_page_template` and the rest); the current post's fields and the tags that print (`the_title`, `the_content`, `the_excerpt`, `the_permalink`, `the_post_thumbnail`, `post_class`, `body_class`, `language_attributes`, `bloginfo`, `wp_head`, `wp_body_open`, `wp_footer`, `comments_template`, `wp_link_pages`, `the_posts_pagination`, `get_search_form`, `the_custom_logo`, `wp_nav_menu`, `dynamic_sidebar`, `is_active_sidebar`, `get_avatar`, `parse_blocks` and `render_block`), each answering bytes; `get_header`, `get_footer`, `get_sidebar` and `get_template_part` fire their actions and say which template file the theme renders |
| `theme` | `functions.php`'s setup calls | `add_theme_support`, `remove_theme_support`, `register_nav_menus`, `register_sidebar`, `add_image_size`, `set_post_thumbnail_size`, `add_editor_style`, `$content_width`, `register_block_style`, `register_block_pattern`, `get_theme_mod`, `set_theme_mod`, `remove_theme_mod`; and the template files the theme has, with each page template's name and post types |
| `customize` | `customize_register`'s `$wp_customize` | panels, sections, settings, controls and selective refresh partials as JSON arguments, with sanitize, validate, active and render callbacks as callback numbers |
| `theme-guest` (export) | the template file PHP would include | `render( file ) -> result<list<u8>, string>` |

- Template tags are host calls into the core ports `hyscribe-render` already has, running core's filters and other extensions' callbacks (Elementor's `the_content` at priority 9), so a theme's page is assembled from the same functions a block theme's is. The loop's state (`$wp_query`, `$post`) lives in the host for the request, as PHP's globals do
- The host resolves the template hierarchy (`get_query_template()`, the `{type}_template_hierarchy` filters, `template_include` from SDK 0.2) over the template files the theme lists, as `locate_template()` does over files, and calls `render` with the one it picks. A plugin's template that calls `get_header()` (Elementor's header and footer template, 11.7) has the host render the theme's `header` the same way
- Hooks that pass objects cross as their PHP shapes once each is ported and checked against PHP (ADR 0022's 11.2 amendment): Twenty Twenty-One needs the menu item `WP_Post`, the `wp_nav_menu()` arguments, an attachment `WP_Post` and a post for `the_password_form`

### What a theme gets beyond a plugin

- Implicit: its own theme mods, `theme_mods_<stylesheet>`, read and written only through `get_theme_mod` and `set_theme_mod` (which run `theme_mod_{$name}` and `pre_set_theme_mod_{$name}` and write the serialized array as PHP does); the menu locations and widget areas it registers; the template tags' reads, which show what the page shows any visitor
- Not implicit: hooks (`after_setup_theme`, `wp_enqueue_scripts`, `body_class`, `customize_register` and the rest are listed under `[hooks]` like a plugin's), any other option (Hello's `hello_theme_version`), meta, REST namespaces, post types and the sensitive hooks. A Customizer setting of type `option` needs that option in the options grant, so a theme cannot write `siteurl` through the Customizer
- The manifest: `[extension]` takes `kind = "theme"`, `id` (the stylesheet slug and folder), `version`, `sdk`, `replaces_versions`, `compatibility` and `license`, and no `replaces`, as ADR 0022's 11.3 reader already requires. What the Themes screen shows (name, author, version, description, tags, `Template:`) is read from the package's `public/style.css` as PHP reads a theme's headers, so `name` and the other display keys are refused for a theme
- Activation is the Themes screen's: it shows the grants and requested limits before activating, as the Plugins screen does, and activating approves them (`approved.ext.toml` in `hyscribe/themes/<slug>/`); `switch_theme()`'s rows stay as 10.2 writes them

### `after_setup_theme`, theme features and enqueues

- Hooks are added in `init` once, as for plugins (ADR 0022)
- Theme features are request state, as PHP's globals are: `add_theme_support`, menu locations, widget areas, image sizes, editor styles, `$content_width`, block styles and patterns are taken in any action of the request and dropped at its end. The host fires `after_setup_theme`, `init`, `widgets_init` and `wp_loaded` at PHP's points, so a theme's setup callback runs each request, which Twenty Twenty-One's luminance check over a theme mod and Hello's admin only version write both need (question 6). Cost: one callback and a few host calls, a few microseconds
- Enqueues go through SDK 0.1's `assets` over the package's `public/`, which keeps the original theme's paths, so `/wp-content/themes/<slug>/style.css` is the same URL; the interface gains `wp_localize_script`, `wp_script_add_data` and `wp_style_add_data` (Twenty Twenty-One's `rtl` replacement)
- Page view writes a theme makes in PHP (Hello's `hello_theme_version`) are made the same, in primary mode only, through its options grant (ADR 0003)

### Rendering instances and time

- A component instance cannot be entered twice, and a theme's template calls `wp_head()`, whose callbacks include the theme's own (`wp_enqueue_scripts` inside it). So a theme gets two instances in a request: one runs `render`, the other answers the theme's hook callbacks. A plugin rendering a template (`template_include`) gets the same pair. Neither is kept past the request; state the template shares with callbacks goes through the host (the theme mods, the loop, `$content_width`), not through instance memory (question 4)
- Time spent in host calls is core's, bounded by `REQUEST_TIME_LIMIT`, and not counted against the component's call and request budgets: the host sets the epoch deadline again on return from each host call with what the guest had left. Without it, rendering a long page (core's `the_content` of ten posts) would count against a 50 ms call. Applies to every component call, not only themes (question 5)

### Child classic themes

- A child theme port is a crate that depends on its parent's port crate and overrides templates and callbacks at compile time; it builds into one component holding both, so finding a template part child first then parent happens inside the component, and both `functions.php` files' hooks run in one place, as PHP loads both (question 7)
- `stylesheet` names the child and `template` the parent, as now; the child's `public/style.css` has `Template:`; the parent's package must be installed too, as its files are served at the parent's URL. A child records the parent version it was built against, and with another parent installed it does not run and the Themes screen says why
- A child has its own `theme_mods_<child>`, never the parent's, as PHP keeps them
- Considered: a child as a component of its own, the host resolving template parts across both. A parent's template part that the child overrides, calling back a part only the parent has, would enter the parent while it is rendering, needing a third instance; one component avoids it
- 10.6 adds a fixture child of Twenty Twenty-One beside 10.3's block child, under `reference/themes/fixtures/`

### Nav menus

- `register_nav_menus` fills the locations; `nav_menu_locations` stays a theme mod; `wp_nav_menu()` is ported in the host with `Walker_Nav_Menu`, its filters (`nav_menu_css_class`, `nav_menu_link_attributes`, `walker_nav_menu_start_el`, `nav_menu_item_args`, `wp_nav_menu_items`, `wp_nav_menu_args`) and its `fallback_cb` values `wp_page_menu` and `false`, over the item setup `navigation::classic` already has
- A custom `Walker` class (PHP subclassing) does not carry over: a port uses the walker's filters or builds its menu from `wp_get_nav_menu_items()`'s data; neither theme of phase 10 has one
- Appearance > Menus (`nav-menus.php`) and its saves (`nav_menu` terms, `nav_menu_item` posts and their `_menu_item_*` meta, `nav_menu_locations`, `nav_menu_options`) are ported with 10.5, which Hello's two locations need; REST `menus`, `menu-items` and `menu-locations` follow PHP's answers for a classic theme
- A switch to or from a classic theme maps locations with `wp_map_nav_menu_locations()` over the registered ones, extending 10.2's `_wp_menus_changed()`

### Widget areas

- `register_sidebar` with PHP's arguments and defaults; `dynamic_sidebar()` and `is_active_sidebar()` in the host over `sidebars_widgets`, running `dynamic_sidebar_params`, `widget_display_callback` and `dynamic_sidebar`; block widgets render through the block renderer and the classic core widgets through the legacy widget port
- A switch maps `sidebars_widgets` with `retrieve_widgets()` and `wp_map_sidebars_widgets()` over the registered sidebars, extending 10.2's case of a theme with none
- Appearance > Widgets is the block widgets editor over REST (`sidebars`, `widgets`, `widget-types`), as for a theme that keeps `widgets-block-editor`; its writes are ported with 10.6. The legacy `widgets.php` form (a theme or plugin turning the block editor off) is left out
- Widgets a plugin or theme registers with `register_widget()` (a `WP_Widget` subclass) need an SDK interface of their own (`widget`, `form`, `update` callbacks); neither theme registers one, so it is left for later

### Theme mods and core theme features

- `get_theme_mod()` as PHP: the `theme_mod_{$name}` filter and the default's `sprintf()` of the template and stylesheet URIs; `set_theme_mod()` and `remove_theme_mod()` write the option's serialized array, its keys in PHP's order, through the writer (ADR 0010, 0012)
- The features the themes use are core ports in the host, each matched against PHP: `title-tag`, `automatic-feed-links`, `custom-logo` (`the_custom_logo()` with its width, height, flex and `unlink-homepage-logo` arguments), `custom-background` (`_custom_background_cb()`), `post-thumbnails`, `html5`, `responsive-embeds`, `align-wide`, `editor-styles` and `dark-editor-style`, `editor-color-palette`, `editor-font-sizes`, `wp-block-styles` and `classic-theme-styles`

### The Customizer

Options: answer 501 for every Customizer request, as now, which fails 10.6's done criterion and leaves no way to set a classic theme's settings; or port it. Decision (question 8):

- Port the Customizer: the screen (`/hs-admin/customize`, WordPress's controls scripts served as they are), its preview (the front end rendered with the changeset's values for a logged in user with `customize`), and its saves through `admin-ajax.php` (`customize_save` with PHP's nonce `save-customize_<stylesheet>`, the `customize` capability, each setting's own capability, validation and sanitizing)
- What PHP writes and HyScribe writes the same, each a write oracle action: the `customize_changeset` post (auto-drafts updated while the user edits, then published), the settings it applies (theme mods in `theme_mods_<stylesheet>`, options such as `blogname`, `blogdescription`, `site_icon`, `show_on_front` and `page_on_front`, the `custom_css` post and the `custom_css_post_id` theme mod, menus and their locations, widget options and `sidebars_widgets`), and the changeset afterwards as PHP leaves it
- The panels both themes show: Site Identity, Colors and Background (Twenty Twenty-One), Menus, Widgets, Homepage Settings, Additional CSS (shown only to a user with `edit_css`, which `DISALLOW_UNFILTERED_HTML` denies everyone by default), and the themes' own sections and controls, a custom control's markup from its render callback; selective refresh partials (Twenty Twenty-One's site title, tagline and dark mode)
- Left out at first, each answering 501 or absent as docs/operators/not-supported.md will list: saving a draft and scheduling, previewing another theme from the Themes panel (and `customize_stashed_theme_mods`), taking over another user's changeset lock, preview links shared with other people, and starter content
- Starter content: PHP imports a theme's starter content on a fresh site when the Customizer opens, writing auto-draft posts and a changeset. Recommended: HyScribe skips the import, so its Customizer on a fresh site shows the site as it is and writes nothing for it; skipping a write is allowed by ADR 0003, and is a listed difference (question 9)

### Testing parity with PHP

- Each theme is pinned like Twenty Twenty-One (`themes/pinned.tsv`, version and SHA-256) and mounted into the PHP oracle; 10.5 pins Hello Elementor and checks the licence of everything it bundles before porting any of it (ADR 0004); the notifications package's remote fetch is not ported
- Pages: `reference/pages/theme-list.sh` and `golden.sh` with the classic theme active, on the default install (101 requests) and the Theme Test Data (747, as hashes), byte for byte as the block themes are compared; fixture SQL assigns menus to each theme's locations and widgets to its areas, as the themes' own pages cannot show them otherwise. Every template file and part must be reached by at least one recorded page, or its absence is listed
- Theme files: `reference/themes/files-golden.sh <slug>` for the files of `public/`
- Writes: write oracle actions for switching to and from each classic theme (block to classic, classic to block, classic to classic, to the fixture child), each Customizer save kind, the Menus and Widgets saves, and Hello's admin view write
- REST and admin: the themes route's `theme_supports`, menus, menu items and locations, sidebars and widgets as each role; the Themes, Customizer, Menus and Widgets screens in the admin goldens and the screenshot comparison (S8); the switch-back suite gains a classic theme activated, customized and menus saved in HyScribe, then served by PHP
- The port's own code: built natively against the SDK's test host, where coverage must be 100% with no surviving mutants (docs/TESTING.md), and as a component in the golden suites; the test host does not exist yet and comes with 10.5

What 10.5 needs from this ADR: the `theme` world and `theme-guest` export, askama tied to the SDK as above, the render instance and the host call time rule, the embedder loading the active theme per request (11.3 left it open for plugins too), the SDK test host, the classic template hierarchy in the host, `wp_nav_menu()`, `the_custom_logo()`, Appearance > Menus, the Customizer's screen, preview and publish for Site Identity, Homepage Settings, Menus and Additional CSS, and Hello's version write. Hello's own admin page waits for SDK 0.2's admin pages (11.4): until then its menu entry is absent and its screen answers 501 (question 10).

What 10.6 adds: the full hierarchy and post format parts, `register_sidebar` and `dynamic_sidebar()`, the object shapes of its menu and attachment filters, custom background, dark mode, block patterns and styles, editor styles, the Customizer's Colors, Background and Widgets panels with custom controls and partials, Appearance > Widgets with its REST writes, and the fixture child.

## Consequences

- A classic theme port reads like the theme it ports, file for file, and runs entirely in the sandbox; the server gains no template parser and no interpreter
- Every value a template prints is escaped by WordPress's rules unless it is the SDK's `Html`, and pages stay bytes, so content that is not UTF-8 renders as PHP renders it
- Hello Elementor and Twenty Twenty-One wait on the embedder loading extensions per request, the SDK test host and the theme world; Elementor's front end (11.7) needs Hello's port and SDK 0.2's `template_include`
- A theme costs two instances per request and its memory limit twice; the host call time rule changes ADR 0022's accounting for every component and needs an amendment there once accepted
- The Customizer becomes a large admin port (screen, preview, changesets, partials), staged over 10.5 and 10.6; drafts, scheduling, theme previews, lock takeover, shared preview links and starter content stay out until asked for
- A child theme is rebuilt whenever its parent's port changes; a site's own unported child theme shows as missing until someone ports it
- Third party classic themes are installed with 10.7, which waits on a decision (distribution and signing); until then only the themes the binary carries are served

What stays out:

- Running a theme's PHP, the theme file editor (docs/ADMIN.md) and installing PHP code
- Custom `Walker` subclasses, `WP_Widget` subclasses from themes and plugins, the legacy Widgets form
- Output buffering of a page by a theme, as ADR 0022 refuses it for plugins
- Classic themes that opt into block templates (`block-templates`), hybrid themes, until a theme of the road map needs them
- Elementor Pro's theme locations (`elementor_theme_do_location`), as Hello calls them only when Elementor Pro defines the function

## Questions

1. Is the template engine askama, with templates compiled into the theme's component and tied to WordPress's escaping and bytes by the SDK? Recommended: yes
2. Are Hello Elementor and Twenty Twenty-One ported as components on the runtime rather than natively in the host? Recommended: components
3. How are the first-party classic themes shipped: built from source by the release build and embedded in the binary like the block themes, or as packages installed after 10.7? Recommended: embedded, loaded through the same host path, grants and approval as an installed package
4. Does a theme (and a plugin rendering a template) get a second instance per request, one rendering and one answering its callbacks? Recommended: yes
5. Is time inside host calls left out of a component's call and request budgets, bounded by `REQUEST_TIME_LIMIT` instead? Recommended: yes, for every component
6. Are theme features request state, taken in `after_setup_theme` and the other actions each request? Recommended: yes
7. Is a child classic theme compiled together with its parent into one component? Recommended: yes
8. Is the Customizer ported (screen, preview, publish), with drafts, scheduling, theme previews, lock takeover and shared preview links left out? Recommended: yes
9. Is starter content skipped on a fresh site, writing nothing? Recommended: yes, for now
10. Does Hello's own admin page wait for SDK 0.2's admin pages (absent and 501 until then) rather than 10.5 waiting on 11.4? Recommended: wait for 0.2

## Amendment: the foundation as built (MVP 10.5, 2026-11-22)

- The `theme` world is SDK 0.3 (`hyscribe:sdk@0.3.0`), a minor version over 0.2 whose interfaces the host links under 0.1, 0.2 and 0.3 alike; a theme's manifest names `sdk = "0.3"` or later. The `theme` interface of the table above is named `theme-setup` in the WIT, as a package cannot name an interface and a world alike
- `template` carries the loop as functions of its own (`have-posts`, `the-post`, `rewind-posts`, `reset-postdata`, `in-the-loop`), the conditional tags, the printing tags and the tags answering a value by WordPress's name (`conditional`, `print`, `get`), and `part` for `get_header()` and the other part functions. Named tags keep the interface additive: a tag the host serves later needs no new type, and a name it does not serve answers `invalid`. `theme-setup` lists the theme's template files in `init` (`templates`), with each page template's name and post types; its other calls and `customize`'s are request state, answered by the site for the theme's stylesheet
- The manifest takes `kind = "theme"` without `replaces` and without the display keys (`name`, `description`, `author`, `author_uri`, `plugin_uri`), and needs SDK 0.3 or later. A component loads as its manifest's kind only: a plugin importing `template`, `theme-setup` or `customize` is refused, and so is a theme without `theme-guest`'s `render`
- The SDK builds the `theme` world with its `theme` feature and exports a theme with `theme!` (a `Theme` beside its `Plugin`); the test theme `crates/hyscribe-ext/guest/theme`, built by `guest/build.sh theme`, drives every call
- The SDK test host: built natively, the SDK's functions call the same interface names and types as the bindings, answered by `hyscribe_sdk::test`'s state on the thread rather than by the host, so one source serves both targets and the SDK's own wrappers are covered and mutation tested natively. A port's tests use `test::Host` (docs/EXTENSIONS.md)
- The embedder loads the active theme per request with the plugins: `Runtime` takes the themes folder beside the plugins folder and, from the `stylesheet` and `template` options, loads the component installed and approved in `themes/<stylesheet>/` as the last extension of the request's set, so its callbacks follow the plugins' at a priority, as `functions.php` loads after them. A child runs only with its parent installed; the cache key holds both, and a change to either folder is a new version. Admin actions (a plugin's activation and deactivation) run over the plugins only for now
- `Request::render` runs `theme-guest.render` in the theme's second instance, made at its first render and kept for the request; its host calls fire hooks the first instance answers. The extension's budget and its stop after a trap, timeout or memory fault are one ledger across both instances; a render while rendering is refused. Host call time is left out of the render's budget as of every call (ADR 0022's 11.4 amendment)
- The classic hierarchy: `hyscribe_ext::locate` picks the first of a request's candidates (the block hierarchy's names, in the order the front end lists them, a page template's path among them) that the theme lists as a PHP file, as the template loader's getters and `locate_template()` pick it. Checked against PHP with Twenty Twenty-One pinned: `reference/themes/hierarchy-golden.sh` records the file the loader picked (through `template_include`) for the 101 requests of `reference/pages/default.list`, and all 101 match. The front end does not render a classic theme's page yet: the template tags its templates call are served with Hello Elementor's port

## Amendment: decisions for the Hello port (2026-11-22)

- The escaper askama uses in the SDK is written afresh from `esc_html()`'s specified behaviour, not ported from WordPress's code, and checked against PHP goldens, so the SDK keeps its Apache-2.0 OR MIT licence
- First-party theme components are built by `scripts/package.sh` and CI with the pinned toolchain before the binary, which includes their bytes; no `build.rs` runs cargo
- Raw bytes keep this ADR's scheme of markers with the bytes kept aside and spliced in after rendering; the private-use character scheme proposed during 10.5 was declined, as content could carry those characters
- `basic-toml`, which askama's `config` feature brings to read a port's `askama.toml`, is unmaintained (RUSTSEC-2026-0374): the advisory is ignored in `deny.toml` (2026-11-22), as the crate runs only at build time inside askama's proc macro, reading the port's own file, and is unmaintained rather than vulnerable; the entry goes when askama drops it

## Amendment: decisions for the classic head and theme faults (2026-11-23)

- A classic theme's page view makes the write PHP's classic head makes: `theme_mods_<stylesheet>` gains `custom_css_post_id` -1 when the theme has none, once, as PHP leaves it (ADR 0003 allows what PHP writes)
- An action callback may return output, which the host prints where the action fires (`wp_head`, `wp_footer`, `wp_body_open` and the other printing actions); an addition to SDK 0.3, so Hello's description meta tag in `wp_head` prints as PHP's does
- A fault of the active theme's component while rendering answers PHP's critical error page ("There has been a critical error on this website.") with status 500, not the database error page
- As built and kept: a theme call made from inside a hook a tag fired sees the page as it was before that tag, since no lock is held while a tag runs; the `hello-elementor-default` body class, from Hello's unported notifications package, is added by the port's own `body_class` filter


## Amendment: the classic head as built (MVP 10.5, 2026-11-23)

- An action that prints runs core's callbacks among the extensions' by priority, core's first at a priority as core adds its callbacks first; an extension's callback prints the string it answers, any other answer nothing (`hyscribe_model::hooks::do_action_printing`)
- `wp_head()` answers the head at once, as PHP prints it, with a placeholder unique to the page where the global styles go; when the page is rendered the host fills it with the global styles `wp_footer()` built and puts the styles enqueued after the head before the first `</head>`, as WordPress 6.9's template output buffer does for a classic theme loading block assets on demand
- A classic theme's `theme.json` is read from its package's `public/`, beside the files it serves; a classic theme without one answers `wp_head()` not served until its head (`classic-theme-styles`, global styles without `theme.json`) is ported with Twenty Twenty-One (10.6)
- The head's custom CSS lookup is written only where `current_theme` names the theme: PHP otherwise names it from the theme's headers, which HyScribe does not read for a classic theme yet, and skipping a write is allowed (ADR 0003)

## Amendment: the setup calls as built (MVP 10.5, 2026-11-23)

- The host fires `after_setup_theme` once a request's extensions are loaded, and answers `is_admin()` outside a page, true for hs-admin and wp-admin's PHP files. `register_nav_menus()` adds `menus` and `add_editor_style()` adds `editor-style`, as PHP does; locations, editor styles and the content width are not kept until what reads them (Appearance > Menus, the block editor, embeds and media) is ported. Theme mods and image sizes still answer not served
- The host serves `get_queried_object_id()`, `get_post_field( 'post_excerpt', $id )` of the queried post shown on its own (any other field or post is not served) and `wp_strip_all_tags()`, the tags Hello's description meta tag reads
- Markup and text the host answers cross as bytes through `print` (bytes keep content that is not UTF-8, which a `get` value cannot carry); `Raw::is_empty` lets a port test what a tag printed as PHP's `if ( $markup )` does. The SDK test host answers a tag by name and arguments (`get_bloginfo[Text("name")]`) and can refuse a conditional tag, so a port's every failure path is tested natively
- A port's template computes what it prints in PHP's order of calls, then renders; a template part PHP includes inline (`get_header()`, `get_template_part()`, `comments_template()`) is rendered by the port and appended where it is included, as only host tags make an `Html`
- Hello Elementor 3.5.2 was ported on these calls (`functions.php`'s callbacks and its eleven templates, its `version_compare()` matching PHP on 3,481 pairs) and moved out of HyScribe with the decisions below. The host's tests play Hello's callbacks with a stand-in that reads the queried post's excerpt through the tags above: its head and footer match PHP on all 68 pages of `reference/classic-head`, recorded with Hello pinned and active, and its template tags match on the pages of `reference/theme-tags`
- Not yet: `add_post_type_support()` (no SDK call), the template tags classic templates call beyond those served (`bloginfo`, `esc_url`, `home_url`, `the_title`, `the_content`, `post_class`, the comments template and the archive and search tags), serving a classic theme's `public/` files at its URL, and whole page goldens

## Amendment: no third-party theme or plugin in the binary (2026-11-23)

- The release binary carries no third-party plugin or theme. This reverses the 2026-11-22 decision that `scripts/package.sh` builds the first-party theme components into the binary, and answers question 3 the other way: ported themes ship as packages, installed like any extension
- Hello Elementor's port moves to a separate repository of ports, built and shipped apart from HyScribe as its own `.hyx` package, to be built later; its `version_compare()` fuzz target, its build script and the write oracle of its version write go with it. HyScribe keeps everything a classic theme needs from the host (the setup calls, the template tags, the classic head and footer, the SDK and its test host), tested against its own test theme (`crates/hyscribe-ext/guest`, `build.sh theme`) or a stand-in, never against a port's component
- Should HyScribe ever carry a theme, writing it into `hyscribe/themes/<slug>/` at start is the accepted way
- Hello Elementor stays pinned (`themes/pinned.tsv`) as the PHP oracle of the host's head, footer and template tag goldens. `basic-toml`'s advisory is no longer ignored in `deny.toml`, as no crate of HyScribe reads an `askama.toml` now

## Amendment: the template tags as built (MVP 10.5, 2026-11-24)

- The tags Hello Elementor's templates call are host tags by WordPress's name (`print`, `get` and `conditional`), listed in HISTORY (2026-11-24); each answers over the global post, which follows the main loop and `wp_reset_postdata()`, or answers not served without one. Filters whose arguments are post objects (`the_permalink`, `get_the_archive_title`) do not cross to extensions yet
- `comments_template()` sets up the post's comments and prints nothing: the theme's port renders its own `comments.php` from `have_comments`, `get_comments_number`, `wp_list_comments`, `the_comments_navigation` and `comment_form`, as only host tags make an `Html`. A theme listing no `comments.php` gets not served, as core's compat template is not ported for a classic theme
- The escaping functions are host tags, so a port escapes host values with core's own ports
- A theme package's `public/` is served at the theme's URL as the web server serves a theme folder: the files only, never PHP source, nothing outside `public/`
