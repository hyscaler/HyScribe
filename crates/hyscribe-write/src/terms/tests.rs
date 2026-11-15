// SPDX-License-Identifier: AGPL-3.0-or-later

//! The term functions against the write oracle's goldens
//! (`reference/writes/actions-terms.php`).

use hyscribe_config::Mode;
use hyscribe_db::{Meta, PostFields, Write, Writer};
use hyscribe_testkit::{fixture_database, writes::check_writes};

use super::*;

// One check at a time in the whole crate, so no two wait on each other's rows.
use crate::tests::ONE_AT_A_TIME;

async fn writer() -> Writer {
    Writer::open(&fixture_database(), Mode::Primary)
        .await
        .unwrap()
        .unwrap()
}

/// Checks the golden `golden`: `setup` and `action` each run as one action
/// on the oracle's connection.
async fn check<T>(
    golden: &str,
    setup: impl AsyncFnOnce(&mut Write<'_>) -> T,
    action: impl AsyncFnOnce(&mut Write<'_>, T),
) {
    let _one = ONE_AT_A_TIME.lock().await;
    let writer = writer().await;
    let setup = async |conn: &mut hyscribe_db::Connection| {
        let mut write = writer.begin_on(conn).await.unwrap();
        let state = setup(&mut write).await;
        write.commit().await.unwrap();
        Ok(state)
    };
    let action = async |conn: &mut hyscribe_db::Connection, state| {
        let mut write = writer.begin_on(conn).await.unwrap();
        action(&mut write, state).await;
        write.commit().await.unwrap();
        Ok(())
    };
    let checked = check_writes(&fixture_database(), golden, setup, action).await;
    checked.unwrap();
}

async fn none(_: &mut Write<'_>) {}

async fn tag(write: &mut Write<'_>, name: &[u8]) -> TermIds {
    let args = TermArgs::default();
    wp_insert_term(write, Taxonomy::PostTag, name, &args)
        .await
        .unwrap()
}

async fn category(write: &mut Write<'_>, name: &[u8], parent: u64) -> TermIds {
    let args = TermArgs {
        parent: Some(parent as i64),
        ..TermArgs::default()
    };
    wp_insert_term(write, Taxonomy::Category, name, &args)
        .await
        .unwrap()
}

async fn insert(
    write: &mut Write<'_>,
    taxonomy: Taxonomy,
    name: &[u8],
    args: TermArgs<'_>,
) -> Result<TermIds, TermError> {
    wp_insert_term(write, taxonomy, name, &args).await
}

async fn update(
    write: &mut Write<'_>,
    taxonomy: Taxonomy,
    id: u64,
    args: TermArgs<'_>,
) -> Result<TermIds, TermError> {
    wp_update_term(write, taxonomy, id, &args).await
}

fn slug(slug: &[u8]) -> TermArgs<'_> {
    TermArgs {
        slug: Some(slug),
        ..TermArgs::default()
    }
}

fn parent(parent: u64) -> TermArgs<'static> {
    TermArgs {
        parent: Some(parent as i64),
        ..TermArgs::default()
    }
}

fn alias(alias_of: &[u8]) -> TermArgs<'_> {
    TermArgs {
        alias_of: Some(alias_of),
        ..TermArgs::default()
    }
}

#[tokio::test]
async fn inserts_a_category() {
    let action = async |write: &mut Write<'_>, ()| {
        let args = TermArgs {
            description: Some(b"What is new."),
            ..TermArgs::default()
        };
        insert(write, Taxonomy::Category, b"News", args)
            .await
            .unwrap();
    };
    check("insert-category", none, action).await;
}

#[tokio::test]
async fn inserts_a_tag_on_a_post() {
    let action = async |write: &mut Write<'_>, ()| {
        let term = tag(write, b"Oracle").await;
        let terms = [TermRef::Id(term.term_id)];
        let set = wp_set_post_terms(write, 1, TermList::List(&terms), Taxonomy::PostTag, false);
        assert_eq!(set.await.unwrap(), Some(vec![term.term_taxonomy_id]));
    };
    check("insert-tag-on-post", none, action).await;
}

