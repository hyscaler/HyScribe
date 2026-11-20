# Themes

First-party theme ports, one folder per WordPress theme slug (docs/EXTENSIONS.md). A block theme port is mostly data: every file of the theme but its PHP, as WordPress bundles it, plus its PHP patterns run once into block markup (`patterns.json`), so HyScribe never runs PHP. The theme's files are built into the binary and served at the theme's usual URL, each theme's by its folder name, and the site is served with the theme its `stylesheet` option names; `patterns.json`, `styles.json` and `registered.json` (the block styles and pattern categories its PHP registers, its post formats and theme features, whether it supports `wp-block-styles`, the stylesheets it enqueues, the order PHP lists its templates and parts in, its entries on the REST themes route and the Themes screen, the block variations and bindings sources it changes) are the export's own and are not served. Adding a folder here adds a theme: no Rust names one. The export names HyScribe in the footer credit.

| Folder | Source | License |
| --- | --- | --- |
| `twentytwentyfive/` | Twenty Twenty-Five, as bundled with WordPress 7.1.3, exported by `reference/themes/export.sh` (patterns in en_US) | GPL-2.0-or-later (see its `style.css`); its fonts are SIL OFL 1.1 and its images CC0, as its `readme.txt` lists |
| `twentytwentyfour/` | Twenty Twenty-Four, as bundled with WordPress 7.1.3, exported by `reference/themes/export.sh twentytwentyfour` | GPL-2.0-or-later (see its `style.css`); its fonts are SIL OFL 1.1 and its images CC0, as its `readme.txt` lists |
| `twentytwentythree/` | Twenty Twenty-Three, as bundled with WordPress 7.1.3, exported by `reference/themes/export.sh twentytwentythree` | GPL-2.0-or-later (see its `style.css`); its fonts are SIL OFL 1.1, as its `readme.txt` lists |
| `twentytwentytwo/` | Twenty Twenty-Two 2.2, pinned from wordpress.org (an open question, see below), exported by `reference/themes/export.sh twentytwentytwo` | GPL-2.0-or-later ("GNU General Public License v2 or later" in its `style.css`); its fonts are SIL OFL 1.1 and its images CC0, as its `readme.txt` lists |

Never edit these files by hand; re-export them when the pinned WordPress version changes.

## Pinned themes

A theme WordPress 7.1.3 no longer bundles is pinned from wordpress.org at its latest release: `pinned.tsv` records each slug, version and the SHA-256 of `https://downloads.wordpress.org/theme/<slug>.<version>.zip`. `reference/themes/pin.sh` downloads the archives, checks each against its checksum before unpacking, and unpacks it into its own new folder, `target/pinned-themes/<slug>`, which `reference/default-install/php.sh` mounts read only into the PHP oracle, so the export, the goldens and the theme files golden read the theme as PHP runs it. HyScribe's build (`crates/hyscribe-assets/build.rs`) verifies and unpacks the same archive for the files' times and digests (`HYSCRIBE_THEME_ZIPS=<folder>` for offline builds). Nothing from an archive is run on the host.

| Slug | Version | License | Port |
| --- | --- | --- | --- |
| `twentytwentytwo` | 2.2 | GPL-2.0-or-later; fonts SIL OFL 1.1, images CC0 | `twentytwentytwo/` (10.1) |
| `twentytwentyone` | 2.9 | GPL-2.0-or-later (its `style.css` and `readme.txt`); bundled normalize.css is MIT | pinned only, ported in 10.6 |

A new pin is a row in `pinned.tsv`: take the version from the wordpress.org theme API, download the archive once and record its `sha256sum`.
