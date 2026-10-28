# WordPress core data

Files from WordPress core that HyScribe uses as data: the core `theme.json`, every core block's `block.json`, the block types as WordPress registers them (`blocks/registry.json`) and their `block.json` in registration order (`blocks/registered.json`), the tags and attributes `wp_kses_post()` allows (`kses-post.json`), and the block library styles. Exported by `reference/wordpress/export.sh` from the pinned version in `VERSION`. Never edit by hand.

License: GPL-2.0-or-later, as WordPress (ADR 0004).