#[tokio::test]
async fn sanitizes_names_and_descriptions() {
    let action = async |write: &mut Write<'_>, ()| {
        let args = TermArgs {
            description: Some(br"<p>Para</p><script>x()</script> O\'Brien"),
            ..TermArgs::default()
        };
        let name = br"A <b>bold</b> & O\'Brien\\s  tag";
        insert(write, Taxonomy::PostTag, name, args).await.unwrap();
        tag(write, b"!!!").await;
    };
    check("insert-term-markup", none, action).await;
}

#[tokio::test]
async fn inserts_child_categories() {
    let setup = async |write: &mut Write<'_>| category(write, b"Parent", 0).await.term_id;
    let action = async |write: &mut Write<'_>, parent| {
        category(write, b"Child", parent).await;
        category(write, b"Second child", parent).await;
    };
    check("insert-child-category", setup, action).await;
}

#[tokio::test]
async fn refuses_duplicate_tags() {
    let setup = async |write: &mut Write<'_>| {
        let oracle = tag(write, b"Oracle").await.term_id;
        let tags = Taxonomy::PostTag;
        let thing = insert(write, tags, b"Thing", slug(b"custom")).await;
        let kid = insert(write, tags, b"Kid", parent(1)).await;
        (oracle, thing.unwrap().term_id, kid.unwrap())
    };
    let action = async |write: &mut Write<'_>, (oracle, thing, kid): (u64, u64, TermIds)| {
        let tags = Taxonomy::PostTag;
        let no_args = TermArgs::default;
        let exists = insert(write, tags, b"ORACLE", no_args()).await;
        assert!(matches!(exists, Err(TermError::Exists(id)) if id == oracle));
        let other_slug = insert(write, tags, b"thing", no_args()).await;
        assert!(matches!(other_slug, Err(TermError::Exists(id)) if id == thing));
        let kid_2 = insert(write, tags, b"Kid", parent(1)).await.unwrap();
        assert_ne!(kid_2, kid);
        let empty = insert(write, tags, b" \t ", no_args()).await;
        assert!(matches!(empty, Err(TermError::EmptyName)));
        let invalid = insert(write, tags, b"<b></b>", no_args()).await;
        assert!(matches!(invalid, Err(TermError::InvalidName)));
        let orphan = insert(write, tags, b"Missing parent", parent(999)).await;
        assert!(matches!(orphan, Err(TermError::MissingParent)));
        let other = slug(b"oracle-other");
        insert(write, tags, b"Oracle", other).await.unwrap();
        insert(write, tags, b"Another", slug(b"oracle"))
            .await
            .unwrap();
        insert(write, tags, b"Yet another", slug(b"oracle"))
            .await
            .unwrap();
    };
    check("insert-duplicate-tag", setup, action).await;
}

#[tokio::test]
async fn refuses_duplicate_categories_and_suffixes_slugs() {
    let setup = async |write: &mut Write<'_>| {
        let parent = category(write, b"Parent", 0).await.term_id;
        let child = category(write, b"Child", parent).await.term_id;
        let other = category(write, b"Other", 0).await.term_id;
        let stray = tag(write, b"Stray").await.term_id;
        (parent, child, other, stray)
    };
    let action = async |write: &mut Write<'_>, (parent, child, other, stray)| {
        let cats = Taxonomy::Category;
        let under = |slug: &'static [u8], parent: u64| TermArgs {
            slug: Some(slug),
            parent: Some(parent as i64),
            ..TermArgs::default()
        };
        let by_slug = insert(write, cats, b"child", self::parent(parent)).await;
        assert!(matches!(by_slug, Err(TermError::Exists(id)) if id == child));
        let by_name = insert(write, cats, b"Child", self::parent(parent)).await;
        assert!(matches!(by_name, Err(TermError::Exists(id)) if id == child));
        let given = insert(write, cats, b"Child", under(b"child", parent)).await;
        assert!(matches!(given, Err(TermError::Exists(id)) if id == child));
        let kid = insert(write, cats, b"Kid", under(b"kid", parent)).await;
        kid.unwrap();
        let other_slug = insert(write, cats, b"Child", under(b"other", parent)).await;
        other_slug.unwrap();
        category(write, b"Child", 0).await;
        category(write, b"Child", other).await;
        let young = insert(write, cats, b"Young", under(b"child", other)).await;
        young.unwrap();
        let tot = insert(write, cats, b"Tot", slug(b"kid-child")).await;
        tot.unwrap();
        let grandchild = insert(write, cats, b"Grandchild", under(b"kid", child)).await;
        grandchild.unwrap();
        let stray = insert(write, cats, b"Stray", under(b"kid", stray)).await;
        stray.unwrap();
        let baby = insert(write, cats, b"Baby", under(b"child", child)).await;
        baby.unwrap();
        let tyke = insert(write, cats, b"Tyke", under(b"small", parent)).await;
        tyke.unwrap();
        category(write, b"tyke", parent).await;
    };
    check("insert-duplicate-category", setup, action).await;
}

