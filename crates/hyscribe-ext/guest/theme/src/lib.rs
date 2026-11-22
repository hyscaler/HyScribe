// SPDX-License-Identifier: Apache-2.0 OR MIT

//! The test theme (ADR 0023): drives every call of the SDK's `theme`
//! world for the host's tests, each round answering what the host said as
//! text, and renders templates that print, call back into the host,
//! misbehave or take their time. Built on `hyscribe-sdk` with its `theme`
//! feature, without `std`.

#![no_std]

extern crate alloc;

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;
use core::fmt::Debug;
use core::hint::black_box;

use hyscribe_sdk::customize::{self, CustomizeKind};
use hyscribe_sdk::theme::{self, TemplateFile};
use hyscribe_sdk::{Plugin, SdkError, Theme, Value, hooks, template};

hyscribe_sdk::callbacks! {
    enum Cb {
        Calls => calls,
        Head => head,
    }
}

struct TestTheme;

type Answer = Result<Value, String>;

fn rounds(steps: &[&dyn Debug]) -> String {
    let all: Vec<String> = steps.iter().map(|step| format!("{step:?}")).collect();
    all.join(" ")
}

/// Every call of `template`, `theme-setup` and `customize` in a request.
fn calls(_: Vec<Value>) -> Answer {
    let one = [Value::Int(1)];
    let crop = Value::Bool(true);
    let bad = Value::Json("{".into());
    let looped = (
        template::have_posts(),
        template::the_post(),
        template::in_the_loop(),
        template::rewind_posts(),
        template::wp_reset_postdata(),
    );
    Ok(Value::Text(rounds(&[
        &looped,
        &template::is("is_home", &one),
        &template::is("is_page", &[]),
        &template::is("fail", &[]),
        &template::is("is_home", &[bad.clone()]),
        &template::print("the_title", &one),
        &template::print("bytes", &[]),
        &template::print("fail", &[]),
        &template::print("the_title", &[bad.clone()]),
        &template::get("get_the_ID", &one),
        &template::get("bytes", &[]),
        &template::get("fail", &[]),
        &template::get("get_the_ID", &[bad.clone()]),
        &template::get_header(None, &[]),
        &template::get_footer(Some("x"), &one),
        &template::get_sidebar(Some("bytes"), &[]),
        &template::get_template_part("template-parts/content", Some("page"), &[]),
        &template::get_template_part("none", None, &[bad.clone()]),
        &theme::add_support("title-tag", &[]),
        &theme::add_support("html5", &[bad.clone()]),
        &theme::remove_support("title-tag"),
        &theme::remove_support("fail"),
        &theme::register_nav_menus(&[("menu-1", "Header"), ("menu-2", "Footer")]),
        &theme::register_sidebar("{\"id\":\"sidebar-1\"}"),
        &theme::register_sidebar("{"),
        &theme::add_image_size("wide", 1200, 0, &crop),
        &theme::add_image_size("wide", 1200, 0, &bad),
        &theme::set_post_thumbnail_size(800, 600, &crop),
        &theme::set_post_thumbnail_size(800, 600, &bad),
        &theme::add_editor_style(&["editor.css"]),
        &theme::add_editor_style(&["../x.css"]),
        &theme::set_content_width(800),
        &theme::register_block_style("core/button", "{\"name\":\"outline\"}"),
        &theme::register_block_style("core/button", "{"),
        &theme::register_block_pattern("t/hero", "{\"title\":\"Hero\"}"),
        &theme::register_block_pattern("t/hero", "{"),
        &theme::get_mod("color", None),
        &theme::set_mod("color", &Value::Text("red".into())),
        &theme::get_mod("color", Some(&Value::Text("blue".into()))),
        &theme::get_mod("width", Some(&Value::Int(3))),
        &theme::get_mod("bytes", None),
        &theme::get_mod("color", Some(&bad)),
        &theme::set_mod("color", &bad),
        &theme::remove_mod("color"),
        &theme::remove_mod("color"),
        &customize::add(CustomizeKind::Section, "links", "{}", &[("active_callback", 1)]),
        &customize::add(CustomizeKind::Setting, "links", "{", &[]),
        &customize::remove(CustomizeKind::Control, "x"),
        &customize::remove(CustomizeKind::Panel, "y"),
        &theme::templates(&[]),
    ])))
}

