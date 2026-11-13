// SPDX-License-Identifier: AGPL-3.0-or-later

//! The avatar block with `get_avatar()` and `get_avatar_data()`: Gravatar
//! images by the SHA-256 of the email address.

use hyscribe_kses::{esc_attr, esc_url};
use hyscribe_model::{Comment, User};
use hyscribe_phpser::{Key, Value, bin2hex, is_empty_string, isset, php_trim, rawurlencode};
use sha2::{Digest, Sha256};

use super::{Block, Context};
use crate::style::get_styles;

/// Whose avatar to show.
pub(super) enum Subject<'a> {
    User(Option<User>),
    Comment(&'a Comment),
}

/// `render_block_core_avatar()`.
pub(super) fn avatar(context: &Context<'_>, block: &Block<'_>) -> Vec<u8> {
    let size = block.attribute("size").map_or(96, Value::to_int);
    let wrapper = block.wrapper(context, b"");
    let (border_class, border_style) = border_attributes(block);
    let class = if border_class.is_empty() {
        "wp-block-avatar__image".to_owned()
    } else {
        format!("wp-block-avatar__image {border_class}")
    };
    // get_avatar() escapes the class but not the extra attributes.
    let style = if is_empty_string(&border_style) {
        String::new()
    } else {
        format!(" style=\"{}\"", esc_attr(&border_style))
    };
    let link = block.attribute("isLink").is_some_and(|v| !v.is_empty());
    let target = block
        .attribute("linkTarget")
        .map(Value::to_php_string)
        .unwrap_or_default();
    let blank = target == b"_blank";
    let mut image;
    match &context.comment {
        None => {
            let author = match block.attribute("userId") {
                Some(id) => id.clone(),
                // Without a post, get_query_var( 'author' ), which is empty
                // as author archives are not routed.
                None => Value::String(
                    context
                        .post()
                        .map(|post| post.author.to_string().into_bytes())
                        .unwrap_or_default(),
                ),
            };
            if author.is_empty() {
                return Vec::new();
            }
            // The attribute is a number. get_avatar() takes its absolute
            // value, get_userdata() only IDs of 1 and up.
            let id = author.to_int();
            let user = context.site.user(id.unsigned_abs());
            let name = user
                .as_ref()
                .filter(|_| id >= 1)
                .map(|user| user.display_name.clone())
                .unwrap_or_default();
            let alt = [&name[..], b" Avatar"].concat();
            image = get_avatar(context, &Subject::User(user), size, &alt, &class, &style);
            if link {
                let url = crate::permalinks::author_posts_url(context.site, id);
                let label = blank
                    .then(|| [&b"("[..], &name, b" author archive, opens in a new tab)"].concat());
                image = link_to(&esc_url(&url), &target, label, &image);
            }
        }
        Some(comment) => {
            let alt = [&comment.author[..], b" Avatar"].concat();
            image = get_avatar(
                context,
                &Subject::Comment(comment),
                size,
                &alt,
                &class,
                &style,
            );
            if link && !comment.author_url.is_empty() {
                let label = blank.then(|| {
                    [
                        &b"("[..],
                        &comment.author,
                        b" website link, opens in a new tab)",
                    ]
                    .concat()
                });
                image = link_to(&esc_url(&comment.author_url), &target, label, &image);
            }
        }
    }
    [format!("<div {wrapper}>").as_bytes(), &image, b"</div>"].concat()
}

fn link_to(url: &[u8], target: &[u8], label: Option<Vec<u8>>, image: &[u8]) -> Vec<u8> {
    let label = label.map_or(String::new(), |label| {
        format!("aria-label=\"{}\"", esc_attr(&label))
    });
    [
        &b"<a href=\""[..],
        url,
        b"\" target=\"",
        esc_attr(target).as_bytes(),
        b"\" ",
        label.as_bytes(),
        b" class=\"wp-block-avatar__link\">",
        image,
        b"</a>",
    ]
    .concat()
}