#[tokio::test]
async fn groups_aliases() {
    let setup = async |write: &mut Write<'_>| {
        tag(write, b"Oracle").await;
    };
    let action = async |write: &mut Write<'_>, ()| {
        let tags = Taxonomy::PostTag;
        insert(write, tags, b"Synonym", alias(b"oracle"))
            .await
            .unwrap();
        insert(write, tags, b"Third", alias(b"oracle"))
            .await
            .unwrap();
        insert(write, tags, b"Lonely", alias(b"missing"))
            .await
            .unwrap();
    };
    check("insert-alias-tag", setup, action).await;
}

#[tokio::test]
async fn returns_an_older_term_with_the_slug() {
    let setup = async |write: &mut Write<'_>| tag(write, b"0").await;
    let action = async |write: &mut Write<'_>, zero| {
        let term = tag(write, b"0!").await;
        assert_eq!(term, zero);
        let terms = [TermRef::Id(term.term_id)];
        let list = TermList::List(&terms);
        wp_set_post_terms(write, 1, list, Taxonomy::PostTag, false)
            .await
            .unwrap();
    };
    check("insert-zero-slug-tag", setup, action).await;
}

#[tokio::test]
async fn updates_a_term() {
    let setup = async |write: &mut Write<'_>| category(write, b"News", 0).await.term_id;
    let action = async |write: &mut Write<'_>, id| {
        let cats = Taxonomy::Category;
        let name = |name: &'static [u8]| TermArgs {
            name: Some(name),
            ..TermArgs::default()
        };
        let missing = update(write, cats, 999, name(b"Nothing")).await;
        assert!(matches!(missing, Err(TermError::InvalidTerm)));
        let blank = update(write, cats, id, name(b" ")).await;
        assert!(matches!(blank, Err(TermError::EmptyName)));
        let orphan = update(write, cats, id, parent(999)).await;
        assert!(matches!(orphan, Err(TermError::MissingParent)));
        let args = TermArgs {
            name: Some(b"Renamed & <i>new</i>"),
            description: Some(b"<em>What</em> is new."),
            ..TermArgs::default()
        };
        let ids = update(write, cats, id, args).await.unwrap();
        assert_eq!(ids.term_id, id);
    };
    check("update-term", setup, action).await;
}

#[tokio::test]
async fn updates_slugs() {
    let setup = async |write: &mut Write<'_>| {
        tag(write, b"One").await;
        let two = tag(write, b"Two").await.term_id;
        (two, tag(write, b"Three").await.term_id)
    };
    let action = async |write: &mut Write<'_>, (two, three)| {
        let tags = Taxonomy::PostTag;
        let taken = update(write, tags, two, slug(b"one")).await;
        assert!(matches!(taken, Err(TermError::DuplicateSlug(s)) if s == b"one"));
        let renamed = |name: &'static [u8]| TermArgs {
            name: Some(name),
            slug: Some(b""),
            ..TermArgs::default()
        };
        update(write, tags, two, renamed(b"One")).await.unwrap();
        update(write, tags, three, renamed(b"!!!")).await.unwrap();
    };
    check("update-term-slug", setup, action).await;
}

#[tokio::test]
async fn updates_an_alias() {
    let setup = async |write: &mut Write<'_>| {
        tag(write, b"Oracle").await;
        tag(write, b"Other").await.term_id
    };
    let action = async |write: &mut Write<'_>, other| {
        update(write, Taxonomy::PostTag, other, alias(b"oracle"))
            .await
            .unwrap();
    };
    check("update-term-alias", setup, action).await;
}

