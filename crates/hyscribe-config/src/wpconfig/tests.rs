// SPDX-License-Identifier: AGPL-3.0-or-later

use super::*;

fn php(body: &str) -> WpConfig {
    parse(format!("<?php\n{body}").as_bytes()).unwrap()
}

fn value(body: &str) -> Value {
    php(&format!("define('X', {body});"))
        .get("X")
        .unwrap()
        .value
        .clone()
}

fn computed(text: &str) -> Value {
    Value::Computed(text.to_string())
}

fn string(text: &str) -> Value {
    Value::String(text.to_string())
}

#[test]
fn reads_the_wordpress_sample() {
    let config = parse(include_bytes!("../../tests/corpus/wp-config/sample.php")).unwrap();
    assert_eq!(config.string("DB_NAME"), Some("database_name_here"));
    assert_eq!(config.string("DB_HOST"), Some("localhost"));
    assert_eq!(config.string("DB_COLLATE"), Some(""));
    assert_eq!(config.table_prefix, Some(string("wp_")));
    let debug = config.get("WP_DEBUG").unwrap();
    assert_eq!(debug.value, Value::Bool(false));
    assert!(!debug.conditional);
    let abspath = config.get("ABSPATH").unwrap();
    assert_eq!(abspath.value, computed("__DIR__ . '/'"));
    assert!(abspath.conditional);
}

#[test]
fn literals() {
    assert_eq!(value("'a'"), string("a"));
    assert_eq!(value("42"), Value::Int(42));
    assert_eq!(value("-7"), Value::Int(-7));
    assert_eq!(value("+7"), Value::Int(7));
    assert_eq!(value("0x1F"), Value::Int(31));
    assert_eq!(value("0B101"), Value::Int(5));
    assert_eq!(value("0o17"), Value::Int(15));
    assert_eq!(value("017"), Value::Int(15));
    assert_eq!(value("0"), Value::Int(0));
    assert_eq!(value("1_000"), Value::Int(1000));
    assert_eq!(value("TRUE"), Value::Bool(true));
    assert_eq!(value("False"), Value::Bool(false));
    assert_eq!(value("null"), Value::Null);
    assert_eq!(value("1.5"), computed("1.5"));
    assert_eq!(value("1e3"), computed("1e3"));
    assert_eq!(
        value("9223372036854775808"),
        computed("9223372036854775808")
    );
    assert_eq!(value("- 'a'"), computed("- 'a'"));
    assert_eq!(value("PHP_EOL"), computed("PHP_EOL"));
    assert_eq!(value("'a' . 'b'"), computed("'a' . 'b'"));
    assert_eq!(value("array(1, 2)"), computed("array(1, 2)"));
    assert_eq!(value("[1, [2]]"), computed("[1, [2]]"));
    assert_eq!(value("f(']')"), computed("f(']')"));
    assert_eq!(value("1 ]"), computed("1 ]"));
    assert_eq!(value(""), computed(""));
    // Not UTF-8: only PHP knows what the site does with it.
    assert_eq!(
        parse(b"<?php define('X', '\xff');")
            .unwrap()
            .get("X")
            .unwrap()
            .value,
        computed("'\u{fffd}'")
    );
    assert_eq!(value("'a'").as_str(), Some("a"));
    assert_eq!(value("1").as_str(), None);
}

#[test]
fn single_quoted_strings() {
    assert_eq!(value(r"'it\'s \\ \n'"), string(r"it's \ \n"));
}