/// `ext_theme_head`, which the host fires while the theme renders: run in
/// the theme's other instance.
fn head(args: Vec<Value>) -> Answer {
    let head = args.first().and_then(Value::text).unwrap_or_default();
    Ok(Value::Text(format!("{head}<!-- theme hook -->")))
}

/// What the calls answer in `init`, which has no request.
const IN_INIT: &str = r#"false Err(SdkError::Failed("runs in a request, not in init")) Err(SdkError::Failed("runs in a request, not in init")) Err(SdkError::Failed("runs in a request, not in init")) Err(SdkError::Failed("runs in a request, not in init")) None Err(SdkError::Invalid("../x.php is not a PHP file inside the theme")) Err(SdkError::Invalid("x.html is not a PHP file inside the theme"))"#;

fn file(file: &str, name: Option<&str>, post_types: &[&str]) -> TemplateFile {
    TemplateFile {
        file: file.into(),
        name: name.map(String::from),
        post_types: post_types.iter().map(|post_type| String::from(*post_type)).collect(),
    }
}

impl Plugin for TestTheme {
    fn init() -> Result<(), String> {
        let why = |e: SdkError| format!("{e:?}");
        hooks::on_filter("ext_theme", Cb::Calls, 10, 1).map_err(why)?;
        hooks::on_filter("ext_theme_head", Cb::Head, 10, 1).map_err(why)?;
        theme::templates(&[
            file("index.php", None, &[]),
            file("page-wide.php", Some("Wide"), &["page", "post"]),
            file("template-parts/header.php", None, &[]),
            file("front-page.php", None, &[]),
        ])
        .map_err(why)?;
        let answered = rounds(&[
            &template::have_posts(),
            &theme::get_mod("color", None),
            &template::print("the_title", &[]),
            &theme::add_support("title-tag", &[]),
            &theme::register_sidebar("{}"),
            &template::get_header(None, &[]),
            &theme::templates(&[file("../x.php", None, &[])]),
            &theme::templates(&[file("x.html", None, &[])]),
        ]);
        if answered == IN_INIT {
            Ok(())
        } else {
            Err(answered)
        }
    }

    fn activate() -> Result<(), String> {
        Ok(())
    }

    fn deactivate() -> Result<(), String> {
        Ok(())
    }

    fn invoke(callback: u32, args: Vec<Value>) -> Result<Value, String> {
        Cb::invoke(callback, args)
    }
}

impl Theme for TestTheme {
    fn render(file: &str) -> Result<Vec<u8>, String> {
        match file {
            // The head comes from the host, whose hook reaches this theme's
            // callback in its other instance.
            "index.php" => {
                let head = template::print("wp_head", &[]).map_err(|e| format!("{e:?}"))?;
                let mut page = head;
                page.extend_from_slice(b"<main>index</main>");
                Ok(page)
            }
            // The tags the host answers over the page being rendered.
            "front-page.php" => {
                let why = |e: SdkError| format!("{e:?}");
                let front = template::is("is_front_page", &[]).map_err(why)?;
                let mut page = b"<body ".to_vec();
                page.extend(template::print("body_class", &[]).map_err(why)?);
                page.extend_from_slice(if front { b">front</body>" } else { b">other</body>" });
                Ok(page)
            }
            "slow.php" => template::print("slow", &[]).map_err(|e| format!("{e:?}")),
            "trap.php" => core::arch::wasm32::unreachable(),
            "spin.php" => {
                let mut n = 0u64;
                loop {
                    n = black_box(n.wrapping_add(1));
                }
            }
            _ => Err(format!("no template {file}")),
        }
    }
}

hyscribe_sdk::theme!(TestTheme);
