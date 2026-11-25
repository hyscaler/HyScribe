# MVP roadmap

The minimum viable product, and the work between today and it. It cuts across the milestones of [ROADMAP.md](ROADMAP.md): it takes the parts of M2, M4, M5, M7 and M9 a default install needs, and leaves the rest for after.

## The MVP

**HyScribe replaces PHP WordPress for a default install, day to day.** An owner points HyScribe at the database of a default WordPress install (Twenty Twenty-Five, no plugins), and without PHP:

- Visitors see the same site, byte for byte (S1)
- Every existing user logs in with their password and role (S2)
- The owner writes, edits, schedules and publishes posts and pages in the block editor, with categories, tags, featured images, revisions and the trash
- Media uploads work, with the image sizes WordPress makes (S3)
- Comments come in, are moderated, replied to and managed
- Users are added, edited, given roles and deleted; each user edits their own profile
- Every setting of Settings is read and saved
- The site's header, footer, templates, navigation and styles are edited in the site editor
- Scheduled posts publish and the trash empties on time
- At any moment the database can be handed back to PHP WordPress with nothing lost (S7)
- Every hs-admin screen it serves matches `wp-admin` (S8)

**Not in the MVP:** the porting pipeline (M8), multisite, XML-RPC, languages other than English (M9), the external audit and 1.0 hardening (M10). The compatibility scanner of M2 is left out; the setup wizard is in.

**Right after the MVP:** themes (phase 10, from M7) and plugins with the extension runtime (phase 11, from M6), added to this road map on 2026-11-18. They do not hold up the MVP's acceptance (phase 9); the plugin work starts now, alongside the MVP's last phases, and their own acceptance is 11.6.

## Where it stands (2026-11-19)

The acceptance run of 2026-11-19 ([ACCEPTANCE.md](ACCEPTANCE.md)) found S1, S2, S3, S7 and S8 met on the default install, and S4 met for an existing site; what is left is in its gap table.

| Area | State |
| --- | --- |
| Front end | Done for the default install (98 of 99 whole pages match PHP byte for byte, the one difference a bad preview nonce) and all but 4 of 747 requests for the Theme Test Data: pages, routing, feeds, sitemaps, robots.txt, core blocks, embeds, the RSS block, shortcodes, the site editor's saved templates and global styles |
| Logins | Done: passwords in every WordPress format, HyScribe's own sessions, login limits (ADR 0018), logout, lost and reset password, registration |
| Admin, reading | Every screen of a default install's menu is served and matches `wp-admin`: Dashboard, Posts, Pages, Media, Categories, Tags, Comments, Users, Profile (also over HTTPS), Tools, Site Health (Status and Info), Settings, Themes, Plugins, the Site Editor |
| Admin, writing | Done for posts, pages, comments, terms, users, settings, the media library and the dashboard, through the same checks as PHP; the personal data request actions, theme and plugin actions answer 501 |
| REST API, block editor, media, site editor | The REST routes and their writes, the block editor and site editor saves and uploads of every type PHP's default list allows are in; media trash, PDF previews and the metadata of some audio and video formats answer 501 |
| Cron, email, setup wizard | Cron in the service (six core jobs), SMTP mail (ADR 0016) and the wizard for an existing site are in; the wizard's new site path and the compatibility scan are not |
| Security limits | Trusted proxies, request time limit, registration cap, outbound allowlists and global styles braces |
| Themes and plugins | Phases 10 and 11, planned 2026-11-20; every page renders with Twenty Twenty-Five and no plugin runs |

## Phases

Each work package (WP) names its size (S, M, L, XL, roughly one to several rounds of work each), what it depends on, and its exit. Every exit means: matching PHP in goldens or differential tests, 100% coverage, no surviving mutants, an independent review done, as everywhere in the project ([TESTING.md](TESTING.md)).

### Phase 1: the rest of the admin, read only

The screens a default install shows that are not served yet. All are reads, so they can be built side by side.

