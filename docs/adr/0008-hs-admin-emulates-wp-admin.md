# 0008: hs-admin emulates wp-admin

- Status: Accepted
- Date: 2026-10-27

## Context

The question was how HyScribe provides an admin interface. Site owners, editors and agencies know `wp-admin` by heart, documentation and tutorials describe its screens, and plugin admin pages are built from its markup and CSS classes (`.wrap`, `.notice`, `.form-table`, list tables, postboxes). A new admin design would break all three.

## Decision

- The dashboard is called **hs-admin** and emulates `wp-admin` as closely as possible: same menu structure and order, screen names, page titles, list table columns, row actions, bulk actions, filters, screen options, help tabs, notices, color schemes, admin bar and keyboard behaviour
- Screens keep WordPress's screen names under the `/hs-admin/` path, without `.php` (`edit`, `post-new`, `upload`, `options-general`, `admin?page=<slug>`); the login screen is `/hs-login`. Every `/wp-admin/<screen>.php` URL and `/wp-login.php` redirect to the clean form with the query kept, for backwards compatibility only, so stored links, bookmarks and plugin links keep working. Those redirects are temporary (307), so nothing breaks when a site switches back to PHP WordPress
- Screens are server rendered by Rust with the same HTML structure, element IDs and CSS classes as `wp-admin`, using WordPress's own admin CSS and scripts (GPL-2.0-or-later, bundled)
- Editing uses the WordPress block editor packages (`@wordpress/*`) against HyScribe's REST API
- Ported plugins render their admin pages into the same markup, so their screens look as they do in WordPress
- Differences from `wp-admin` are allowed only for security (ADR 0006), for example no plugin or theme file editor, and no installing PHP code; each one is listed in [ADMIN.md](../ADMIN.md)
- This is a deliberate exception to the general shadcn admin template rule for new dashboards: matching `wp-admin` is the product requirement

## Consequences

- Fidelity is testable: each screen is compared against PHP `wp-admin` on the same database (DOM structure and screenshots), see [TESTING.md](../TESTING.md)
- No base color decision is needed; `wp-admin` color schemes apply (default "Default", user meta `admin_color`)
- The admin follows WordPress releases; new or changed screens become compatibility work items
- jQuery and other legacy admin scripts ship as static assets and fall under the same dependency and security review as everything else

## Amendment (2026-11-17): URLs without `.php`