#[test]
fn double_quoted_strings() {
    assert_eq!(
        value(r#""\n\t\r\v\e\f\\\$\"""#),
        string("\n\t\r\u{b}\u{1b}\u{c}\\$\"")
    );
    assert_eq!(value(r#""\101\0\400\7""#), string("A\0\0\u{7}"));
    assert_eq!(value(r#""\x41\x4A\xZ""#), string("AJ\\xZ"));
    assert_eq!(
        value(r#""\u{43}\u{1F600}\u{110000}\u{41\u{}\q""#),
        string("C\u{1F600}\\u{110000}\\u{41\\u{}\\q")
    );
    assert_eq!(value(r#""cost: $""#), string("cost: $"));
    assert_eq!(value(r#""$name""#), computed(r#""$name""#));
    assert_eq!(value(r#""${name}""#), computed(r#""${name}""#));
}

#[test]
fn heredocs_are_computed() {
    assert_eq!(
        value("<<<EOT\n x ; ' \n EOT"),
        computed("<<<EOT\n x ; ' \n EOT")
    );
    assert_eq!(value("<<< 'EOT'\nx\nEOT"), computed("<<< 'EOT'\nx\nEOT"));
    assert_eq!(
        value("<<<\"EOT\"\nx\nEOTX\nEOT"),
        computed("<<<\"EOT\"\nx\nEOTX\nEOT")
    );
}

#[test]
fn computed_text_is_cut_for_display() {
    let long = format!("f({})", "1,".repeat(300));
    match value(&long) {
        Value::Computed(text) => assert_eq!(text.len(), MAX_SOURCE),
        other => panic!("{other:?}"),
    }
}

#[test]
fn conditional_defines() {
    let config = php("if (x) { define('BLOCK', 1); }\n\
         defined('OR') || define('OR', 2);\n\
         if (x) define('NO_BRACES', 3);\n\
         define('PLAIN', 4); } define('AFTER_STRAY_BRACE', 5);");
    let conditional = |name| config.get(name).unwrap().conditional;
    assert!(conditional("BLOCK"));
    assert!(conditional("OR"));
    assert!(conditional("NO_BRACES"));
    assert!(!conditional("PLAIN"));
    assert!(!conditional("AFTER_STRAY_BRACE"));
}

#[test]
fn the_first_define_wins() {
    let config = php("define('A', 'first'); define('A', 'second', true);");
    assert_eq!(config.string("A"), Some("first"));
    assert_eq!(config.constants.len(), 2);
    assert_eq!(config.string("MISSING"), None);
}

#[test]
fn defines_that_cannot_be_read_are_skipped() {
    let config = php(
        "define; define(NAME, 1); define('NO_COMMA'); define(\"$v\", 1); \
                      define('\u{0}'); define('\u{e9}', 1); define(",
    );
    let names: Vec<&str> = config.constants.iter().map(|c| c.name.as_str()).collect();
    assert_eq!(names, ["\u{e9}"]);
    let config = parse(b"<?php define('\xff', 1);").unwrap();
    assert!(config.constants.is_empty());
}

#[test]
fn table_prefix() {
    assert_eq!(
        php("$table_prefix = 'a_'; $table_prefix = \"b_\";").table_prefix,
        Some(string("b_"))
    );
    assert_eq!(
        php("$table_prefix = getenv('P') ?: 'wp_';").table_prefix,
        Some(computed("getenv('P') ?: 'wp_'"))
    );
    assert_eq!(
        php("$table_prefix = 'a_'; $table_prefix .= 'b';").table_prefix,
        Some(computed("compound assignment"))
    );
    assert_eq!(
        php("$table_prefix = 1; $table_prefix += 1;").table_prefix,
        Some(computed("compound assignment"))
    );
    // Reads and comparisons do not change it.
    assert_eq!(
        php("$table_prefix = 'a_'; if ($table_prefix == 'x') {} echo $table_prefix . 'y'; $table_prefix;")
            .table_prefix,
        Some(string("a_"))
    );
    assert_eq!(php("$other = 'x';").table_prefix, None);
}

#[test]
fn tags_comments_and_inline_html() {
    let config = parse(
        b"define('IN_HTML', 1); <?PHP\n\
          # define('HASH', 1);\n\
          // define('SLASHES', 1);\n\
          /* define('BLOCK', 1); */\n\
          #[Attr] define('AFTER_ATTRIBUTE', 1);\n\
          // ends here ?> define('HTML', 1); <?php define('BACK', 1);\n\
          define('CUT', 1 ?> <?php define('LAST', 'yes');",
    )
    .unwrap();
    let names: Vec<&str> = config.constants.iter().map(|c| c.name.as_str()).collect();
    assert_eq!(names, ["AFTER_ATTRIBUTE", "BACK", "CUT", "LAST"]);
    assert_eq!(config.get("CUT").unwrap().value, Value::Int(1));
    assert_eq!(parse(b"no php here").unwrap(), WpConfig::default());
    assert_eq!(
        parse(b"<?php // comment to the end").unwrap(),
        WpConfig::default()
    );
    assert_eq!(parse(b"").unwrap(), WpConfig::default());
}

#[test]
fn unterminated_input_is_an_error() {
    for (source, offset) in [
        (&b"<?php define('X', 'abc"[..], 18),
        (b"<?php define('X', \"abc", 18),
        (b"<?php define('X', \"abc\\", 18),
        (b"<?php /* never closed", 6),
        (b"<?php $x = <<<EOT\nno end", 11),
        (b"<?php $x = <<<\nempty label\n", 11),
        (b"<?php $x = <<<\"EOT\nmismatched\nEOT", 11),
        (b"<?php $x = <<<EOT", 11),
    ] {
        assert_eq!(
            parse(source),
            Err(WpConfigError::Unterminated { offset }),
            "{}",
            String::from_utf8_lossy(source)
        );
    }
    assert_eq!(
        WpConfigError::Unterminated { offset: 3 }.to_string(),
        "wp-config.php has an unterminated string or comment at byte 3"
    );
}

#[test]
fn large_files_are_refused() {
    let big = vec![b' '; MAX_SIZE + 1];
    let err = parse(&big).unwrap_err();
    assert_eq!(err, WpConfigError::TooLarge(MAX_SIZE + 1));
    assert_eq!(
        err.to_string(),
        format!(
            "wp-config.php is {} bytes; files over {MAX_SIZE} bytes are refused",
            MAX_SIZE + 1
        )
    );
    let err: Box<dyn std::error::Error> = Box::new(err);
    assert!(err.source().is_none());
    assert!(parse(&vec![b' '; MAX_SIZE]).is_ok());
}

#[test]
fn db_host_forms() {
    let host = |value: &str| parse_db_host(value).unwrap();
    let plain = |name: &str, port: Option<u64>, socket: Option<&str>, ipv6: bool| DbHost {
        host: name.to_string(),
        port,
        socket: socket.map(String::from),
        ipv6,
    };
    assert_eq!(host("localhost"), plain("localhost", None, None, false));
    assert_eq!(host(""), plain("", None, None, false));
    assert_eq!(host("db:3307"), plain("db", Some(3307), None, false));
    assert_eq!(host("db:0"), plain("db", None, None, false));
    assert_eq!(host("0"), plain("", None, None, false));
    assert_eq!(host("db:"), plain("db", None, None, false));
    assert_eq!(
        host("db:99999999999999999999"),
        plain("db", Some(i64::MAX.unsigned_abs()), None, false)
    );
    assert_eq!(
        host("localhost:3307:/tmp/my.sock"),
        plain("localhost", Some(3307), Some("/tmp/my.sock"), false)
    );
    assert_eq!(host("[::1]:3306"), plain("::1", Some(3306), None, true));
    assert_eq!(host("fe80::1"), plain("fe80::1", None, None, true));
    assert_eq!(parse_db_host("[zz::1]"), None);
}

fn lex(source: &[u8]) -> Vec<(lex::Tok, usize, usize)> {
    lex::Lexer::new(source)
        .tokens()
        .unwrap()
        .into_iter()
        .map(|t| (t.tok, t.start, t.end))
        .collect()
}

#[test]
fn token_spans() {
    use lex::Tok::*;
    assert_eq!(
        lex(b"ab<?php $ $x 1.5 ?>c<?php ;"),
        [
            (OpenTag, 2, 7),
            (Punct(b'$'), 8, 9),
            (Var(b"x".to_vec()), 10, 12),
            (Other, 13, 16),
            (CloseTag, 17, 19),
            (OpenTag, 20, 25),
            (Punct(b';'), 26, 27),
        ]
    );
    assert_eq!(
        lex(b"<?php <<<\"EOT\"\nx\n  EOT; <<<'N'\nN\n,"),
        [
            (OpenTag, 0, 5),
            (Other, 6, 22),
            (Punct(b';'), 22, 23),
            (Other, 24, 32),
            (Punct(b','), 33, 34),
        ]
    );
    assert_eq!(
        lex(b"<?php 'a\\'b' \"c\\x41\\q\" x1 ?>"),
        [
            (OpenTag, 0, 5),
            (Str(Some(b"a'b".to_vec())), 6, 12),
            (Str(Some(b"cA\\q".to_vec())), 13, 22),
            (Ident(b"x1".to_vec()), 23, 25),
            (CloseTag, 26, 28),
        ]
    );
    assert_eq!(
        lex(b"<?php // c\n1 # d ?>/* */ <?php /* x */;"),
        [
            (OpenTag, 0, 5),
            (Int(1), 11, 12),
            (CloseTag, 17, 19),
            (OpenTag, 25, 30),
            (Punct(b';'), 38, 39),
        ]
    );
}

#[test]
fn a_dot_needs_digits_on_both_sides_to_make_a_float() {
    use lex::Tok::*;
    assert_eq!(
        lex(b"<?php 1 2 3."),
        [
            (OpenTag, 0, 5),
            (Int(1), 6, 7),
            (Int(2), 8, 9),
            (Int(3), 10, 11),
            (Punct(b'.'), 11, 12),
        ]
    );
}

#[test]
fn only_define_defines() {
    let config = php("foo('NOT_A_CONSTANT', 1); defined('ALSO_NOT', 2);");
    assert!(config.constants.is_empty());
}

#[test]
fn a_value_may_run_to_the_end_of_the_file() {
    assert_eq!(php("define('X', 1").string("X"), None);
    assert_eq!(php("define('X', 1").get("X").unwrap().value, Value::Int(1));
}