| WP | Work | Size | Exit |
| --- | --- | --- | --- |
| 1.1 | Settings: General, Writing, Reading (`options-general.php`, `options-writing.php`, `options-reading.php`) | M | Each screen matches PHP for the roles that may open it, over the default install and changed options |
| 1.2 | Settings: Discussion, Media, Permalinks, Privacy (`options-discussion.php`, `options-media.php`, `options-permalink.php`, `options-privacy.php`) | M | As 1.1 |
| 1.3 | Users: editing another user (`user-edit.php`) and Add New User (`user-new.php`) | M | Matches PHP for every role, over added users |
| 1.4 | Media: the library list and grid shells (`upload.php`), Add New Media (`media-new.php`), an attachment's edit screen | M | Matches PHP over the Theme Test Data's attachments |
| 1.5 | Appearance and Plugins: themes (`themes.php`), the site editor shell (`site-editor.php`), plugins (`plugins.php`, empty on a default install), and the Tools subscreens (Import, Export, Site Health, Export and Erase Personal Data) | M | Matches PHP on the default install |
| 1.6 | The notices that follow each action (`?updated=1`, `?trashed=1`, `?approved=1` and the rest) on every screen | S | Every notice of the ported screens matches PHP |

**Phase exit:** every menu entry of a default install opens a screen that matches PHP (S8 for reads).

### Phase 2: the write foundation

What every change needs, before any screen saves anything.

| WP | Work | Size | Exit |
| --- | --- | --- | --- |
| 2.1 | ADR 0012: the admin's writes, extending ADR 0010's gated writer: one transaction per action, counts recomputed as PHP recomputes them, no cache of writable state across requests, what happens when PHP writes the same rows | S | ADRs 0010 and 0012 accepted |
| 2.2 | A write layer in `hyscribe-db`: inserts, updates and deletes for every core table, as prepared statements, inside transactions, refused in shadow mode | L | Every write path tested against MariaDB, and its rows compared with what PHP writes |
| 2.3 | A write oracle: run one action in PHP over a fresh database, dump the rows it changed, run the same action in HyScribe over another fresh database, compare the dumps (dates and generated values masked) | M | The harness, its first ten actions, documented in [TESTING.md](TESTING.md) |
| 2.4 | The core write functions, ported: `wp_insert_post()`/`wp_update_post()` with slugs, GUIDs, dates and statuses; `wp_trash_post()`, untrash and delete; `wp_insert_term()`, `wp_update_term()`, `wp_delete_term()`, term counts; `wp_insert_comment()`, `wp_set_comment_status()`, comment counts; `wp_insert_user()`, `wp_update_user()`, `wp_delete_user()`; `update_option()`, meta functions; cache invalidation of what HyScribe caches | XL | Each function matches PHP's rows in the write oracle |
| 2.5 | Nonce checks and `check_admin_referer()` on every action, capability checks as `map_meta_cap()` does them | S | Refusals match PHP, every action refused without a valid nonce |

**Phase exit:** the core write functions match PHP row for row; nothing writes in shadow mode.

### Phase 3: the admin's actions

Every button and form of the screens of phase 1 and before, through `admin-post.php`, `options.php`, `admin-ajax.php` and the screens' own handlers.

