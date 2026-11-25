# Not supported yet

HyScribe answers `501 Not Implemented` where it reaches something it does not handle yet, rather than guess at what PHP WordPress would do. A 501 never writes anything. This page groups what answers 501 today, and the other gaps an operator should know before moving a site.

A 501 is a reason to keep PHP WordPress at hand: whatever answers 501 in HyScribe still works there, on the same database.

## The site as a whole

- **Themes:** the block themes Twenty Twenty-Two, Twenty Twenty-Three, Twenty Twenty-Four and Twenty Twenty-Five are built into the binary, and Appearance > Themes switches between them as PHP does. A site whose active theme is not one of these four is served with Twenty Twenty-Five, as WordPress falls back to its default theme, and activating a theme that is not built in answers 501. Child block themes resolve over their parent as PHP does, but a release builds in no child theme, so a site's own child theme is served as Twenty Twenty-Five too. Classic themes are not served yet (planned as MVP phase 10): the first, Hello Elementor, is being ported. The templates, template parts, navigation menus and global styles saved in the site editor are used in place of the theme's files, as PHP does
- **Plugins:** WordPress's PHP plugins never run, so their shortcodes, blocks, routes and admin screens are missing. HyScribe's own extension runtime (MVP phase 11) is in early development: an extension is a sandboxed WebAssembly package, installed with `./hyscribe plugin install <file>` when its signature `<file>.minisig` verifies against a key in `EXTENSION_TRUSTED_KEYS` (or with `--unsigned`), and removed with `./hyscribe plugin delete <id>`. No popular plugin is ported yet. The Plugins screen lists the site's plugins as PHP does (missing ones are listed without PHP's write), each installed extension as the plugin it stands in for, and activates and deactivates installed extensions; as under WordPress's `DISALLOW_FILE_MODS`, nothing is installed, updated or deleted from the browser. `./hyscribe plugin install <name>` and `./hyscribe plugin update` install from a hosted registry (`EXTENSION_REGISTRY`), which has no address yet, and a release carries no registry keys yet
- **Uploads:** every type in WordPress's default allowed list can be uploaded through hs-admin and the REST media route, images (JPEG, PNG, GIF, WebP, AVIF) with their sizes made. PDFs are kept without preview images (PHP renders them through Ghostscript), MP3 files are read with their tags and cover pictures, MP4, M4A, M4V, QuickTime and 3GPP files with their cover pictures, and Word, Excel and PowerPoint 97 to 2003 files upload. Still 501: the metadata of Ogg, FLAC, ASF, Matroska, AVI and FLV files, MP3, MP4 and old Office files with unusual structure (listed in ADR 0020), bytes whose type PHP's libmagic would call `data`, the async uploader's long answer, a REST upload whose sizes would leave the attachment's folder and a custom `upload_path`. Images over the size threshold are scaled and rotated photos turned as PHP does (`-scaled`, `-rotated`). Everything under `uploads/` is served, shown in place for images, audio, video, PDF and text and sent as a download otherwise. Elsewhere under `/wp-content/` and `/wp-includes/` only the files built into the binary are served: WordPress's `wp-includes` and the four built-in themes
- **Scheduled tasks:** in primary mode HyScribe runs WordPress's six core jobs (publishing scheduled posts, emptying the trash, deleting expired transients, old auto-drafts and personal data exports, failing stale privacy requests). Events of plugins and the update checks stay in the `cron` option until PHP WordPress runs its cron again (see [maintenance.md](maintenance.md))
- **Mail:** sent only through an SMTP relay named by `SMTP_HOST` ([configuration.md](configuration.md#mail)); without one none is sent and users cannot reset their own passwords. Sent: password resets (lost password form, Users list, Edit User), new user notices (Add User, and the registration form's, to the admin and to the user with a link to set a password), the profile's email change confirmation, the admin email confirmation and change notice, comment notices, and the password and email change notices of a user's update (Edit User, the profile, REST). Also sent: the notice to the admin that a personal data request was confirmed. Not sent yet: the other personal data request mails (the confirmation request, the export ready and erasure done notices; their actions answer 501)
- **Outbound fetches:** off unless `HTTP_OUTBOUND` is set ([ADR 0019](https://github.com/hyscaler/HyScribe/blob/main/docs/adr/0019-outbound-http-client.md)); until then an uncached embed stays as its link, the RSS block shows its error (with fetches on it still reaches only the hosts of `RSS_ALLOWED_HOSTS`, empty by default, and so do the redirect hops of a feed or an embed; embeds reach WordPress's oEmbed providers and `EMBED_ALLOWED_HOSTS`) and link previews in the editor (`url-details`) answer 501
- **Multisite:** refused at start
- **XML-RPC, `wp-cron.php` and WordPress's other PHP files:** not served
- **A POST to a front end page:** answered 405
- **Database TLS:** not supported

## Front end

- `?rest_route=` given as an array answers 501
- Attachment pages asked for by their name are not routed yet
- A logged in user sees private posts, drafts and previews as PHP WordPress shows them, but the site prints neither the toolbar, the comment form nor the comment edit links of a logged in user: their pages match PHP WordPress for a user whose profile turns the toolbar off (Show Toolbar when viewing site) on a post without comments and with comments closed
- Author archives are off unless `AUTHOR_ARCHIVES=on` ([configuration.md](configuration.md)); with it on, an author combined with any query var but `paged` and `feed`, such as a search, a date, a term or a post (`?author=1&s=x`, `?author=1&m=2026`, `?author=1&cat=1`, `?p=1&author=1`), still answers 404, and a classic theme's `is_author()` naming an author answers 501
- The query pagination block's links under pretty permalinks use `?paged=N` where WordPress links `/page/N/`, on every archive

## Login (`/hs-login`)

`wp-login.php` redirects to `/hs-login`. Login, logout, the lost password form, the reset password form, registration (when Settings, General allows anyone to register) and the confirmation link of a personal data request work in primary mode; the reset and welcome links go out only when mail is set up. Registering takes the same turn and place as a password check under the login limits, so a flood of registrations from one address is answered as a failure to register, and the mail with the link to set a password counts against the reset mail limits. These answer 501:

- Every action sent by POST but logging in, the lost password and reset password forms and registration
- An unknown `action`, the post password form, and the confirmation link of a request whose action is neither an export nor an erasure
- A registration form with an array as `redirect_to`, which makes PHP fail after it writes
- Logging in again from the "session expired" dialog inside hs-admin

## Comments (`wp-comments-post.php`)

Posting a comment works in primary mode, with comment paging on or off, from a URL-encoded or a `multipart/form-data` body, and the commenter cookies (`comment_author_*`) fill the comment form and show a visitor their own held comments, as WordPress does. These answer 501:

- A multipart body whose parts HyScribe's reader cannot take
- A `redirect_to` sent as an array

## hs-admin screens

`wp-admin/<screen>.php` redirects to `/hs-admin/<screen>`. hs-admin works in primary mode only. These screens are served:

- Dashboard
- Posts and Pages lists, revisions
- The block editor, for writing and editing posts and pages (`post-new`, `post?action=edit`); its saves go through the REST API
- The Site Editor's page, for administrators; its saves of templates, template parts, patterns and global styles go through the REST API (see below)
- Media Library and Add New Media File, with uploads, attachment edit
- Comments list, comment edit
- Categories, Tags, term edit
- Users list, Add User, Profile, Edit User
- Settings: General, Writing, Reading, Discussion, Media, Permalinks, Privacy
- Tools, Import, Export, Site Health (the Status tab with HyScribe's own six checks, and the Info tab), Export Personal Data, Erase Personal Data, Themes (the list, and activating a built-in theme), Plugins (the list, and activating and deactivating installed extensions)
- Profile and Edit User also when the site is reached over HTTPS, with the application passwords list
- Updates, refused as PHP refuses it (no user holds `update_core`)

Everything else answers 501, notably:

- Appearance screens other than the Themes list and the Site Editor, the Customizer
- Themes, Import and Export given other arguments (activating a theme that is not built in, importing or exporting)
- Any form sent as `multipart/form-data` other than an upload

## hs-admin actions

Ported, through the same checks as PHP:

- Screen Options
- Posts and pages: Quick Draft, Quick Edit, bulk edit, trash, restore, delete, empty trash, restoring a revision, the edit lock
- Comments: approve, unapprove, spam, trash, delete, reply, Quick Edit, the comment edit screen, bulk actions
- Categories and tags: add, edit, Quick Edit, delete, bulk delete
- Users: add, edit, profile, change role, delete with reassignment, password reset, log out everywhere
- Settings screens' saves, the permalink structure, creating the privacy policy page
- Dashboard: welcome panel, box order and closed boxes, hidden columns, admin color scheme
- Media: uploads, the attachment edit screen's save, attach and detach, delete
- Themes: activating a built-in theme
- Plugins: activating and deactivating an installed extension

Every other action answers 501 once its permission and nonce checks pass, for example the personal data screens (adding, resending, completing or deleting a request, the export file and the erasure steps), saving a post from the classic `post.php` form, adding an existing user, theme actions other than activating a built-in theme, and every Customizer request.

## REST API (`/wp-json/`)

Ported writes: posts and pages (with the block editor's saves and autosaves), templates and template parts, global styles, navigation menus, synced patterns, categories and tags, settings, comments and users. Reads are ported for most core routes. These answer 501:

- **Routes:** fonts, plugins, block and pattern directories, application passwords, site health, abilities, icons, media post processing and editing, template and global styles revisions, widget types' encode and render
- **Batch requests:** a batch with any request HyScribe does not port, or one to the sidebars or widgets while they would be stored
- **oEmbed:** the proxy for any URL but this site's own posts (it would fetch), and an answer PHP cached; the provider for a pretty permalink URL other than `?p=`, `?page_id=` or `?attachment_id=`, for an attachment, or for a post with a featured image
- **Sidebars and widgets:** while `retrieve_widgets()` would store the sidebars' widgets, as on a fresh install whose widgets came from a classic theme (PHP stores them on the first read; open the Widgets screen in PHP WordPress once), the edit context of any widget but the block widget, and every write
- **Writes:** media trash and restore, `featured_media` or a negative `post` on a media update, deleting an attachment used as a logo, site icon, header or classic menu item, menu items, revision deletes, new terms created inline from the editor, a custom template created from the editor
- **Inside the ported writes:** `meta` and note statuses that PHP refuses only after writing, a new password for one's own user, a sticky password protected post when the request carries a post password cookie, content whose blocks HyScribe cannot render yet, pattern meta that PHP refuses only after writing, autosave `meta`, reverting a template that has no customised post or theme file, text the database refuses
- **Arguments:** `after`, `before`, `modified_after`, `modified_before`, `meta`, `wp_pattern_category`, taxonomy queries with `include_children`, `operator=AND` or `tax_relation=OR`, hierarchical term queries
- **Bodies:** `multipart/form-data`
- **Other reads:** the root index unless `_fields` leaves out its namespaces and routes, `author_ip` on comments, block patterns, global styles with background images, dynamic blocks in the block renderer not yet checked against PHP, `url-details` while `HTTP_OUTBOUND` is off, `_fields` on templates, some `_embed` answers

Visitors cannot post comments through the REST API, as in a default WordPress install.

## Shadow mode is not a 501

In shadow mode a write that HyScribe does handle is refused with `403` and a message that the site is read only; the login screen says so and logs nobody in. Something not ported still answers 501 first.
