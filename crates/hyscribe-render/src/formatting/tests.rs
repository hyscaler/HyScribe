// SPDX-License-Identifier: AGPL-3.0-or-later

//! Expected outputs are PHP WordPress 7.1.3's, with the default install's
//! shortcodes plus `code`.

use super::*;

/// Core's shortcodes, plus the `code` one the PHP generator adds.
fn shortcode_tags() -> Vec<&'static [u8]> {
    CORE_SHORTCODE_TAGS
        .into_iter()
        .chain([&b"code"[..]])
        .collect()
}

const SMILIES_URL: &[u8] = b"http://127.0.0.1:8081/wp-includes/images/smilies/";

fn texturizes(cases: &[(&str, &str)]) {
    for &(input, expected) in cases {
        let ours = wptexturize(input.as_bytes(), &shortcode_tags());
        assert_eq!(String::from_utf8_lossy(&ours), expected, "{input:?}");
    }
}

fn converts(cases: &[(&str, &str)]) {
    for &(input, expected) in cases {
        let ours = convert_smilies(input.as_bytes(), SMILIES_URL);
        assert_eq!(String::from_utf8_lossy(&ours), expected, "{input:?}");
    }
}

#[test]
fn shortcodes_need_registering_and_mentioning() {
    // Without registered names, a shortcode is text.
    assert_eq!(wptexturize(b"[code]\"x\"", &[]), b"[code]&#8221;x&#8221;");
    // A registered name only counts where the text mentions it.
    assert_eq!(shortcode_names(b"[a [/b [/ [c=1 [] [&"), [b"a", b"b", b"c"]);
}

#[test]
fn texturizes_invalid_utf8() {
    assert_eq!(wptexturize(b"\xff'\xc2", &[]), b"\xff&#8217;\xc2");
}

#[test]
fn quotes_and_apostrophes() {
    texturizes(&[
        ("\"Hello,\" she said.", "&#8220;Hello,&#8221; she said."),
        ("'Single,' he said.", "&#8216;Single,&#8217; he said."),
        ("It's John's.", "It&#8217;s John&#8217;s."),
        ("don't 'quote' me", "don&#8217;t &#8216;quote&#8217; me"),
        (
            "'cause 'tis 'tain't 'twere 'twas 'twill 'til 'bout 'nuff 'round 'em",
            "&#8217;cause &#8217;tis &#8217;tain&#8217;t &#8217;twere &#8217;twas &#8217;twill &#8217;til &#8217;bout &#8217;nuff &#8217;round &#8217;em",
        ),
        (
            "(\"a\") ['b'] {\"c\"} -\"d\" -'e'",
            "(&#8220;a&#8221;) [&#8216;b&#8217;] {&#8220;c&#8221;} -&#8220;d&#8221; -&#8216;e&#8217;",
        ),
        (
            "&lt;\"a\"&gt; &lt;'b'&gt;",
            "&lt;&#8220;a&#8221;&gt; &lt;&#8216;b&#8217;&gt;",
        ),
        ("x'", "x&#8217;"),
        ("'", "&#8216;"),
        ("\"", "&#8220;"),
        ("``a''", "&#8220;a&#8221;"),
        ("a' b", "a&#8217; b"),
        ("a'\tb", "a&#8217;\tb"),
        ("a'\u{a0}b", "a&#8217;\u{a0}b"),
        ("a'&nbsp;b", "a&#8217;&nbsp;b"),
        ("\u{a0}'x'", "\u{a0}&#8216;x&#8217;"),
        ("&nbsp;\"x\"", "&nbsp;&#8220;x&#8221;"),
        ("\"\u{a0}x\"", "&#8221;\u{a0}x&#8221;"),
        ("a'.", "a&#8217;."),
        ("a',", "a&#8217;,"),
        ("a':", "a&#8217;:"),
        ("a';", "a&#8217;;"),
        ("a'!", "a&#8217;!"),
        ("a'?", "a&#8217;?"),
        ("a'\"", "a'&#8221;"),
        ("a''", "a&#8221;"),
        ("a'(", "a'("),
        ("a')", "a&#8217;)"),
        ("a'{", "a'{"),
        ("a'}", "a&#8217;}"),
        ("a'[", "a'["),
        ("a']", "a&#8217;]"),
        ("a'-", "a&#8217;-"),
        ("a'&lt;", "a'&lt;"),
        ("a'&gt;", "a&#8217;&gt;"),
        ("a'&amp;", "a&#8217;&amp;"),
        ("\"x\" ", "&#8220;x&#8221; "),
        ("\"x\"\n", "&#8220;x&#8221;\n"),
        ("a\"b", "a&#8221;b"),
        ("\" x\"", "&#8221; x&#8221;"),
        ("\"x\"&gt;", "&#8220;x&#8221;&gt;"),
        ("\"x\"-", "&#8220;x&#8221;-"),
        ("'x'\n", "&#8216;x&#8217;\n"),
        ("\"a\"b\"", "&#8220;a&#8221;b&#8221;"),
        ("'a'b'", "&#8216;a&#8217;b&#8217;"),
    ]);
}

