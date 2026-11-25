# Landscape

Related projects and what HyScribe takes from each. Checked 2026-10-27; star counts and activity change, so re-check before relying on them.

## Summary

No other project serves an existing WordPress database and `wp-content` without PHP. Every other project either keeps PHP, asks the site to migrate away from WordPress, or talks to WordPress over its API. Drop-in compatibility is what sets HyScribe apart, and only the fixture-driven tests in [TESTING.md](TESTING.md) keep it true.

## Projects

| Project | What it is | Relation to HyScribe |
| --- | --- | --- |
| [Automattic/wordpress-rs](https://github.com/Automattic/wordpress-rs) | Rust client for the WordPress REST API, with Kotlin and Swift bindings for the mobile apps. MPL-2.0, active | Complementary: it is a client, HyScribe is the server. Its typed client and integration suite can test HyScribe's REST API |
| [EmDash](https://github.com/emdash-cms/emdash) (Cloudflare) | TypeScript CMS on Astro, presented as a successor to WordPress. MIT, developer preview since April 2026 | Related: it also addresses plugin risk, with plugins isolated and capabilities declared in a manifest. Sites migrate to it, where HyScribe keeps the existing database |
| [WordPress Playground](https://github.com/WordPress/wordpress-playground) | PHP compiled to WebAssembly, running WordPress in the browser or Node.js | Keeps PHP. Useful as a light reference runtime for tests |
| [FrankenPHP](https://github.com/php/frankenphp) | Modern PHP application server | Keeps PHP. Same single-binary ease of deployment, without removing PHP's attack surface |
| [Rust-CMS/radical](https://github.com/Rust-CMS/radical) | Headless CMS described as a "Rusty WordPress Replacement". MIT, inactive since August 2024 | Not WordPress compatible |
| [virtuallabdev/rustpress](https://github.com/virtuallabdev/rustpress), [avored-rust-cms](https://github.com/avored/avored-rust-cms) and similar | Rust CMS prototypes and headless CMSs | Cannot read a WordPress site |
| [rnaga/wp-node](https://github.com/rnaga/wp-node) | TypeScript library and CLI that read and write a WordPress database without PHP. MIT, experimental | Closest at the data layer (our `hyscribe-db`), with no rendering, admin or plugins |
| [Kickstarter: WordPress rewrite in C/Rust](https://www.kickstarter.com/projects/programmerhat/fully-wordpress-compatible-rewrite-in-c-rust-for-20x-speedup) | Claims a drop-in rewrite compatible with themes, plugins and core updates | Not verified: the page could not be fetched and no public code was found |
| `php_serde`, `serde_php`, `phpserz` | Rust crates for PHP's serialize format | No byte-exact round trip or PHP golden tests, so `hyscribe-phpser` exists |

## What HyScribe takes from this

- **Compatibility is the product.** Projects that called themselves WordPress replacements became separate CMSs and stalled. Every layer is tested against PHP WordPress on the default install fixture first
- **No migration.** EmDash makes the same plugins-are-the-problem argument. HyScribe's approach keeps the same database, URLs, logins and admin (ADR 0008)
- **Plugin capabilities.** EmDash gives a plugin no binding at all for what its manifest does not declare. ADR 0002 and [EXTENSIONS.md](EXTENSIONS.md) follow the same principle
- **REST conformance from wordpress-rs.** Its integration tests run against a WordPress container. From M5, run them against HyScribe on the same fixture. MPL-2.0 is file-level copyleft and can be combined with AGPL code, but its files keep their MPL license
