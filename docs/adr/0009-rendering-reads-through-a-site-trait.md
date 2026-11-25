# 0009: Rendering reads site data through a trait

- Status: Accepted
- Date: 2026-10-28

## Context

Dynamic blocks read the database while they render: the query loop runs its own query from its attributes, post blocks read terms, authors and featured images, comment blocks read comments, navigation reads menus and pages. Which reads happen depends on the template, the patterns and the attributes, so they cannot all be known before rendering starts.

`hyscribe-render` is synchronous, pure and has no database dependency, which keeps it fast to test, fuzzable and free of `sqlx` and `tokio`. `hyscribe-db` is asynchronous.

## Decision

- `hyscribe-render` defines a `Site` trait with synchronous methods for what blocks read (options, posts by ID, a block query, terms, users, comments, menus), returning plain data types from `hyscribe-model`, which `hyscribe-db` also reads rows into. Methods are added as blocks need them, never ahead
- Blocks receive the trait object and the block context (`postId`, `postType`, `queryId`, `query`), as `WP_Block` passes `$block->context`
- `hyscribe-http` implements `Site` over `hyscribe-db`: the page renders inside `tokio::task::spawn_blocking`, and each method waits on its query with the runtime handle. Reads within one request are memoized, as WordPress's object cache does
- SQL stays in `hyscribe-db` as fixed statements with bound values (ADR 0006); the trait carries values, never SQL
- Render's tests implement `Site` over the default install fixture dump (`hyscribe-testkit`), so block goldens run without a database. The real queries are tested in `hyscribe-db` against the fixture database

## Consequences

- Rendering a page holds one blocking thread for its duration; database waits do not block the async workers
- A block's data needs are visible in one trait, which is also the surface a future extension runtime (ADR 0002) would see
- The fixture-backed implementation answers only what the default install needs; queries it cannot answer faithfully (ordering ties, paging over many posts) are covered by `hyscribe-db` tests instead