#[test]
fn years_and_numbers() {
    texturizes(&[
        (
            "'99 '99s '99's '9 '99% '999 '99.0 '99,5 '99. '99,",
            "&#8217;99 &#8217;99s &#8217;99&#8217;s &#8216;9 &#8216;99% &#8216;999 &#8216;99.0 &#8216;99,5 &#8217;99. &#8217;99,",
        ),
        (
            "'99' '99\" '99'. '99\"!",
            "&#8217;99&#8217; &#8217;99&#8221; &#8217;99&#8217;. &#8217;99&#8221;!",
        ),
        ("'99'x '99\"x", "&#8217;99&#8217;x &#8217;99&#8243;x"),
        (
            "'42' '0.42' \"42\" \"4,2.\" '4x' \"4x\"",
            "&#8217;42&#8217; &#8216;0.42&#8217; &#8220;42&#8221; &#8220;4,2.&#8221; &#8216;4x&#8217; &#8220;4x&#8221;",
        ),
        ("x '12' y", "x &#8217;12&#8217; y"),
        ("'12'x", "&#8217;12&#8217;x"),
        ("x\"12\"", "x&#8221;12&#8243;"),
        ("x'12'", "x&#8217;12&#8217;"),
    ]);
}

#[test]
fn primes() {
    texturizes(&[
        (
            "9' 9\" 5'10\" 6' tall",
            "9&#8242; 9&#8243; 5&#8217;10&#8221; 6&#8242; tall",
        ),
        (
            "\"It's 9' long,\" he said.",
            "&#8220;It&#8217;s 9&#8242; long,&#8221; he said.",
        ),
        (
            "'It's 9' long,' he said.",
            "&#8216;It&#8217;s 9&#8242; long,&#8217; he said.",
        ),
        ("\"a 9\" b 7\".\"", "&#8220;a 9&#8243; b 7&#8243;.&#8221;"),
        ("\"5\" and 6\"", "&#8220;5&#8221; and 6&#8243;"),
        ("\"x\" y\" z\".", "&#8220;x&#8221; y&#8221; z&#8221;."),
        ("'a' b' c'.", "&#8216;a&#8217; b&#8217; c&#8217;."),
        ("\"a 2\" b 3\" c\"", "&#8220;a 2&#8243; b 3&#8243; c&#8221;"),
        ("\"one 2\" three 4\"", "&#8220;one 2&#8243; three 4&#8221;"),
        ("'one 2' three 4'.", "&#8216;one 2&#8242; three 4&#8217;."),
        ("\"a 2\" b 3\".", "&#8220;a 2&#8243; b 3&#8221;."),
        ("\"2\" 3\"", "&#8220;2&#8221; 3&#8243;"),
        ("'2' 3'", "&#8216;2&#8217; 3&#8242;"),
        ("\"x 2\"y 3\"z", "&#8220;x 2&#8243;y 3&#8243;z"),
        ("'x 2'y 3'z", "&#8216;x 2&#8217;y 3&#8217;z"),
        ("\"x\" y\" 3\"", "&#8220;x&#8221; y&#8221; 3&#8243;"),
        ("9'&gt;", "9&#8242;&gt;"),
        ("9'&lt;", "9&#8242;&lt;"),
        ("'x' y' 3'", "&#8216;x&#8217; y&#8217; 3&#8242;"),
        ("\"x 'y' z\"", "&#8220;x &#8216;y&#8217; z&#8221;"),
        ("'x \"y\" z'", "&#8216;x &#8220;y&#8221; z&#8217;"),
    ]);
}

#[test]
fn dashes_and_symbols() {
    texturizes(&[
        ("a - b", "a &#8211; b"),
        ("a -- b", "a &#8212; b"),
        ("a --- b", "a &#8212; b"),
        ("a--b", "a&#8211;b"),
        ("a---b", "a&#8212;b"),
        ("a----b", "a&#8212;-b"),
        ("xn--abc", "xn--abc"),
        ("-", "&#8211;"),
        ("--", "&#8212;"),
        ("-\n", "&#8211;\n"),
        ("\u{a0}-\u{a0}", "\u{a0}&#8211;\u{a0}"),
        ("&nbsp;--&nbsp;", "&nbsp;&#8212;&nbsp;"),
        ("a-b", "a-b"),
        ("Wait...", "Wait&#8230;"),
        ("Name (tm)", "Name &#8482;"),
        ("Name(tm)", "Name(tm)"),
    ]);
}

