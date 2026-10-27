# Themes

First-party theme ports, one folder per WordPress theme slug (docs/EXTENSIONS.md). A block theme port is mostly data: the theme's own templates, parts, `theme.json` and `style.css`, plus its PHP patterns run once into block markup (`patterns.json`), so HyScribe never runs PHP.

| Folder | Source | License |
| --- | --- | --- |
| `twentytwentyfive/` | Twenty Twenty-Five, as bundled with WordPress 7.1.3, exported by `reference/themes/export.sh` (patterns in en_US) | GPL-2.0-or-later (see its `style.css`) |

Never edit these files by hand; re-export them when the pinned WordPress version changes.