#[tokio::test]
async fn moves_categories_without_loops() {
    let setup = async |write: &mut Write<'_>| {
        let alpha = category(write, b"Alpha", 0).await.term_id;
        let beta = category(write, b"Beta", alpha).await.term_id;
        (alpha, beta, category(write, b"Gamma", 0).await.term_id)
    };
    let action = async |write: &mut Write<'_>, (alpha, beta, gamma)| {
        let cats = Taxonomy::Category;
        update(write, cats, alpha, parent(beta)).await.unwrap();
        update(write, cats, gamma, parent(gamma)).await.unwrap();
        update(write, cats, gamma, parent(beta)).await.unwrap();
        let taken = update(write, cats, beta, slug(b"gamma")).await;
        assert!(matches!(taken, Err(TermError::DuplicateSlug(_))));
    };
    check("update-category-parent", setup, action).await;
}

#[tokio::test]
async fn breaks_a_loop_above_the_new_parent() {
    let setup = async |write: &mut Write<'_>| {
        let c = category(write, b"Loop C", 0).await;
        let d = category(write, b"Loop D", c.term_id).await;
        let row = (c.term_id, "category");
        let looped = write.update_term_taxonomy(c.term_taxonomy_id, row, b"", d.term_id);
        looped.await.unwrap();
        (c.term_id, category(write, b"Loop E", 0).await.term_id)
    };
    let action = async |write: &mut Write<'_>, (c, e)| {
        update(write, Taxonomy::Category, e, parent(c))
            .await
            .unwrap();
    };
    check("break-category-loop", setup, action).await;
}

#[tokio::test]
async fn deletes_a_tag() {
    let setup = async |write: &mut Write<'_>| {
        let id = tag(write, b"Oracle").await.term_id;
        let terms = [TermRef::Id(id)];
        let list = TermList::List(&terms);
        wp_set_post_terms(write, 1, list, Taxonomy::PostTag, false)
            .await
            .unwrap();
        wp_set_object_terms(write, 2, &terms, Taxonomy::PostTag, false)
            .await
            .unwrap();
        write
            .add_meta(Meta::Term, id, "color", Some(b"red"))
            .await
            .unwrap();
        id
    };
    let action = async |write: &mut Write<'_>, id| {
        let tags = Taxonomy::PostTag;
        let missing = wp_delete_term(write, tags, 999).await.unwrap();
        assert_eq!(missing, Deletion::Missing);
        let deleted = wp_delete_term(write, tags, id).await.unwrap();
        assert_eq!(deleted, Deletion::Deleted);
    };
    check("delete-tag", setup, action).await;
}

#[tokio::test]
async fn deletes_a_shared_term_from_one_taxonomy() {
    let setup = async |write: &mut Write<'_>| {
        let id = tag(write, b"Shared").await.term_id;
        let shared = write.insert_term_taxonomy((id, "category"), b"", 0);
        shared.await.unwrap();
        id
    };
    let action = async |write: &mut Write<'_>, id| {
        let deleted = wp_delete_term(write, Taxonomy::PostTag, id).await;
        assert_eq!(deleted.unwrap(), Deletion::Deleted);
    };
    check("delete-shared-term", setup, action).await;
}

#[tokio::test]
async fn deletes_a_category_for_the_default() {
    let setup = async |write: &mut Write<'_>| {
        let news = category(write, b"News", 0).await.term_id;
        category(write, b"Local", news).await;
        let other = category(write, b"Other", 0).await.term_id;
        wp_set_post_categories(write, 1, &[TermRef::Id(news)], false)
            .await
            .unwrap();
        let terms = [TermRef::Id(news), TermRef::Id(other)];
        wp_set_object_terms(write, 2, &terms, Taxonomy::Category, false)
            .await
            .unwrap();
        news
    };
    let action = async |write: &mut Write<'_>, news| {
        let cats = Taxonomy::Category;
        let default = wp_delete_term(write, cats, 1).await.unwrap();
        assert_eq!(default, Deletion::Default);
        let deleted = wp_delete_term(write, cats, news).await.unwrap();
        assert_eq!(deleted, Deletion::Deleted);
    };
    check("delete-category", setup, action).await;
}

#[tokio::test]
async fn deletes_a_category_without_a_default() {
    let setup = async |write: &mut Write<'_>| {
        let news = category(write, b"News", 0).await.term_id;
        wp_set_post_categories(write, 1, &[TermRef::Id(news)], false)
            .await
            .unwrap();
        let option = write.update_option("default_category", b"999");
        option.await.unwrap();
        news
    };
    let action = async |write: &mut Write<'_>, news| {
        let deleted = wp_delete_term(write, Taxonomy::Category, news).await;
        assert_eq!(deleted.unwrap(), Deletion::Deleted);
    };
    check("delete-category-no-default", setup, action).await;
}