#[test]
fn times() {
    texturizes(&[
        ("9x9", "9&#215;9"),
        ("10x10", "10&#215;10"),
        ("1.5x2.5", "1.5&#215;2.5"),
        ("0x9999", "0x9999"),
        ("0x1", "0x1"),
        ("00x1", "00&#215;1"),
        ("1x2x3", "1x2x3"),
        ("a9x9", "a9x9"),
        ("9x9a", "9x9a"),
        ("9x9.", "9&#215;9."),
        ("1.x2 3x4", "1.&#215;2 3&#215;4"),
        ("1.x2", "1.x2"),
        ("12 3x4", "12 3&#215;4"),
        ("9x.9", "9x.9"),
        ("9x9_", "9x9_"),
        ("1x2.,3", "1&#215;2.,3"),
        ("1x2.,", "1&#215;2.,"),
        ("01.x2", "01.x2"),
        ("12 34 5x6", "12 34 5&#215;6"),
        ("9x.9 1x1", "9x.9 1&#215;1"),
        ("123x4.,", "123&#215;4.,"),
        ("a 9x9 b", "a 9&#215;9 b"),
        ("12345x6x7", "12345x6x7"),
        ("9x9 and 1.x2", "9&#215;9 and 1.&#215;2"),
        ("1x2.a3x4", "1&#215;2.a3x4"),
    ]);
}

#[test]
fn ampersands() {
    texturizes(&[
        ("a & b", "a &#038; b"),
        (
            "&amp; &AMP; &#038; &#x26; &#X2a; &#xZZ; &#; &#x; &a1; &a5; &; &",
            "&amp; &AMP; &#038; &#x26; &#X2a; &#038;#xZZ; &#038;#; &#038;#x; &a1; &#038;a5; &#038;; &#038;",
        ),
        ("&abcdefgh; &abcdefghi;", "&abcdefgh; &#038;abcdefghi;"),
        ("<a href=\"x&y\">&</a>", "<a href=\"x&#038;y\">&#038;</a>"),
        ("<a title=\"&amp;&x\">", "<a title=\"&amp;&#038;x\">"),
    ]);
}

#[test]
fn html() {
    texturizes(&[
        ("<p>\"a\"</p>", "<p>&#8220;a&#8221;</p>"),
        (
            "<a href=\"it's\">it's</a>",
            "<a href=\"it's\">it&#8217;s</a>",
        ),
        ("<!-- \"c\" -->\"d\"", "<!-- \"c\" -->&#8220;d&#8221;"),
        ("<!-- \"unclosed", "<!-- \"unclosed"),
        ("<!-->\"x\"", "<!-->&#8220;x&#8221;"),
        ("<!--->\"x\"", "<!--->&#8220;x&#8221;"),
        ("<!-- a -- b -->\"x\"", "<!-- a -- b -->&#8220;x&#8221;"),
        (
            "<!-- a & \"b\" -->\"c\"",
            "<!-- a & \"b\" -->&#8220;c&#8221;",
        ),
        ("abc<!-->\"x\"", "abc<!-->&#8220;x&#8221;"),
        (
            "<!-- a > \"b\" -->\"c\"",
            "<!-- a > \"b\" -->&#8220;c&#8221;",
        ),
        ("x <!-- <pre> -->\"c\"", "x <!-- <pre> -->&#8220;c&#8221;"),
        ("<\"x\"", "<\"x\""),
        ("a < b > \"c\"", "a < b > &#8220;c&#8221;"),
        ("<p", "<p"),
        ("<<p>>\"x\"", "<<p>>&#8221;x&#8221;"),
        ("<", "<"),
        ("</>\"x\"", "</>&#8220;x&#8221;"),
    ]);
}

