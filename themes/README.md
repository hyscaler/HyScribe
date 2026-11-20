# Themes

First-party theme ports, one folder per WordPress theme slug (docs/EXTENSIONS.md). A block theme port is mostly data: every file of the theme but its PHP, as WordPress bundles it, plus its PHP patterns run once into block markup (`patterns.json`), so HyScribe never runs PHP. The theme's files are built into the binary and served at the theme's usual URL, each theme's by its folder name, and the site is served with the theme its `stylesheet` option names; `patterns.json`, `styles.json` and `registered.json` (the block styles and pattern categories its PHP registers, its post formats and theme features, the stylesheets it enqueues, the order PHP lists its templates and parts in, its entries on the REST themes route and the Themes screen, the block variations and bindings sources it changes) are the export's own and are not served. Adding a folder here adds a theme: no Rust names one. The export names HyScribe in the footer credit.

| Folder | Source | License |
| --- | --- | --- |
| `twentytwentyfive/` | Twenty Twenty-Five, as bundled with WordPress 7.1.3, exported by `reference/themes/export.sh` (patterns in en_US) | GPL-2.0-or-later (see its `style.css`); its fonts are SIL OFL 1.1 and its images CC0, as its `readme.txt` lists |
| `twentytwentyfour/` | Twenty Twenty-Four, as bundled with WordPress 7.1.3, exported by `reference/themes/export.sh twentytwentyfour` | GPL-2.0-or-later (see its `style.css`); its fonts are SIL OFL 1.1 and its images CC0, as its `readme.txt` lists |
| `twentytwentythree/` | Twenty Twenty-Three, as bundled with WordPress 7.1.3, exported by `reference/themes/export.sh twentytwentythree` | GPL-2.0-or-later (see its `style.css`); its fonts are SIL OFL 1.1, as its `readme.txt` lists |

Never edit these files by hand; re-export them when the pinned WordPress version changes.