#[tokio::test]
async fn sets_post_categories() {
    let setup = async |write: &mut Write<'_>| {
        let news = category(write, b"News", 0).await.term_id;
        wp_set_post_categories(write, 1, &[TermRef::Id(news)], false)
            .await
            .unwrap();
        let date = "2026-01-01 00:00:00".to_string();
        let draft = PostFields {
            author: 1,
            date: date.clone(),
            date_gmt: date.clone(),
            content: Vec::new(),
            content_filtered: Vec::new(),
            title: b"Auto Draft".to_vec(),
            excerpt: Vec::new(),
            status: "auto-draft".into(),
            post_type: "post".into(),
            comment_status: "open".into(),
            ping_status: "open".into(),
            password: Vec::new(),
            name: Vec::new(),
            to_ping: Vec::new(),
            pinged: Vec::new(),
            modified: date.clone(),
            modified_gmt: date,
            parent: 0,
            menu_order: 0,
            mime_type: String::new(),
            guid: Vec::new(),
        };
        (news.to_string(), write.insert_post(&draft).await.unwrap())
    };
    let action = async |write: &mut Write<'_>, (news, draft): (String, u64)| {
        let none = wp_set_post_categories(write, draft, &[], false).await;
        assert_eq!(none.unwrap(), Some(Vec::new()));
        let page = wp_set_post_categories(write, 2, &[], false).await;
        assert_eq!(page.unwrap(), Some(Vec::new()));
        let default = wp_set_post_categories(write, 1, &[], true).await;
        assert_eq!(default.unwrap(), Some(vec![1]));
        let news_id = TermRef::Id(news.parse().unwrap());
        let terms = [
            TermRef::Text(news.as_bytes()),
            TermRef::Text(b"Missing"),
            news_id,
        ];
        wp_set_post_categories(write, 1, &terms, true)
            .await
            .unwrap();
        let nothing = wp_set_post_categories(write, 1, &[TermRef::Text(b"")], false);
        assert_eq!(nothing.await.unwrap(), Some(Vec::new()));
    };
    check("set-post-categories", setup, action).await;
}

#[tokio::test]
async fn sets_post_tags() {
    let action = async |write: &mut Write<'_>, ()| {
        let tags = Taxonomy::PostTag;
        let text = |text: &'static [u8]| TermList::Text(text);
        let zero = wp_set_post_terms(write, 0, text(b"zero"), tags, false).await;
        assert_eq!(zero.unwrap(), None);
        wp_set_post_terms(write, 1, text(b" alpha, Beta,,"), tags, false)
            .await
            .unwrap();
        let replace = [b"alpha" as &[u8], b"gamma", b"beta "].map(TermRef::Text);
        wp_set_post_terms(write, 1, TermList::List(&replace), tags, false)
            .await
            .unwrap();
        let more = [
            TermRef::Text(b"delta"),
            TermRef::Id(999),
            TermRef::Text(b""),
        ];
        wp_set_post_terms(write, 1, TermList::List(&more), tags, true)
            .await
            .unwrap();
        let empty = wp_set_post_terms(write, 2, text(b"0"), tags, false).await;
        assert_eq!(empty.unwrap(), Some(Vec::new()));
        // No slug: `term_exists()` takes the taxonomy's first term.
        let first = [TermRef::Text(b"???")];
        wp_set_post_terms(write, 2, TermList::List(&first), tags, false)
            .await
            .unwrap();
        wp_set_post_terms(write, 2, text(b"delta,gamma"), tags, true)
            .await
            .unwrap();
    };
    check("set-post-tags", none, action).await;
}

#[tokio::test]
async fn removes_post_tags() {
    let setup = async |write: &mut Write<'_>| {
        let list = TermList::Text(b"alpha, beta, gamma");
        wp_set_post_terms(write, 1, list, Taxonomy::PostTag, false)
            .await
            .unwrap();
    };
    let action = async |write: &mut Write<'_>, ()| {
        let tags = Taxonomy::PostTag;
        let none = [b"missing" as &[u8], b"", br"\ "].map(TermRef::Text);
        assert!(!wp_remove_object_terms(write, 1, &none, tags).await.unwrap());
        assert!(!wp_remove_object_terms(write, 1, &[], tags).await.unwrap());
        let unrelated = [TermRef::Text(b"alpha")];
        assert!(
            !wp_remove_object_terms(write, 2, &unrelated, tags)
                .await
                .unwrap()
        );
        let some = [b"alpha" as &[u8], b"Beta", b"missing"].map(TermRef::Text);
        assert!(wp_remove_object_terms(write, 1, &some, tags).await.unwrap());
    };
    check("remove-post-tags", setup, action).await;
}