| WP | Work | Size | Depends on | Exit |
| --- | --- | --- | --- | --- |
| 3.1 | Posts and pages: row actions (trash, restore, delete), bulk actions, Quick Edit and Bulk Edit saves (`inline-save`) | M | 2.4 | Write oracle and the screens after each action match PHP |
| 3.2 | Categories and tags: add, Quick Edit, edit screen (`term.php`), delete, bulk delete | M | 2.4 | As 3.1 |
| 3.3 | Comments: approve, unapprove, spam, trash, restore, delete, bulk, reply and Quick Edit (`replyto-comment`, `edit-comment`), Empty Spam and Trash, the comment edit screen (`comment.php`) | M | 2.4 | As 3.1 |
| 3.4 | Users: add, edit, change role, delete with reassignment, password reset links, profile saves, color scheme (`save-user-color-scheme`), session ends (`destroy-sessions`) | M | 2.4 | As 3.1 |
| 3.5 | Settings saves (`options.php` and the permalink screen's own) | S | 2.4 | As 3.1 |
| 3.6 | Dashboard writes: Quick Draft, the welcome panel and meta box state, screen options (`hidden-columns`, `closed-postboxes`, per page) | S | 2.4 | As 3.1 |
| 3.7 | Front end comments: `wp-comments-post.php`, with flood, duplicate and moderation checks and the comment cookies | M | 2.4 | As 3.1, from the front end |

**Phase exit:** everything the classic admin of a default install can change, changes the same rows as PHP.

### Phase 4: the REST API

The block editor and site editor talk to `wp/v2` and a few other namespaces. Only the routes they call are in the MVP.

| WP | Work | Size | Exit |
| --- | --- | --- | --- |
| 4.1 | REST server: routing, `?rest_route=` and `/wp-json/`, cookie authentication with the `wp_rest` nonce, `_fields`, `_embed`, `context`, errors, pagination headers, the index and schemas | L | The server's responses match PHP's for the index and errors |
| 4.2 | Read routes: posts, pages, types, statuses, taxonomies, categories, tags, users (`me` and the authors list), comments, settings, search, templates, template parts, global styles, navigation, blocks (synced patterns), block patterns and categories, block types, themes (the active one) | XL | Each route matches PHP for every context and role, over both fixtures |
| 4.3 | Write routes: posts and pages (with autosaves and revisions), terms, comments, users, settings, templates, template parts, global styles, navigation, blocks | XL | Responses and the rows written match PHP |
| 4.4 | `block-renderer`, and `url-details` as decided | M | Matches PHP, or the decided answer |

**Phase exit:** every request the two editors make on the default install answers as PHP's.

### Phase 5: the block editor

| WP | Work | Size | Depends on | Exit |
| --- | --- | --- | --- | --- |
| 5.1 | `post.php` and `post-new.php` serving the editor: the editor's scripts and styles, its settings, the preloaded REST responses (`block_editor_rest_api_preload()`), the meta boxes area | L | 4.2 | The editor's page matches PHP's for a new and an existing post and page |
| 5.2 | Saving: drafts, publishing, scheduling, private and password protected posts, sticky posts, featured images, categories and tags, the slug, excerpts, discussion settings | M | 4.3 | A post written in HyScribe's editor shows in PHP WordPress as written there (S7) |
| 5.3 | Autosaves, revisions and their screen (`revision.php`), post locks and their heartbeat (`heartbeat` in `admin-ajax.php`) | M | 4.3 | Matches PHP |

**Phase exit:** writing and publishing in HyScribe is indistinguishable from writing in PHP WordPress.

### Phase 6: media

| WP | Work | Size | Depends on | Exit |
| --- | --- | --- | --- | --- |
| 6.1 | ADR: image decoding and resizing. A memory safe Rust library, fuzzed, with limits on dimensions and memory; no shelling out | S | | ADR accepted |
| 6.2 | Uploads: `async-upload.php` and `wp/v2/media`, file names as `wp_unique_filename()` makes them, the uploads folder layout by month, MIME checks as `wp_check_filetype_and_ext()` | M | 2.4, 6.1 | Files and rows match PHP's |
| 6.3 | Image sizes: the intermediate sizes, `-scaled` for big images, `_wp_attachment_metadata`, EXIF orientation as WordPress applies it | L | 6.2 | Sizes and metadata match PHP's within the encoders' differences, which the ADR bounds |
| 6.4 | The media library: the grid's `query-attachments`, the modal in the editors, attachment edits, deletes | M | 6.2, 4.3 | Matches PHP |

**Phase exit:** S3.

### Phase 7: the site editor

| WP | Work | Size | Depends on | Exit |
| --- | --- | --- | --- | --- |
| 7.1 | `site-editor.php` serving the editor with its settings and preloads | L | 4.2 | Matches PHP's page |
| 7.2 | Template and template part edits, saved as `wp_template` and `wp_template_part` posts and rendered by the front end in place of the theme's files | L | 4.3 | Edited templates render as PHP renders them |
| 7.3 | Global styles and navigation edits, with their effect on the front end | M | 4.3 | As 7.2 |

**Phase exit:** a site customised in HyScribe's site editor looks the same in PHP WordPress.

### Phase 8: background work and operations

| WP | Work | Size | Depends on | Exit |
| --- | --- | --- | --- | --- |
| 8.1 | ADR and port: WP-Cron's jobs in the service, from the `cron` option as PHP stores it: publishing missed scheduled posts, `wp_scheduled_delete`, expired transients, `delete_expired_transients`, the privacy request clean up | M | 2.4 | Jobs run when PHP's would, and the `cron` option stays one PHP WordPress reads |
| 8.2 | ADR and port: email. A mail transport (SMTP settings in `.env`), and the messages WordPress sends: password reset, new user, email change confirmation, comment notifications, admin email checks | M | | Messages match PHP's text and headers |
| 8.3 | The lost password and reset flows of `/hs-login` | S | 8.2, 2.4 | Matches PHP |
| 8.4 | The setup wizard (M2's wizard, without the compatibility scanner): database details, salts, mode, first run checks, written to `.env` | M | | S4 |
| 8.5 | Switching back: a test suite that runs every action of phases 3 to 7 in HyScribe, then serves the database with PHP WordPress and checks every screen and page | M | 3 to 7 | S7 |

### Phase 9: acceptance

| WP | Work | Size | Exit |
| --- | --- | --- | --- |
| 9.1 | The MVP's criteria end to end on both fixtures: S1, S2, S3, S4, S7, S8 | M | All met, in CI |
| 9.2 | The threat model updated for the write path, the REST API and uploads; every new parser fuzzed; an internal security review of every write | M | No open high findings |
| 9.3 | Operator documentation: install, switch over, switch back, backups | S | Published with the release |

### Phase 10: themes

The targets, in order: the bundled block themes, then two classic themes chosen: Hello Elementor, the theme Elementor's pages are built on, and Twenty Twenty-One, the last bundled classic theme. Each is matched against PHP WordPress running the same theme at a pinned version on the default install and the Theme Test Data, and each theme's licence is checked against ADR 0004 before any of it is ported.

| WP | Work | Size | Depends on | Exit |
| --- | --- | --- | --- | --- |
| 10.1 | Every bundled block theme served as data (Twenty Twenty-Four, Twenty Twenty-Three, Twenty Twenty-Two): `theme.json`, templates, parts, patterns and style variations, with no theme specific Rust | L | | Each theme's front end matches PHP's on both fixtures |
| 10.2 | Switching themes: Appearance > Themes activate, `switch_theme()`'s writes (`template`, `stylesheet`, `theme_mods_*`, `theme_switched`), the theme details and the Customizer's absence for block themes as PHP shows it | M | 10.1 | Screens and rows match PHP's; a switched site serves as PHP serves it |
| 10.3 | Child block themes: `stylesheet` over `template`, file lookup child then parent | S | 10.1 | A child theme fixture matches PHP |
| 10.4 | ADR: the classic theme model. The template engine for ported PHP templates (ADR 0001 left it open), how `functions.php` logic becomes a component on the extension runtime, nav menus, widget areas, theme mods and the Customizer's saves | S | 11.1 | ADR accepted |
| 10.5 | Hello Elementor, the first classic theme: small, and the theme Elementor's pages are built on; header, footer, the template hierarchy, theme mods, `the_custom_logo` | M | 10.4, 11.2 | Front end and its admin screens match PHP, alone and with Elementor (11.7) |
| 10.6 | Twenty Twenty-One, the last bundled classic theme: the template hierarchy, nav menus, widget areas, theme mods, its dark mode toggle, `the_custom_logo` and the Customizer's saves it relies on | L | 10.5 | Front end, the Customizer's saves and its admin screens match PHP on both fixtures |
| 10.7 | Installing themes: `hyscribe theme install <file>` into `hyscribe/themes/<slug>/`, upload from Appearance > Themes, delete, with the package checks of 11.3 | M | 10.2, 11.3 | Matches PHP's screens; a refused package changes nothing |

**Phase exit:** a site switches between the bundled block themes, a child theme, Hello Elementor and Twenty Twenty-One in HyScribe, and PHP WordPress serves each the same way.

### Phase 11: plugins and the extension runtime

The three reference plugins are the most installed open source plugins: Contact Form 7, Yoast SEO and Elementor, ported in that order, smallest first. Each runs as a WebAssembly component (ADR 0002) beside its PHP original at a pinned version, and must leave the database as the PHP plugin leaves it, so that switching back keeps it working.

| WP | Work | Size | Depends on | Exit |
| --- | --- | --- | --- | --- |
| 11.1 | ADR 0022, the extension host (drafted as 0020 on its branch; renumbered, 0020 being the non-image uploads ADR): wasmtime, the WIT world of SDK 0.1, fuel and memory limits per call, hooks crossing into a component and back in bounded time, capabilities, the `.hyx` package | M | its ten questions | ADR accepted |
| 11.2 | The runtime and `hyscribe-sdk` 0.1: every WordPress hook point the subsystems expose made callable from a component, with options, meta, `WP_Query`, REST routes, shortcodes, blocks, assets, cron, mail and `current_user_can` as EXTENSIONS.md lists them, each limited by the manifest's capabilities | L | 11.1 | A test extension drives every SDK call; a component that overruns its limits is stopped and the request still answers |
| 11.3 | The lifecycle: the `.hyx` package and manifest (parsed and fuzzed), `hyscribe plugin install`, activate, deactivate and delete from the Plugins screen as PHP does them, `active_plugins` kept as PHP reads it, the capability list shown before activation, missing plugins listed | M | 11.2 | Screens and `active_plugins` match PHP's; switching back leaves the original plugins active |
| 11.4 | SDK 0.2, what the three plugins need beyond 0.1: admin pages and the settings API, multipart requests and writes into granted `uploads/` folders, outbound HTTP to granted hosts (ADR 0019), page templates and `template_include`, rewrite rules and query variables, plugin owned tables with typed schemas and migrations (never string-built SQL), roles and capabilities, transients | L | 11.2 | Each addition driven by the test extension, under its capability |
| 11.5 | Contact Form 7: the form shortcode and block, validation, its REST submission route, the mail it sends (ADR 0016), its admin screens and the `wpcf7_contact_form` posts | M | 11.3, 11.4 | Forms, answers, mail and rows match the PHP plugin's |
| 11.6 | Yoast SEO: the head output (titles, meta, canonical, Open Graph, schema JSON-LD), its XML sitemaps over rewrite rules, the indexables tables, its settings and post metabox saves | L | 11.3, 11.4 | Pages, sitemaps, REST answers and rows match the PHP plugin's on both fixtures |
| 11.7 | Elementor's front end: pages rendered from `_elementor_data`, its page templates, the CSS files it writes under `uploads/elementor/`, with Hello Elementor (10.5) | L | 11.4, 10.5 | Elementor built pages match PHP's byte for byte |
| 11.8 | Elementor's editor, after its front end: its JavaScript app served as is, with HyScribe answering its admin-ajax actions as PHP does | L | 11.7 | Pages edited in HyScribe's editor match pages edited in PHP's |
| 11.9 | Security: the threat model for extensions, the runtime and package parser reviewed, every host call checked against the manifest, fuzz targets for the manifest, the package and the WIT boundary | M | 11.2, 11.3 | No open high findings |
| 11.10 | Acceptance for phases 10 and 11: the MVP's criteria rerun on each theme and with each reference plugin active, the switch back suite extended to themes and plugins, all in CI | M | 10, 11.1 to 11.9 | S1, S7 and S8 met with themes and plugins, in CI |

**Phase exit:** the three reference plugins and the chosen themes run in HyScribe without recompiling it, a misbehaving extension cannot take the server down, and the site switches back to PHP WordPress with the original theme and plugins active.

## Order and parallel work

Phases 1 and 2 start together: phase 1's screens are independent reads, phase 2 is foundations. Phase 3 needs 2. Phases 4 and 8 can start once 2 is done. Phases 5, 6 and 7 need 4. Phase 9 closes. After the MVP: the block themes (10.1 to 10.3) start at once, as they need no runtime; the extension host ADR (11.1) waits on its questions, then the runtime (11.2), the lifecycle (11.3) and SDK 0.2 (11.4) open the plugin ports, smallest first; the classic theme ADR (10.4) follows 11.1, and 11.10 closes both phases.

```
1 (read screens) ──────────────┐
2 (write foundation) ── 3 (actions) ─────────────────────────┐
                    └── 4 (REST) ── 5 (block editor) ────────┤
                                 ├─ 6 (media) ───────────────┤── 9
                                 └─ 7 (site editor) ─────────┤
                    └── 8 (cron, mail, wizard) ──────────────┘

10.1 to 10.3 (block themes) ──────────────────────────────────────┐
11.1 (ADR) ── 11.2 (runtime) ── 11.3, 11.4 ─┬── 11.5 (CF7) ───────┤
     │                                     ├── 11.6 (Yoast) ──────┤
     └── 10.4 ── 10.5 (Hello) ─────────────┴── 11.7, 11.8 ────────┼── 11.10
                     └── 10.6 (Twenty Twenty-One) ────────────────┘
```

Within a phase, the work packages of a table can be built in parallel on separate branches. They share one PHP oracle and one fixture database, so recordings and database backed test runs take turns through a lock, and their branches merge into `development` one at a time.

## Decisions it needs

| Decision | Blocks |
| --- | --- |
| HTTP client for outbound fetches (decided: `hyper` and `rustls` over the host's certificates; the client is still to build, with its own ADR) | 4.4, embeds in the editor |
| update notices in hs-admin (decided: HyScribe's own release notice, once releases publish one) | 1.5 (Updates screen) |
| the dashboard's Quick Draft auto-draft (decided: written as PHP does) | 3.6 |
| the dashboard's calls to wordpress.org | Events and News on the dashboard |
| the admin's user settings writes (decided: written as PHP does) | 3.6 |
| the edit lock a view of the edit screen writes (decided: written as PHP does, with the write layer) | 1.4 (attachment edit screen), phase 5 (block editor) |
| the public users REST endpoint (decided: on by default as WordPress serves it, with an owner setting to turn it off) | 4.2 (users route) |
| ADR 0010, the gated writer, and ADR 0012, the admin's writes (2.1): accepted 2026-11-14 with amendments (portable SQL, an optional read only database user, no writes to MyISAM tables, a warning for a persistent object cache) | Phase 2 |
| ADR 0015, image decoding and resizing (6.1, decided: accepted 2026-11-16): the codec crates with SIMD off and HyScribe's own resampler behind a `WP_Image_Editor` shaped interface, the IJG licence for `jpeg-encoder`, AVIF kept without sizes, WebP sizes lossless for now, limits at the oracle's ImageMagick policy (32,000 wide and high, 256 million pixels, 1 GiB) in `.env`, a worker pool sized to the CPUs, WordPress's image hooks; the goldens' bounds as proposed | Phase 6 |
| ADR 0017, WP-Cron in the service (8.1, decided: accepted as proposed 2026-11-16): a once a minute task in primary mode behind PHP's `doing_cron` lock, none with a persistent object cache on the PHP side; the six core hooks it runs, every other event (update checks, plugins') left untouched; `DISABLE_WP_CRON` with `hyscribe cron run`, `ALTERNATE_WP_CRON` ignored, both copied by the wizard; events keyed by hook name, `wp_schedule_event` and its siblings under their names | Phase 8 |
| ADR 0016, the mail transport (8.2, decided: accepted 2026-11-16): SMTP settings in `.env` with `MAIL_FROM` defaulting to `wordpress@` and the host, `lettre` certified by HyScribe with cargo vet, the reset limit per account kept and a bounded queue over the overall limit, background sending where PHP ignores the result, the commenter's IP address in notifications, one `wp_mail()` with WordPress's filters and a `phpmailer_init` stand in | Sending any mail (8.2), the lost password flows (8.3) |
| ADR 0018, login rate limits (decided: accepted 2026-11-16 with its defaults): a step in the `authenticate` chain, `authenticate`, `wp_login_failed` and `wp_login` under their names, concurrent password checks capped site wide at the CPU count | The `authenticate` chain and the site wide cap (follow-up to the built limits) |
| ADR 0019, the outbound HTTP client (accepted 2026-11-17): `hyper` and `rustls` with `ring` over the host's certificates, one connection per hop; WordPress's URL checks with every resolved address of every hop public and connected to as checked, no exemption for the site's own host; 30 seconds, 4 MiB and 5 redirects at most; nothing sent but the request; `http_request_args` and `pre_http_request`; `HTTP_OUTBOUND` off by default; trusted publishers for the TLS crates in cargo vet | 4.4 (`url-details`, built and answering 501 until accepted), the RSS block, embeds, the opt-in calls |
| ADR 0013's writes amendment, the REST server's writes (4.3, decided: accepted 2026-11-16): `rest_invalid_json` ported, WordPress's REST hooks at their points, route registration as `register_rest_route()` with a required `permission_callback` | 4.3 onwards |
| distribution of extensions (hosted registry, Git index or both; signing) | 10.8, 11.3 (what `install` accepts) |
| the classic themes of phase 10 (decided: Hello Elementor and Twenty Twenty-One) | 10.5, 10.6 |
| the extension host ADR's ten questions (wasmtime, instance lifetime, limits, faults, supply chain, sensitive hooks, Elementor's editor, SDK 0.2's order, SDK versions, the spike's guest) | 11.1 onwards |
| the three reference plugins (decided: Elementor, Yoast SEO, Contact Form 7) | 11.5 to 11.8 |
| when the plugin work starts (decided: now, alongside the MVP's last phases) | Phase 11 |
| ADR 0002, WebAssembly components (decided: accepted 2026-11-14) and the SDK's licence (decided: Apache-2.0 OR MIT) | Phase 11 |

## Size

Phase 1 is about a week of the current pace. Phases 2 and 3 are the next largest block; the REST API (4) and the two editors (5, 7) are the largest single parts, as they carry the most behaviour to match. No dates are given until phase 2's write oracle shows how fast writes can be matched.