#[test]
fn no_texturize_elements() {
    texturizes(&[
        ("<pre>\"x\"</pre>\"y\"", "<pre>\"x\"</pre>&#8220;y&#8221;"),
        (
            "<code>\"x\"</code>\"y\"",
            "<code>\"x\"</code>&#8220;y&#8221;",
        ),
        (
            "<kbd>\"x\"</kbd><style>\"x\"</style><script>\"x\"</script><tt>\"x\"</tt>\"y\"",
            "<kbd>\"x\"</kbd><style>\"x\"</style><script>\"x\"</script><tt>\"x\"</tt>&#8220;y&#8221;",
        ),
        (
            "<pre class=\"a\">\"x\"</pre>\"y\"",
            "<pre class=\"a\">\"x\"</pre>&#8220;y&#8221;",
        ),
        ("<PRE>\"x\"</PRE>", "<PRE>&#8220;x&#8221;</PRE>"),
        ("<prex>\"x\"</prex>", "<prex>&#8220;x&#8221;</prex>"),
        ("<pre>\"x\"", "<pre>\"x\""),
        ("</pre>\"x\"", "</pre>&#8220;x&#8221;"),
        (
            "<pre><code>\"x\"</code>\"y\"</pre>\"z\"",
            "<pre><code>\"x\"</code>\"y\"</pre>&#8220;z&#8221;",
        ),
        (
            "<pre>\"a\"</code>\"b\"</pre>\"c\"",
            "<pre>\"a\"</code>\"b\"</pre>&#8220;c&#8221;",
        ),
        (
            "<pre\n>\"x\"</pre>\"y\"",
            "<pre\n>&#8220;x&#8221;</pre>&#8220;y&#8221;",
        ),
        ("<pre>\"x\"</pre >\"y\"", "<pre>\"x\"</pre >&#8220;y&#8221;"),
        ("<pre/>\"x\"", "<pre/>&#8220;x&#8221;"),
        ("<pre \"x\"", "<pre \"x\""),
        ("<pre>&</pre>", "<pre>&</pre>"),
    ]);
}

#[test]
fn shortcodes() {
    texturizes(&[
        (
            "[caption]\"x\"[/caption]",
            "[caption]&#8220;x&#8221;[/caption]",
        ),
        (
            "[caption id=\"1\"]\"x\"[/caption]",
            "[caption id=\"1\"]&#8220;x&#8221;[/caption]",
        ),
        ("[[caption]]\"x\"", "[[caption]]&#8220;x&#8221;"),
        (
            "[[caption]\"x\"[/caption]]",
            "[[caption]&#8220;x&#8221;[/caption]]",
        ),
        ("[caption]]\"x\"", "[caption]]&#8220;x&#8221;"),
        ("[caption/]\"x\"", "[caption/]&#8220;x&#8221;"),
        ("[captionx]\"x\"", "[captionx]&#8221;x&#8221;"),
        ("[caption=1]\"x\"", "[caption=1]&#8221;x&#8221;"),
        (
            "[caption <b>\"x\"</b>]\"y\"",
            "[caption <b>\"x\"</b>]&#8220;y&#8221;",
        ),
        (
            "[caption \"x\" <b]\"y\"",
            "[caption &#8220;x&#8221; <b]\"y\"",
        ),
        ("[caption \"x\"", "[caption &#8220;x&#8221;"),
        ("[caption >]\"x\"", "[caption >]&#8221;x&#8221;"),
        (
            "[code]\"x\"[/code]\"y\"",
            "[code]\"x\"[/code]&#8220;y&#8221;",
        ),
        (
            "[code]\"a\"[code]\"b\"[/code]\"c\"[/code]\"d\"",
            "[code]\"a\"[code]\"b\"[/code]\"c\"[/code]&#8220;d&#8221;",
        ),
        ("[[code]]\"x\"", "[[code]]&#8220;x&#8221;"),
        ("[code]]\"x\"", "[code]]&#8220;x&#8221;"),
        ("[/code]\"x\"", "[/code]&#8220;x&#8221;"),
        (
            "[code]<pre>\"a\"[/code]\"b\"</pre>\"c\"",
            "[code]<pre>\"a\"[/code]\"b\"</pre>&#8220;c&#8221;",
        ),
        ("[code\t]\"x\"[/code]", "[code\t]&#8220;x&#8221;[/code]"),
        ("[code\n]\"x\"[/code]", "[code\n]&#8220;x&#8221;[/code]"),
        ("[code\x0b]\"x\"[/code]", "[code\x0b]&#8220;x&#8221;[/code]"),
        ("[code\x0c]\"x\"[/code]", "[code\x0c]&#8220;x&#8221;[/code]"),
        ("[code\r]\"x\"[/code]", "[code\r]&#8220;x&#8221;[/code]"),
        ("[unknown]\"x\"", "[unknown]&#8221;x&#8221;"),
        ("[]\"x\"", "[]&#8221;x&#8221;"),
        ("[/]\"x\"", "[/]&#8221;x&#8221;"),
        ("[&]\"x\"", "[&#038;]&#8221;x&#8221;"),
        ("[//code]\"x\"", "[//code]&#8221;x&#8221;"),
        ("[/[code]\"x\"", "[/[code]\"x\""),
        ("[code/]\"x\"", "[code/]&#8220;x&#8221;"),
        ("[x=\"[code]\"]\"y\"", "[x=&#8221;[code]\"]\"y\""),
        ("[code <b<c>]\"x\"", "[code <b<c>]\"x\""),
        ("[code <b>]\"x\"[/code]", "[code <b>]\"x\"[/code]"),
        ("[code", "[code"),
        ("[ code]\"x\"", "[ code]&#8221;x&#8221;"),
        ("[code x]]\"y\"", "[code x]]&#8220;y&#8221;"),
        ("[code <b]]\"x\"", "[code <b]]\"x\""),
    ]);
}