/// `get_block_core_avatar_border_attributes()`: the border's class names
/// and declarations, from the style engine.
pub(super) fn border_attributes(block: &Block<'_>) -> (String, Vec<u8>) {
    let border = block.attributes.get_path(&["style", "border"]);
    let field = |value: Option<&Value>, name: &str| {
        value
            .and_then(|v| v.get_str(name))
            .cloned()
            .unwrap_or(Value::Null)
    };
    let key = |name: &str| Key::String(name.as_bytes().to_vec());
    let mut styles = Vec::new();
    for name in ["radius", "style", "width"] {
        let value = field(border, name);
        if isset(Some(&value)) {
            styles.push((key(name), value));
        }
    }
    let color = match block.attributes.get_str("borderColor") {
        Some(preset) => {
            Value::String([&b"var:preset|color|"[..], &preset.to_php_string()].concat())
        }
        None => field(border, "color"),
    };
    styles.push((key("color"), color));
    for side in ["top", "right", "bottom", "left"] {
        let side_value = border.and_then(|b| b.get_str(side));
        let parts = ["color", "style", "width"]
            .map(|name| (key(name), field(side_value, name)))
            .to_vec();
        styles.push((key(side), Value::Array(parts)));
    }
    let styles = get_styles(
        &Value::Array(vec![(key("border"), Value::Array(styles))]),
        false,
    );
    (styles.classnames, styles.css)
}

/// `get_avatar( $id_or_email, $size, '', $alt, array( 'extra_attr' =>
/// $extra, 'class' => $class ) )`, or empty where it returns `false`.
pub(super) fn get_avatar(
    context: &Context<'_>,
    subject: &Subject<'_>,
    size: i64,
    alt: &[u8],
    class: &str,
    extra: &str,
) -> Vec<u8> {
    let site = context.site;
    let optimization = context.loading.img(size, size);
    if !shows_avatars(&|name| site.option(name)) {
        return Vec::new();
    }
    let (Some((url2x, _)), Some((url, found))) = (
        avatar_data(context, subject, size.saturating_mul(2)),
        avatar_data(context, subject, size),
    ) else {
        return Vec::new();
    };
    // get_avatar() adds these unless the extra attributes have them, and
    // the style, through safecss_filter_attr(), never holds an `=`.
    let mut extra = extra.to_owned();
    for (name, value) in [
        ("loading", optimization.loading),
        ("fetchpriority", optimization.fetchpriority),
        ("decoding", optimization.decoding),
    ] {
        if let Some(value) = value {
            if !extra.is_empty() {
                extra.push(' ');
            }
            extra.push_str(&format!("{name}='{value}'"));
        }
    }
    img(alt, (&url, &url2x, found), size, class, &extra)
}

/// `get_option( 'show_avatars' )`.
fn shows_avatars(option: &dyn Fn(&str) -> Option<Vec<u8>>) -> bool {
    option("show_avatars").is_some_and(|v| !is_empty_string(&v))
}

/// The `<img>` `get_avatar()` prints, with its URLs at the size and twice it.
fn img(
    alt: &[u8],
    (url, url2x, found): (&[u8], &[u8], bool),
    size: i64,
    class: &str,
    extra: &str,
) -> Vec<u8> {
    let size = data_size(size);
    let mut classes = vec![
        "avatar".to_owned(),
        format!("avatar-{size}"),
        "photo".to_owned(),
    ];
    if !found {
        classes.push("avatar-default".to_owned());
    }
    if !class.is_empty() {
        classes.push(class.to_owned());
    }
    format!(
        "<img alt='{}' src='{}' srcset='{} 2x' class='{}' height='{size}' width='{size}' {extra}/>",
        esc_attr(alt),
        String::from_utf8_lossy(&esc_url(url)),
        String::from_utf8_lossy(&esc_url(url2x)),
        esc_attr(classes.join(" ").as_bytes()),
    )
    .into_bytes()
}

/// `get_avatar( $user_id, $size )` in the admin, where avatars load lazily:
/// empty when the site shows none.
pub fn user_avatar(option: &dyn Fn(&str) -> Option<Vec<u8>>, user: &User, size: i64) -> Vec<u8> {
    if !shows_avatars(option) {
        return Vec::new();
    }
    let name = user_name(user);
    let (url, found) = gravatar(option, &user.email, &name, size, false);
    let (url2x, _) = gravatar(option, &user.email, &name, size.saturating_mul(2), false);
    img(
        b"",
        (&url, &url2x, found),
        size,
        "",
        "loading='lazy' decoding='async'",
    )
}