- hs-admin's screens are addressed without the `.php` extension: `/hs-admin/edit`, `/hs-admin/post?post=1&action=edit`, `/hs-admin/admin-ajax`
- HyScribe prints the clean form in its own links, forms, redirects and script data; the `.php` form keeps working and answers a 301 to the clean form (POSTs are served in place, as a redirect would drop their body), so wp-admin's JavaScript, bookmarks and plugins that build `.php` URLs still work
- Goldens compare with `wp-admin/<file>.php` mapped to `hs-admin/<file>`, as they already map `wp-admin` to `hs-admin`
- Only hs-admin: public endpoints (`wp-comments-post.php`, `wp-cron.php`, `xmlrpc.php`, `wp-login.php`'s aliases) keep WordPress's names, as themes, plugins and outside clients call them

## Amendment (2026-11-20): a look of its own

The requirement is that nobody looking at hs-admin or `/hs-login` mistakes it for `wp-admin`. This departs from "as closely as possible" for the visible look and text only.

- Default color scheme: a user whose stored `admin_color` is missing, `modern` (WordPress 7.1's near-black "Default") or not a registered scheme is shown `coffee`: its stylesheet, the `admin-color-coffee` body class (also on `/hs-login`) and its icon colors. Nothing is written for it (ADR 0003): users keep what is stored, new users get what PHP writes for them, and the profile's color picker, which leaves `modern` out and lists the schemes by name with Coffee checked for those users, saves what the user picks, as PHP would for that choice
- WordPress's name is gone from hs-admin's own text: page titles end in "HyScribe", the footer says "Thank you for creating with HyScribe.", help tabs, notices, screen text and dashboard widgets name HyScribe or are rephrased, and the admin bar has no WordPress logo menu. The login screen shows a "HyScribe" text mark in place of the WordPress logo, linked to the site's home page. Only HyScribe's own screen strings change (those PHP translates with `__()` and its kin); user content and theme and plugin data stay as they are (the strings inside WordPress's JavaScript bundles followed on 2026-11-21, below)
- The markup contract is unchanged: CSS classes, ids, element structure, script and style handles, `/wp-content/` and `/wp-includes/` URLs, hook names, REST routes, cookie names and form field names, so plugins, stylesheets and switching back to PHP keep working; a site switched back gets PHP's own `wp-admin`, untouched
- Goldens stay differential: the PHP oracle runs the must-use plugin `reference/admin-look/hyscribe-look.php`, which makes the same changes through WordPress's own filters (`get_user_option_admin_color`, `gettext`, `gettext_with_context`, `update_footer`, `pre_load_script_translations`, `login_body_class`, `login_headertext`, `login_headerurl`, `login_head`, `admin_bar_menu`), and every admin, login and screenshot golden script loads it

## Amendment (2026-11-21): no WordPress name left in hs-admin, hs-login and the setup wizard

- The footer's version line (`update_footer`) shows HyScribe's own version, `HyScribe x.y.z`, in place of WordPress's
- Strings inside WordPress's script bundles that name WordPress are given HyScribe wording through WordPress's own translation mechanism: hs-admin prints, for the scripts `wp-block-editor`, `wp-block-library`, `wp-edit-post` and `wp-edit-site`, the inline `wp.i18n.setLocaleData()` script WordPress prints for a script's translations (`<handle>-js-translations`), mapping each source string of the `default` text domain to its HyScribe wording. The bundles' bytes stay those of the pinned release; the oracle gives PHP the same locale data through `pre_load_script_translations`
- Block descriptions core translates with the `block description` context (Classic, Shortcode, Comment Edit Link) are reworded in the editor's server-side block definitions; the oracle uses `gettext_with_context`. The REST block types route, a public answer, is unchanged
- The setup wizard names HyScribe in its own text; "WordPress" stays only where it names the PHP WordPress install a site comes from or switches back to, or WordPress versions a database must match
- Still named, as names of other services or software rather than branding: the embed block's WordPress and WordPress.tv variations and the social link block's WordPress service, the private API opt-in strings and deprecation messages of the bundles (never shown), the `X-Redirect-By: WordPress` header (not shown), and user content, theme headers and descriptions as stored

## Amendment (2026-11-23): HyScribe's mark on its own surfaces

The brand guide asks that hs-admin's and `/hs-login`'s own branding surfaces carry the HyScribe mark, never the W. The logo files are those of `brand/`, used as they are and never redrawn; they are not covered by the AGPL (`brand/README.md`), so the binary embeds them from `brand/` (`hyscribe-assets`) and no copy sits in a source folder. A HyScribe colour scheme as the default follows as a later step; until then the default stays Coffee.

- `/hs-login`: the mark (`hyscribe-mark.svg`) replaces the "HyScribe" text mark as the background of the `.login h1 a` link, at the size `login.css` gives WordPress's logo (84 px). The link's text stays "HyScribe" (`login_headertext`), hidden as WordPress hides it, so it stays accessible; the link still leads to the site's home page
- Admin bar: a `hyscribe` item where the WordPress logo menu was, in WordPress's markup (`<span class="ab-icon">` and screen reader text "HyScribe Dashboard"), one link to the Dashboard (`admin_url()`) without a submenu. The admin bar's inline style (`admin-bar-inline-css`) draws the small mark as the icon's `:before` (the bar's stylesheet takes every icon's own background image away with `!important`), at 20 px, 26 px on touch screens, and hides the item at 600 px and under, where WordPress hides its logo
- Favicon: admin and login pages of a site without a Site Icon print `<link rel="icon">` to the small mark (`hyscribe-mark-small.svg`), at `admin_head` before core's callbacks and at `login_head` after `wp_site_icon()`. With a Site Icon, nothing changes. The public front end and its `/favicon.ico` keep PHP's behaviour
- Files: the marks are served as `/hs-admin/images/hyscribe-mark.svg` and `/hs-admin/images/hyscribe-mark-small.svg`, each with an `ETag` from its SHA-256 and, as neither is in the release, no `Last-Modified`
- Goldens stay differential: `reference/admin-look/hyscribe-look.php` prints the login style and the icon and replaces the `wp-logo` node with the `hyscribe` item (`admin_bar_menu`, `wp_add_inline_style()`); `look.sh add` copies the marks into the oracle's `wp-admin/images/` from the same files the binary embeds, and `look.sh remove` deletes them

## Amendment (2026-11-23): a HyScribe colour scheme

Following the mark, hs-admin's default colour scheme takes the brand's colours in place of Coffee.

- Colour scheme: a HyScribe scheme, the default in place of Coffee for users without a scheme, with `modern` or with an unknown one (nothing is written, as before). It is built from WordPress's own colour Sass (`wp-admin/css/colors/_admin.scss`, `_variables.scss`, `_mixins.scss`, GPL-2.0-or-later, compatible per ADR 0004) as core builds Coffee and Ocean: `crates/hyscribe-assets/colors/hyscribe/colors.scss` sets the variables, and `reference/admin-colors/build.sh` compiles it with dart-sass 1.97.3, rtlcss 4.3.0 and clean-css 5.3.3 in a Node image pinned by digest, after checking that the same toolchain rebuilds core's Coffee byte for byte. The compiled `colors.css`, `colors-rtl.css`, `colors.min.css` and `colors-rtl.min.css` are committed. The scheme also sets the `--wp-admin-theme-color` variables that `admin-schemes.css` gives core's schemes, and `/hs-login`, which loads no scheme, gets them inline
- The picker lists HyScribe by name among the others (between Fresh and Light, as `admin_color_scheme_picker()` sorts once `modern` is not registered), with the swatches Ink, Scribe Navy, Scribe Blue and Nib Gold; the colour scheme Ajax save accepts `hyscribe`
- Colours: base (menu and toolbar) Ink `#131b26`, highlight and primary buttons Scribe Blue `#1a5fd0`, submenus and toolbar menus Scribe Navy `#003364`, links Scribe Blue (hover 10% darker), submenu links on focus Sky `#7db2ff`, notification bubbles Nib Gold `#c99a3a` with Ink text, page background Paper `#f6f8fb`, menu icons `#f1f2f3`
- Contrast (WCAG 2 ratios, AA asks 4.5:1 for text): menu text white on Ink 17.32, menu icons on Ink 15.47, current and hovered items white on Scribe Blue 5.85, submenu text (`#b8bbbe`) on Navy 6.55, focused submenu links Sky on Navy 5.84, current submenu item white on Navy 12.67, bubbles Ink on Nib Gold 6.74, toolbar search white on its field 14.13, links Scribe Blue on Paper 5.50 and on white 5.85, hovered links on Paper 7.76, primary buttons white on Scribe Blue 5.85 (hovered 6.93, pressed 8.25)
- Files: the scheme is served where core serves a scheme's files, `/hs-admin/css/colors/hyscribe/` (`colors.min.css`, `colors-rtl.min.css` and `colors.scss`, as core serves its own), with an `ETag` from each file's SHA-256 and, as none is in the release, no `Last-Modified`
- Goldens stay differential: `reference/admin-look/hyscribe-look.php` registers the scheme on `admin_init` with `wp_admin_css_color()`, shows it to users without a scheme, with `modern` or an unknown one, gives `/hs-login` its theme colour, and `look.sh add` copies the compiled scheme into the oracle's `wp-admin/css/colors/hyscribe/` from the same files the binary embeds

## Amendment (2026-11-24): the post editor's publish panel shows the mark

The editor bundle draws the WordPress logo, as an `svg` of class `components-site-icon`, in the "Are you ready to publish?" panel when the site has no Site Icon (with one it draws an `img` of the same class). The bundle is served as released, so a style rule on the block editors' pages (`$current_screen->is_block_editor()`) paints `hyscribe-mark.svg` over that `svg` at 36 px and hides its path; the `img` is not matched, so a Site Icon is never covered. The rule is appended to the admin bar's inline style, after the bar's own, in hs-admin (`hyscribe_admin::look::editor_icon_style`) and in the oracle's look plugin (`admin_enqueue_scripts`), so the goldens stay differential. The 7.1.3 post editor's back button and the site editor's site hub draw an arrow icon, not the logo, so they need no rule.

## Amendment (2026-11-24): the admin footer credits HyScaler

The attribution term in `NOTICE` (AGPL section 7(b)) asks a copy or fork to keep the statement in the Appropriate Legal Notices its interface displays (section 5(d)), so hs-admin's footer carries it.

- The footer's sentence reads `Thank you for creating with HyScribe. HyScribe is developed and supported by <a href="https://hyscaler.com">HyScaler</a>.`, inside the same `#footer-thankyou` span and still passed through `admin_footer_text`; the link is markup, as WordPress prints its own link in that sentence
- Only the admin footer carries it: not the front end, not `/hs-login`
- Goldens stay differential: the oracle's look plugin maps the same gettext string to the same sentence; every admin golden that holds a footer is re-recorded

## Amendment (2026-11-25): the welcome banner and the footer offer porting help

People trying HyScribe out may have custom themes or plugins to port, so the dashboard's welcome banner and the admin footer say where to write.

- The banner's link reads `Learn more about the <HyScribe version> version.` and goes to the release notes of that version on GitHub (`https://github.com/hyscaler/HyScribe/releases/tag/v<version>`), in place of the About page; a paragraph under it asks people trying HyScribe out to write to `contact@hyscribe.com` if they need help porting custom themes or plugins
- The footer's sentence continues `Need help porting your site or plugins? Contact us.`, a `mailto:contact@hyscribe.com` link whose subject is `Porting help for <domain>` (the host and port of the stored home URL, never the request's Host header) and whose body gives the home URL, HyScribe's version and a line to say what is needed; encoded as RFC 6068 asks (`rawurlencode()`, line breaks as `%0D%0A`), `&` escaped in the attribute; nothing else about the site or the user goes into it
- The banner takes the brand's colours: the scheme's ink as its background, the mark beside the text in place of core's artwork, links in the scheme's sky, the three columns' icon squares in the scheme's blue; a style rule on the dashboard's page appended to the admin bar's (`hyscribe_admin::look::welcome_style`)
- Goldens stay differential: the oracle's look plugin rewrites what `wp_welcome_panel()` prints, filters `admin_footer_text` and adds the same style rule, so every admin golden and the dashboard goldens are re-recorded from PHP