#[test]
fn smilies() {
    converts(&[
        ("", ""),
        (":)", "\u{1f642}"),
        (" :) ", " \u{1f642} "),
        ("a:)", "a:)"),
        (":)a", ":)a"),
        (":):)", ":):)"),
        (":)\n:(", "\u{1f642}\n\u{1f641}"),
        ("\t:)\r", "\t\u{1f642}\r"),
        ("\u{a0}:)\u{a0}", "\u{a0}\u{1f642}\u{a0}"),
        ("&nbsp;:)&nbsp;", "&nbsp;\u{1f642}&nbsp;"),
        (
            ":mrgreen:",
            "<img src=\"http://127.0.0.1:8081/wp-includes/images/smilies/mrgreen.png\" alt=\":mrgreen:\" class=\"wp-smiley\" style=\"height: 1em; max-height: 1em;\" />",
        ),
        (
            ":???: :?: :? :!: 8-O 8O ;) :-|",
            "\u{1f615} \u{2753} \u{1f615} \u{2757} \u{1f62f} \u{1f62f} \u{1f609} \u{1f610}",
        ),
        ("x:mrgreen:", "x:mrgreen:"),
        (":??", ":??"),
    ]);
}

#[test]
fn smilies_ignored_elements() {
    converts(&[
        ("<p> :) </p>", "<p> \u{1f642} </p>"),
        ("<code> :) </code> :)", "<code> :) </code> \u{1f642}"),
        (
            "<code class=\"x\"> :) </code> :)",
            "<code class=\"x\"> :) </code> \u{1f642}",
        ),
        ("<pre> :) </pre> :)", "<pre> :) </pre> \u{1f642}"),
        (
            "<style> :) </style><script> :) </script><textarea> :) </textarea> :)",
            "<style> :) </style><script> :) </script><textarea> :) </textarea> \u{1f642}",
        ),
        ("<codex> :) </codex> :)", "<codex> :) </codex> :)"),
        ("<code> :) </CODE> :)", "<code> :) </CODE> :)"),
        ("<CODE> :) </CODE> :)", "<CODE> \u{1f642} </CODE> \u{1f642}"),
        (
            "<code><pre> :) </pre> :) </code> :)",
            "<code><pre> :) </pre> :) </code> \u{1f642}",
        ),
        ("<code> :) </code > :)", "<code> :) </code > :)"),
        ("<p><a :)", "<p><a :)"),
        ("<a :)", "<a :)"),
        ("x<a :)", "x<a \u{1f642}"),
        (":) >", "\u{1f642} >"),
        ("<p title=\":)\"> :)", "<p title=\":)\"> \u{1f642}"),
        ("<code :)", "<code :)"),
    ]);
}

#[test]
fn convert_chars_encodes_bare_ampersands() {
    // convert_chars() in PHP WordPress 7.1.3.
    for (text, php) in [
        ("a & b", "a &#038; b"),
        ("&amp;", "&amp;"),
        ("&x;", "&#038;x;"),
        ("&#1;", "&#1;"),
        ("&abcdefghi;", "&abcdefghi;"),
        ("&&amp;", "&&amp;"),
        ("&", "&"),
        ("&AMP;x", "&AMP;x"),
        ("&a5;", "&#038;a5;"),
        ("x&\n", "x&#038;\n"),
    ] {
        assert_eq!(convert_chars(text.as_bytes()), php.as_bytes(), "{text:?}");
    }
}

#[test]
fn capital_p_dangit_in_titles() {
    for (text, php) in [
        ("Wordpress", "WordPress"),
        ("wordpress Wordpress!", "wordpress WordPress!"),
        ("WordPress", "WordPress"),
        ("Wordpres", "Wordpres"),
    ] {
        assert_eq!(
            capital_p_dangit_title(text.as_bytes()),
            php.as_bytes(),
            "{text:?}"
        );
    }
}