/// `get_avatar( $comment, $size, 'mystery' )` in the admin, for the
/// commenter's `email`: empty when the site shows none.
pub fn comment_avatar(
    option: &dyn Fn(&str) -> Option<Vec<u8>>,
    email: &[u8],
    size: i64,
) -> Vec<u8> {
    if !shows_avatars(option) {
        return Vec::new();
    }
    let mystery = |name: &str| match name {
        "avatar_default" => Some(b"mystery".to_vec()),
        name => option(name),
    };
    let (url, found) = gravatar(&mystery, email, b"", size, false);
    let (url2x, _) = gravatar(&mystery, email, b"", size.saturating_mul(2), false);
    img(
        b"",
        (&url, &url2x, found),
        size,
        "",
        "loading='lazy' decoding='async'",
    )
}

/// `get_avatar( $email, $size, $default, '', array( 'force_default' => true ) )`
/// as the Discussion settings show each default, whether or not avatars show.
pub fn default_avatar(
    option: &dyn Fn(&str) -> Option<Vec<u8>>,
    email: &[u8],
    default: &str,
    size: i64,
) -> Vec<u8> {
    let chosen = |name: &str| match name {
        "avatar_default" => Some(default.as_bytes().to_vec()),
        name => option(name),
    };
    // Initials come from the address's name, its dots, underscores and
    // hyphens read as spaces.
    let name: Vec<u8> = email
        .iter()
        .position(|&b| b == b'@')
        .and_then(|at| email.get(..at))
        .unwrap_or_default()
        .iter()
        .map(|&b| {
            if matches!(b, b'.' | b'_' | b'-') {
                b' '
            } else {
                b
            }
        })
        .collect();
    let (url, _) = gravatar(&chosen, email, &name, size, true);
    let (url2x, _) = gravatar(&chosen, email, &name, size.saturating_mul(2), true);
    img(
        b"",
        (&url, &url2x, false),
        size,
        "",
        "loading='lazy' decoding='async'",
    )
}

/// The name a user's initials come from.
fn user_name(user: &User) -> Vec<u8> {
    if !user.display_name.is_empty() {
        user.display_name.clone()
    } else if !user.first_name.is_empty() && !user.last_name.is_empty() {
        [&user.first_name[..], b" ", &user.last_name].concat()
    } else {
        user.login.clone()
    }
}

/// `get_avatar_data()`'s size: the absolute value, or 96 for none.
fn data_size(size: i64) -> u64 {
    match size.unsigned_abs() {
        0 => 96,
        size => size,
    }
}

/// `get_avatar_data()`: the URL and whether an email address was found,
/// `None` for a comment type that has no avatars.
fn avatar_data(context: &Context<'_>, subject: &Subject<'_>, size: i64) -> Option<(Vec<u8>, bool)> {
    let site = context.site;
    let user = match subject {
        Subject::User(user) => user.clone(),
        Subject::Comment(comment) => {
            // is_avatar_comment_type( get_comment_type( $comment ) )
            if !matches!(&comment.comment_type[..], b"" | b"comment" | b"note") {
                return None;
            }
            (comment.user_id != 0)
                .then(|| site.user(comment.user_id))
                .flatten()
        }
    };
    let (email, name) = match (&user, subject) {
        (Some(user), _) => (user.email.clone(), user_name(user)),
        (None, Subject::Comment(comment)) => (comment.author_email.clone(), comment.author.clone()),
        (None, Subject::User(_)) => (Vec::new(), Vec::new()),
    };
    Some(gravatar(
        &|name| site.option(name),
        &email,
        &name,
        size,
        false,
    ))
}

