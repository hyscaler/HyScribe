# WordPress core data

Files from WordPress core that HyScribe uses as data: the core `theme.json`, every core block's `block.json`, the block types as WordPress registers them (`blocks/registry.json`) and their `block.json` in registration order (`blocks/registered.json`), the tags and attributes `wp_kses_post()` allows (`kses-post.json`), the MIME types a visitor may upload (`mime-types.json`), the registered script modules (`script-modules.json`), the block library and skip link styles, the emoji detection script, and the embed template's style and scripts. Exported by `reference/wordpress/export.sh` from the pinned version in `VERSION`. Never edit by hand.

License: GPL-2.0-or-later, as WordPress (ADR 0004).
