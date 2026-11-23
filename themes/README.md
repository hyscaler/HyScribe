# Themes

First-party theme ports, one folder per WordPress theme slug (docs/EXTENSIONS.md). A block theme port is mostly data: every file of the theme but its PHP, as WordPress bundles it, plus its PHP patterns run once into block markup (`patterns.json`), so HyScribe never runs PHP. The theme's files are built into the binary and served at the theme's usual URL, each theme's by its folder name, and the site is served with the theme its `stylesheet` option names; `patterns.json`, `styles.json` and `registered.json` (the block styles and pattern categories its PHP registers, its post formats and theme features, whether it supports `wp-block-styles`, the stylesheets it enqueues, the order PHP lists its templates and parts in, its entries on the REST themes route and the Themes screen, the block variations and bindings sources it changes) are the export's own and are not served. Adding a folder here adds a theme: no Rust names one. The export names HyScribe in the footer credit.

| Folder | Source | License |
| --- | --- | --- |
| `twentytwentyfive/` | Twenty Twenty-Five, as bundled with WordPress 7.1.3, exported by `reference/themes/export.sh` (patterns in en_US) | GPL-2.0-or-later (see its `style.css`); its fonts are SIL OFL 1.1 and its images CC0, as its `readme.txt` lists |
| `twentytwentyfour/` | Twenty Twenty-Four, as bundled with WordPress 7.1.3, exported by `reference/themes/export.sh twentytwentyfour` | GPL-2.0-or-later (see its `style.css`); its fonts are SIL OFL 1.1 and its images CC0, as its `readme.txt` lists |
| `twentytwentythree/` | Twenty Twenty-Three, as bundled with WordPress 7.1.3, exported by `reference/themes/export.sh twentytwentythree` | GPL-2.0-or-later (see its `style.css`); its fonts are SIL OFL 1.1, as its `readme.txt` lists |
| `twentytwentytwo/` | Twenty Twenty-Two 2.2, pinned from wordpress.org (an open question, see below), exported by `reference/themes/export.sh twentytwentytwo` | GPL-2.0-or-later ("GNU General Public License v2 or later" in its `style.css`); its fonts are SIL OFL 1.1 and its images CC0, as its `readme.txt` lists |

Never edit these files by hand; re-export them when the pinned WordPress version changes.

## Child themes

A child block theme is a folder of its own files and the export's data: its `style.css` names its parent in `Template:`, and HyScribe looks a file up in the child first and then in the parent, as PHP does (`stylesheet` over `template`). Its `theme.json` is merged over the parent's, its style variations add to the parent's (a parent file of a name the child has is left out), and its `patterns.json`, `styles.json` and `registered.json` are the export's with the child active, so they hold what the parent registers too. Only the child's own files are served at its URL; the parent's are at the parent's.

The one child HyScribe has is a test fixture, `hyscribe-fixture-child` (10.3), a child of Twenty Twenty-Five. It is not a shipped theme, so it is not in this folder: its source is `reference/themes/fixtures/hyscribe-fixture-child/` (what the oracle mounts), its export `fixtures/themes/hyscribe-fixture-child/` (`reference/themes/export.sh hyscribe-fixture-child`), and the binary embeds it only with the `fixture-themes` feature of `hyscribe-render`, which the tests of the crates that read themes switch on. A shipped build lists the four themes above and no more.

## Pinned themes

A theme WordPress 7.1.3 no longer bundles is pinned from wordpress.org at its latest release: `pinned.tsv` records each slug, version and the SHA-256 of `https://downloads.wordpress.org/theme/<slug>.<version>.zip`. `reference/themes/pin.sh` downloads the archives, checks each against its checksum before unpacking, and unpacks it into its own new folder, `target/pinned-themes/<slug>`, which `reference/default-install/php.sh` mounts read only into the PHP oracle, so the export, the goldens and the theme files golden read the theme as PHP runs it. HyScribe's build (`crates/hyscribe-assets/build.rs`) verifies and unpacks the same archive for the files' times and digests (`HYSCRIBE_THEME_ZIPS=<folder>` for offline builds). Nothing from an archive is run on the host.

| Slug | Version | License | Port |
| --- | --- | --- | --- |
| `twentytwentytwo` | 2.2 | GPL-2.0-or-later; fonts SIL OFL 1.1, images CC0 | `twentytwentytwo/` (10.1) |
| `twentytwentyone` | 2.9 | GPL-2.0-or-later (its `style.css` and `readme.txt`); bundled normalize.css is MIT | pinned only, ported in 10.6 |
| `hello-elementor` | 3.5.2 | GPL-3.0-or-later (its `style.css` and `readme.txt`); see below for what it bundles | ported as `crates/hyscribe-theme-hello-elementor` (10.5, under way): the binary carries its component and the public files of its archive, installed into `themes/hello-elementor/` at start |

Hello Elementor's bundled parts, checked before porting any of it (ADR 0004, ADR 0023):

- The theme's own PHP, CSS, `theme.json` and front end scripts (`assets/js/hello-frontend.js`, `hello-editor.js`): GPL-3.0-or-later, the theme's licence; this is what the port takes, and GPL-3.0-or-later code may be ported under AGPL-3.0-or-later (GPLv3 section 13)
- `vendor/composer/`: Composer's autoloader, MIT (its `LICENSE`); not ported, as a port has no PHP to load
- `vendor/elementor/wp-notifications-package` 1.2.0: no licence file, its example plugin's header says GPL-3 and it ships inside the GPL-3.0-or-later theme; it fetches remote notices for the theme's admin, and is not ported (ADR 0023)
- The admin app bundles (`hello-elementor-settings.js`, `hello-elementor-topbar.js`, `hello-conversion-banner.js` and `271.js`): webpack builds carrying MUI and Emotion (MIT) with no licence comments kept; they serve Hello's own admin page, which stays absent (ADR 0023 owner question 10), so none is served or ported
- `screenshot.png`: a CC0 photograph and Font Awesome icons (SIL OFL 1.1), as its `readme.txt` lists; the admin images of `assets/images` are Elementor's marks for the admin page and are not served

Nothing it bundles is GPL-2.0-only or non-free, so the port may go ahead.

A new pin is a row in `pinned.tsv`: take the version from the wordpress.org theme API, download the archive once and record its `sha256sum`.