/// The Gravatar URL `get_avatar_data()` makes for `email` from the site's
/// avatar options, with initials from `name` where they are the default,
/// and whether there was an address; `force` shows the default whatever
/// Gravatar holds.
fn gravatar(
    option: &dyn Fn(&str) -> Option<Vec<u8>>,
    email: &[u8],
    name: &[u8],
    size: i64,
    force: bool,
) -> (Vec<u8>, bool) {
    // get_option( 'avatar_default', 'mystery' ): an empty value stays empty.
    let mut default = option("avatar_default").unwrap_or_else(|| b"mystery".to_vec());
    match &default[..] {
        b"mm" | b"mystery" | b"mysteryman" => default = b"mm".to_vec(),
        b"gravatar_default" => default.clear(),
        _ => {}
    }
    let rating = option("avatar_rating")
        .unwrap_or_default()
        .to_ascii_lowercase();
    let hash = if is_empty_string(email) {
        String::new()
    } else {
        bin2hex(&Sha256::digest(php_trim(email).to_ascii_lowercase()))
    };
    let mut url = format!(
        "https://secure.gravatar.com/avatar/{hash}?s={}",
        data_size(size)
    )
    .into_bytes();
    let mut arg = |key: &str, value: &[u8]| {
        if !is_empty_string(value) {
            url.extend(format!("&{key}=").into_bytes());
            url.extend(rawurlencode(value));
        }
    };
    arg("d", &default);
    if force {
        arg("f", b"y");
    }
    arg("r", &rating);
    if default == b"initials" && !name.is_empty() {
        arg("initials", &initials(name));
    }
    (url, !hash.is_empty())
}

/// The initials `get_avatar_data()` sends: the first two characters of a
/// single word or of East Asian script, else the first characters of the
/// first and last words. Names are UTF-8, as the database columns are.
fn initials(name: &[u8]) -> Vec<u8> {
    let name = String::from_utf8_lossy(name);
    let chars: Vec<char> = name.chars().collect();
    let east_asian = chars.iter().any(|&c| is_east_asian(c));
    let picked: String = match name.rfind(' ').filter(|_| !east_asian) {
        None => chars.iter().take(2).collect(),
        // strrpos() gives a byte offset, which mb_substr() reads as characters.
        Some(space) => chars
            .first()
            .into_iter()
            .chain(chars.get(space + 1))
            .collect(),
    };
    picked.into_bytes()
}

/// `\p{Han}`, `\p{Hiragana}`, `\p{Katakana}` or `\p{Hangul}`.
fn is_east_asian(c: char) -> bool {
    const RANGES: [(u32, u32); 56] = [
        (0x1100, 0x11FF),
        (0x2E80, 0x2E99),
        (0x2E9B, 0x2EF3),
        (0x2F00, 0x2FD5),
        (0x3005, 0x3005),
        (0x3007, 0x3007),
        (0x3021, 0x3029),
        (0x302E, 0x302F),
        (0x3038, 0x303B),
        (0x3041, 0x3096),
        (0x309D, 0x309F),
        (0x30A1, 0x30FA),
        (0x30FD, 0x30FF),
        (0x3131, 0x318E),
        (0x31F0, 0x31FF),
        (0x3200, 0x321E),
        (0x3260, 0x327E),
        (0x32D0, 0x32FE),
        (0x3300, 0x3357),
        (0x3400, 0x4DBF),
        (0x4E00, 0x9FFF),
        (0xA960, 0xA97C),
        (0xAC00, 0xD7A3),
        (0xD7B0, 0xD7C6),
        (0xD7CB, 0xD7FB),
        (0xF900, 0xFA6D),
        (0xFA70, 0xFAD9),
        (0xFF66, 0xFF6F),
        (0xFF71, 0xFF9D),
        (0xFFA0, 0xFFBE),
        (0xFFC2, 0xFFC7),
        (0xFFCA, 0xFFCF),
        (0xFFD2, 0xFFD7),
        (0xFFDA, 0xFFDC),
        (0x16FE2, 0x16FE3),
        (0x16FF0, 0x16FF1),
        (0x1AFF0, 0x1AFF3),
        (0x1AFF5, 0x1AFFB),
        (0x1AFFD, 0x1AFFE),
        (0x1B000, 0x1B000),
        (0x1B001, 0x1B11F),
        (0x1B120, 0x1B122),
        (0x1B132, 0x1B132),
        (0x1B150, 0x1B152),
        (0x1B155, 0x1B155),
        (0x1B164, 0x1B167),
        (0x1F200, 0x1F200),
        (0x20000, 0x2A6DF),
        (0x2A700, 0x2B739),
        (0x2B740, 0x2B81D),
        (0x2B820, 0x2CEA1),
        (0x2CEB0, 0x2EBE0),
        (0x2EBF0, 0x2EE5D),
        (0x2F800, 0x2FA1D),
        (0x30000, 0x3134A),
        (0x31350, 0x323AF),
    ];
    RANGES
        .iter()
        .any(|&(from, to)| (from..=to).contains(&u32::from(c)))
}
