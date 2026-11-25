# 0004: Licensing

- Status: Accepted
- Date: 2026-10-26

## Context

HyScribe reimplements WordPress behaviour, will translate logic from WordPress PHP source, may bundle `wp-includes` assets and `@wordpress/*` editor packages, and will host agent-made ports of WordPress plugins and themes. WordPress and nearly all of its ecosystem are licensed "GPLv2 or later". HyScribe chooses AGPL-3.0 so that hosted, modified versions stay open to the people who use them, while remaining compatible with the WordPress GPL ecosystem.

## Compatibility analysis

| Material | Its license | Usable in an AGPL-3.0 project? |
| --- | --- | --- |
| WordPress database schema and stored content | Schema is a functional interface; content belongs to the site owner | Yes. Reading and writing a database format is interoperability and creates no license obligation. The license of HyScribe does not affect anyone's data |
| WordPress core PHP (logic we translate) | GPL-2.0-or-later | Yes. The "or later" clause allows use under GPLv3, and GPLv3 section 13 permits combining with AGPLv3 code |
| `wp-includes` JS/CSS, Gutenberg `@wordpress/*` packages | GPL-2.0-or-later | Yes, same reasoning |
| Third-party libraries bundled by WordPress (jQuery and similar) | MIT and other permissive | Yes |
| Plugins and themes marked "GPLv2 or later" | GPL-2.0-or-later | Yes. Ports may be released as AGPL-3.0-or-later |
| Plugins and themes marked "GPLv2" only (no "or later") | GPL-2.0-only | **No.** GPLv2-only is incompatible with GPLv3 and AGPLv3. Such ports cannot be distributed with or run inside HyScribe unless the copyright holder relicenses |
| Theme images, fonts, icons | Often CC, OFL or other | Case by case. Assets are reused from `wp-content` in place, not redistributed by the port, so usually no issue |
| Premium (commercial) plugins | GPL code plus vendor terms, sometimes non-GPL assets | Only with vendor consent |

## Decision

- HyScribe core: **AGPL-3.0-or-later**, full text in `LICENSE`
- Every source file carries an SPDX header: `// SPDX-License-Identifier: AGPL-3.0-or-later`
- First-party ports: AGPL-3.0-or-later, keeping the original plugin's copyright notices and crediting it as the source
- The porting pipeline reads the plugin's license header and refuses to port GPL-2.0-only or non-GPL plugins without a recorded relicensing permission
- `cargo deny` enforces an allowlist of dependency licenses compatible with AGPL-3.0
- LGPL-2.1 code may be ported, taken under LGPL-2.1 section 3's conversion to the GPL version 2 or any later version, with its original copyright notice kept beside the SPDX line (2026-11-16; first: Text_Diff in `hyscribe_render::text_diff`)

## Network source offer

AGPL section 13 requires that users interacting with a *modified* HyScribe over the network can get its source. HyScribe makes this easy: a configurable "Source" link (default pointing at the upstream repository and version) exposed in the admin and through an HTTP header or endpoint.

## Still to be published

Contribution terms will be published before external contributions are accepted.

## Extensions (2026-11-14)

- The extension SDK and the WIT interface definitions: **Apache-2.0 OR MIT**, so an extension may carry any license, closed source included
- The official extension directory lists only extensions under a GPL-compatible or OSI-approved license
- Ports of WordPress plugins stay bound by the original plugin's license, as above
- Each SDK source file carries its own SPDX header (`// SPDX-License-Identifier: Apache-2.0 OR MIT`), and `cargo deny` treats the SDK crates as such

## Consequences

- Fully compatible with WordPress code, assets and the "GPLv2 or later" ecosystem
- Hosting providers that modify HyScribe must publish their changes; unmodified use carries no extra duty beyond the license notice
- GPLv2-only plugins become a known porting gap, shown as such in the compatibility report

This analysis is engineering guidance, not legal advice.

## Amendment: Plupload, served as WordPress ships it (2026-11-24)

- The WordPress files HyScribe embeds and serves unchanged include Plupload and mOxie (`wp-includes/js/plupload/`), which the media uploader loads. Their headers say "Released under GPL License" with no version, and the licence file beside them is the text of GPL version 2
- HyScribe does what WordPress does: it distributes these files unchanged, as part of WordPress's package, which is GPL version 2 or later. A grant that names no version lets the recipient choose any version (GPL version 2, section 9), so the files are not version 2 only, and the rule against GPL-2.0-only code stands as it is
- The files are served to the browser as they are, with their notices and licence file; nothing of them is ported into or linked with HyScribe's own code
- This is the reading HyScribe follows