#[tokio::test]
async fn refuses_what_is_not_ported() {
    let _one = ONE_AT_A_TIME.lock().await;
    let writer = writer().await;
    let mut write = writer.begin().await.unwrap();
    // A term shared by two taxonomies, from before WordPress 4.2.
    let shared = tag(&mut write, b"Shared").await.term_id;
    write
        .insert_term_taxonomy((shared, "category"), b"", 0)
        .await
        .unwrap();
    let args = TermArgs::default();
    let split = wp_update_term(&mut write, Taxonomy::PostTag, shared, &args).await;
    assert!(matches!(split, Err(TermError::Unported(_))));

    // A term in a classic menu.
    let id = tag(&mut write, b"Linked").await.term_id;
    let item = PostFields {
        author: 1,
        date: "2026-01-01 00:00:00".into(),
        date_gmt: "2026-01-01 00:00:00".into(),
        content: Vec::new(),
        content_filtered: Vec::new(),
        title: Vec::new(),
        excerpt: Vec::new(),
        status: "publish".into(),
        post_type: "nav_menu_item".into(),
        comment_status: "closed".into(),
        ping_status: "closed".into(),
        password: Vec::new(),
        name: b"linked".to_vec(),
        to_ping: Vec::new(),
        pinged: Vec::new(),
        modified: "2026-01-01 00:00:00".into(),
        modified_gmt: "2026-01-01 00:00:00".into(),
        parent: 0,
        menu_order: 1,
        mime_type: String::new(),
        guid: Vec::new(),
    };
    let item = write.insert_post(&item).await.unwrap();
    let id_text = id.to_string();
    for (key, value) in [
        ("_menu_item_object_id", id_text.as_bytes()),
        ("_menu_item_type", b"taxonomy"),
        ("_menu_item_object", b"post_tag"),
    ] {
        write
            .add_meta(Meta::Post, item, key, Some(value))
            .await
            .unwrap();
    }
    let menu = wp_delete_term(&mut write, Taxonomy::PostTag, id).await;
    assert!(matches!(menu, Err(TermError::Unported(_))));
}

#[test]
fn large_options_are_not_autoloaded() {
    assert_eq!(default_autoload(&[b'x'; 150_000]), "auto");
    assert_eq!(default_autoload(&[b'x'; 150_001]), "auto-off");
}

#[tokio::test]
async fn a_new_term_that_fails_fails_the_assignment() {
    let _one = ONE_AT_A_TIME.lock().await;
    let writer = writer().await;
    let mut write = writer.begin().await.unwrap();
    // No tag yet, so no first tag stands in for a text without a slug.
    let terms = [TermRef::Text(b"<b></b>")];
    let set = wp_set_object_terms(&mut write, 1, &terms, Taxonomy::PostTag, false).await;
    assert!(matches!(set, Err(TermError::InvalidName)));
}

#[tokio::test]
async fn a_failed_statement_fails_the_action() {
    use sqlx::Connection as _;
    let db = fixture_database();
    let options = sqlx::mysql::MySqlConnectOptions::new()
        .host(&db.host)
        .port(db.port)
        .database(&db.name)
        .username(&db.user)
        .password(db.password.expose());
    let mut conn = sqlx::MySqlConnection::connect_with(&options).await.unwrap();
    let id: u64 = sqlx::query_scalar("SELECT CONNECTION_ID()")
        .fetch_one(&mut conn)
        .await
        .unwrap();
    let writer = writer().await;
    let mut write = writer.begin_on(&mut conn).await.unwrap();
    let mut killer = sqlx::MySqlConnection::connect_with(&options).await.unwrap();
    let kill = sqlx::query("KILL ?").bind(id);
    kill.execute(&mut killer).await.unwrap();
    let args = TermArgs::default();
    let inserted = wp_insert_term(&mut write, Taxonomy::Category, b"News", &args).await;
    assert!(matches!(inserted, Err(TermError::Db(_))));
}
