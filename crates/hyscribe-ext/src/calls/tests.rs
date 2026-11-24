// SPDX-License-Identifier: AGPL-3.0-or-later

use hyscribe_phpser::Key;

use super::*;

#[test]
fn names_within_wordpress_limits() {
    assert_eq!(named("x", "Post type", 20), Ok(()));
    assert_eq!(named(&"é".repeat(20), "Post type", 20), Ok(()));
    let long =
        SdkError::Invalid("Post type names must be between 1 and 20 characters in length.".into());
    assert_eq!(named(&"x".repeat(21), "Post type", 20), Err(long));
    let empty =
        SdkError::Invalid("Taxonomy names must be between 1 and 32 characters in length.".into());
    assert_eq!(named("", "Taxonomy", 32), Err(empty));
}

#[test]
fn records_read_from_vals_with_their_shape_checked() {
    assert!(fields(None).is_empty());
    assert!(fields(Some(&Val::Bool(true))).is_empty());
    assert_eq!(text(None), "");
    assert_eq!(texts(Some(&Val::U32(1))), Vec::<String>::new());
    assert_eq!(number(None), 0);
    assert_eq!(id(None), 0);
    let empty = asset(&[]);
    assert_eq!(
        (empty.kind, empty.version, empty.handle),
        (AssetKind::Script, None, String::new())
    );
    let route = route_in(&[]);
    assert_eq!(
        (route.path, route.methods, route.callback),
        (String::new(), Vec::new(), 0)
    );
    assert_eq!(message(&[]).to, Vec::<String>::new());
}

#[test]
fn the_post_types_a_query_reads() {
    let args = |post_type: Option<Value>| {
        let entries = post_type.map(|value| (Key::String(b"post_type".to_vec()), value));
        Value::Array(entries.into_iter().collect())
    };
    let text = |name: &str| Value::String(name.as_bytes().to_vec());
    assert_eq!(post_types(&args(None)), Ok(vec!["post".to_string()]));
    assert_eq!(
        post_types(&args(Some(text("page")))),
        Ok(vec!["page".to_string()])
    );
    let list = Value::Array(vec![(Key::Int(0), text("a")), (Key::Int(1), text("b"))]);
    assert_eq!(
        post_types(&args(Some(list))),
        Ok(vec!["a".to_string(), "b".to_string()])
    );
    let invalid = Err(SdkError::Invalid(
        "post_type names the post types read".into(),
    ));
    assert_eq!(post_types(&args(Some(text("any")))), invalid);
    assert_eq!(post_types(&args(Some(Value::Int(1)))), invalid);
    assert_eq!(
        post_types(&Value::Null),
        Err(SdkError::Invalid("query variables are an object".into()))
    );
}

/// `admin.page`'s position read from its record.
#[test]
fn a_page_s_position_is_read() {
    let some = |value: Val| Val::Option(Some(Box::new(value)));
    let record = Val::Record(
        [
            ("parent", some(Val::String("tools.php".into()))),
            ("page-title", Val::String("P".into())),
            ("menu-title", Val::String("M".into())),
            ("capability", Val::String("read".into())),
            ("slug", Val::String("p".into())),
            ("callback", Val::U32(3)),
            ("icon", Val::String(String::new())),
            ("position", some(Val::S32(5))),
        ]
        .map(|(name, value)| (name.to_string(), value))
        .to_vec(),
    );
    let page = admin_page(&[record]);
    assert_eq!(
        (page.parent.as_deref(), page.position),
        (Some("tools.php"), Some(5))
    );
    assert_eq!(admin_page(&[]).position, None);
}

/// A field added again takes its own first place, not another's.
#[test]
fn a_field_added_again_keeps_its_place() {
    let engine = wasmtime::Engine::default();
    let (grants, slug) = (Arc::new(crate::Grants::default()), Arc::from("ext"));
    let mut store = State::store(
        &engine,
        (&grants, &slug),
        Phase::Init,
        1 << 20,
        std::time::Duration::from_millis(50),
    );
    let state = store.data_mut();
    let add = |state: &mut State, id: &str, title: &str| {
        let args = (
            id.into(),
            title.into(),
            1,
            "p".into(),
            "s".into(),
            "{}".into(),
        );
        add_field(state, args)
    };
    for (id, title) in [("a", "A"), ("b", "B"), ("a", "A2")] {
        assert_eq!(add(state, id, title), Ok(()));
    }
    let titles: Vec<_> = state
        .registered
        .fields
        .iter()
        .map(|f| f.title.as_str())
        .collect();
    assert_eq!(titles, ["A2", "B"]);
}
