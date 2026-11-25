# Compatibility contract

"100% backwards compatible" is defined here so it can be tested. Each surface lists what HyScribe must guarantee.

## Core rules

1. **Read anything.** Any database produced by a supported WordPress version loads without errors or migrations.
2. **Write only what WordPress would write.** Every row HyScribe inserts or updates must be one PHP WordPress could have produced for the same action, with identical meaning.
3. **No core schema changes.** HyScribe never alters, adds columns to, or drops WordPress tables or indexes.
4. **Reversible at any time.** Pointing PHP WordPress back at the database shows the site exactly as HyScribe left it.
5. **Files stay where they are.** `wp-content` is served as-is. New files follow WordPress paths and naming.

## Security exceptions

Security comes first ([ADR 0006](adr/0006-security-first.md)). Data compatibility is never relaxed, but WordPress behaviours that are known attack vectors are off by default:

- XML-RPC (`xmlrpc.php`) and pingbacks are not served, and a trackback is answered that trackbacks are closed. No `.env` key turns them on: XML-RPC is planned for M9, and with it a key to opt in
- User enumeration through author archives: `?author=N`, `?author_name=` and `/author/<nicename>/` answer 404 unless `AUTHOR_ARCHIVES=on`, which serves them, and the `?author=N` redirect to the author's nicename, as WordPress does. The unauthenticated users REST endpoint answers visitors as WordPress does unless `REST_PUBLIC_USERS=off`
- Redirects of embed and RSS fetches that leave the allowed hosts are not followed; a global styles value with `{` or `}` is dropped as an unsafe one
- Executing or serving uploaded files as anything other than static content (never allowed)
- Hostile input PHP's stack answers with an error, or half done, is answered or refused whole: a REST answer whose `Link` header would hold a control character taken from a query parameter's name is sent without that header, where PHP's web server answers 500 (white space in it collapses to single spaces, as in PHP); a file sent to `wp/v2/media` as a form under a name that is not UTF-8 is refused with a server error and nothing stored, where PHP stores the file, fails to insert its attachment (400) and leaves the file
- Resource limits where PHP has none of its own: block markup nested more than 128 deep, or whose unclosed blocks would copy more than 64 MiB, parses to no blocks; `wp_kses()` drops a comment or declaration whose repeated filtering runs past its depth or work limit (never allowed)

The page head names HyScribe in its generator tag (`<meta name="generator" content="HyScribe <version>" />`) instead of WordPress, and leaves out the RSD tag while XML-RPC is off. The REST API and oEmbed discovery tags and `Link` headers match PHP; the endpoints they point to arrive with the REST API (M5).

The compatibility report lists each such difference for the site being scanned.

## Database

| Area | Requirement |
| --- | --- |
| Table prefix | Any `$table_prefix`, including non-default; multisite per-site prefixes (`wp_2_`) |
| Core tables | posts, postmeta, users, usermeta, options, terms, term_taxonomy, term_relationships, termmeta, comments, commentmeta, links |
| Multisite tables | blogs, blogmeta, site, sitemeta, signups, registration_log |
| Plugin tables | Left untouched unless a port owns them; ports must use the original plugin's schema |
| Charset | utf8mb4 and legacy utf8/latin1 sites; never re-encode stored data |
| Serialized values | PHP `serialize` format read and written byte-exactly, including string byte lengths and nested objects (`O:`) preserved as opaque values |
| Schema version | Supported from WordPress 7.0. `db_version` option read and mapped to a release; below 7.0 the wizard asks the user to upgrade with PHP WordPress first; refuse to write to unknown newer versions |
| Options | autoload semantics (`yes`/`no` and the newer `on`/`off`/`auto` values), transients, site transients |
| Dates | `post_date` and `post_date_gmt` pairs kept consistent with `gmt_offset` / `timezone_string` |
| GUIDs | `guid` column preserved and generated the WordPress way for new posts |
| Counters | `comment_count`, term `count` updated as WordPress would |

## Content rendering

| Area | Requirement |
| --- | --- |
| Blocks | Block comment grammar parser equivalent to `parse_blocks`; static blocks render from saved markup; dynamic core blocks rendered server-side |
| Classic content | `wpautop`, `wptexturize`, `convert_smilies`, `shortcode_unautop` equivalents |
| Shortcodes | Same parser semantics (nesting, attributes, escaping with `[[ ]]`); core shortcodes such as `gallery`, `caption`, `audio`, `video`, `embed` |
| Embeds | oEmbed results cached in postmeta (`_oembed_*`) reused as stored |
| Sanitization | kses-equivalent allowed HTML per role/capability |
| Excerpts | `the_excerpt` rules, `<!--more-->` and `<!--nextpage-->` |

## URLs and HTTP

| Area | Requirement |
| --- | --- |
| Permalinks | Every `permalink_structure` tag; plain, date, post name, custom; category and tag bases |
| Query strings | `?p=`, `?page_id=`, `?cat=`, `?s=`, `?attachment_id=`, `?feed=` and other public query vars |
| Redirects | canonical redirects, `_wp_old_slug` and `_wp_old_date` redirects, trailing slash rules |
| Feeds | RSS2, Atom, RDF, comment feeds, per category/tag/author |
| Sitemaps | `wp-sitemap.xml` and its sub-sitemaps |
| REST API | `/wp-json/` discovery and `wp/v2` routes with matching JSON shapes, `_embed`, `_fields`, pagination headers |
| Other endpoints | `wp-admin/admin-ajax.php`, `xmlrpc.php`, `wp-cron.php`, `robots.txt`, `favicon.ico`; `wp-login.php` and `wp-admin/<screen>.php` redirect (307) to `/hs-login` and `/hs-admin/<screen>` with the query kept (ADR 0008) |
| Static files | `/wp-content/...` and `/wp-includes/...` URLs stay exactly as they are: never redirected or rewritten |

## Users and authentication

| Area | Requirement |
| --- | --- |
| Password hashes | Verify legacy MD5, phpass portable (`$P$`), and the bcrypt `$wp` format introduced in WordPress 6.8 (old hashes survive upgrades to 7.x until the user next logs in); new hashes in the format WordPress 7.x writes |
| Cookies | `wordpress_logged_in_*` and `wordpress_sec_*` cookies valid across PHP WordPress and HyScribe when both use the same salts |
| Sessions | `session_tokens` usermeta format |
| Roles and caps | `{prefix}user_roles` option, per-user `{prefix}capabilities` and `{prefix}user_level` meta |
| Application passwords | Stored and verified compatibly |
| Nonces | Same lifetime and tick semantics |

## Files

| Area | Requirement |
| --- | --- |
| Uploads | Existing sites: the original `wp-content/uploads` stays in place. New sites: `hyscribe/uploads`. Always served at `/wp-content/uploads/`, with `YYYY/MM/` folders (or flat when `uploads_use_yearmonth_folders` is off), `upload_path` / `upload_url_path` respected |
| Image sizes | Intermediate sizes named `name-WxH.ext`, `-scaled` big image handling, sizes from `thumbnail_size_*`, `medium_size_*` and registered sizes |
| Attachment meta | `_wp_attached_file` and `_wp_attachment_metadata` written in the WordPress structure |
| Theme and plugin assets | Served at their WordPress URLs (`/wp-content/themes/<slug>/`, `/wp-content/plugins/<slug>/`) from `hyscribe/themes` and `hyscribe/plugins`, with the original relative paths |
| Core assets | `wp-includes` scripts and styles that themes and content reference must be available (bundled in the binary, ADR 0005, from the pinned release downloaded and checked by SHA-256 at build time) |

## Explicitly out of scope

- Running PHP from plugins, themes or `mu-plugins`
- Drop-ins such as `object-cache.php` and `advanced-cache.php` (replaced by HyScribe equivalents)
- Behaviour that depends on PHP-specific bugs, unless a fixture shows real sites rely on it
