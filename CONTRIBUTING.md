# Contributing to HyScribe

Thanks for your interest in improving HyScribe. Bug reports, compatibility reports from real WordPress sites, docs fixes, tests and code are all welcome. This guide takes you from a fresh clone to an open pull request, then covers the rules every change follows.

Security reports never go into a public issue or pull request: see [SECURITY.md](SECURITY.md). Everyone taking part follows the [Code of Conduct](CODE_OF_CONDUCT.md).

## Before you start

- Read [GOALS.md](GOALS.md) for what HyScribe is and is not, and [docs/COMPATIBILITY.md](docs/COMPATIBILITY.md) for the contract every change respects
- Check [PROGRESS.md](PROGRESS.md) and [docs/ROADMAP.md](docs/ROADMAP.md): the feature may be done, under way or planned
- For anything larger than a small fix, open an issue first and say what you plan to do, so the work is not done twice

## Your first pull request

You need Rust stable 1.99 or later, Docker (for the fixture databases and the PHP WordPress oracle) and git.

1. Fork the repository and clone your fork
2. Build the fixtures once and start the fixture databases:

   ```
   reference/theme-test-data/build.sh
   reference/default-install/db.sh up
   ```

3. Run the tests: `cargo test --workspace`
4. Make the change on a branch from `development`, with its tests in the same commit
5. Run the checks under [Before opening a pull request](#before-opening-a-pull-request)
6. Push the branch and open a pull request against `development`

Keep a pull request to one change, describe what changed and why, and reference the issue. Do not commit secrets; `.env` is git-ignored and stays that way.

## Before opening a pull request

```
cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
cargo llvm-cov --workspace --ignore-filename-regex '/tests/' \
  --fail-under-lines 100 --fail-under-regions 100
cargo deny check
```

Mutation testing (`cargo mutants`, limited to your change with `--in-diff`) is run on every change before it is merged; running it yourself saves a round of review. [docs/TESTING.md](docs/TESTING.md) explains each gate, and [CLAUDE.md](CLAUDE.md) lists the commands for every golden script.

## The rules every change follows

- **Security comes first** ([docs/SECURITY_MODEL.md](docs/SECURITY_MODEL.md)). No `unsafe` outside the reviewed allowlist, no SQL built from strings, no execution of files from disk, and every parser of untrusted input has a fuzz target. A port never reproduces a known vulnerability of the code it ports
- **PHP WordPress is the test oracle.** A claim about WordPress's behaviour needs a differential or golden test recorded from PHP WordPress, not a reading of its source alone
- **Full coverage.** Every change ships with its tests: 100% line and region coverage, and no surviving mutants in changed code. A bug fix starts with a failing test
- **The database stays WordPress's.** Never change a WordPress core table's schema, and never write data PHP WordPress would not write, so a site can switch back at any time ([ADR 0003](docs/adr/0003-database-compatibility-contract.md))
- **WordPress's names.** A subsystem exposes WordPress's filters and actions under their WordPress names and argument order ([docs/EXTENSIONS.md](docs/EXTENSIONS.md))
- **Keep the code lean.** No speculative helpers, unused dependencies or leftover placeholders
- **Architecture decisions get an ADR** in `docs/adr/`; say so in the issue before writing one
- **New dependencies** pass `cargo deny check` and `cargo vet`, and need a reason in the pull request

## What is not accepted

- Code copied or translated from a GPL-2.0-only or non-free source ([ADR 0004](docs/adr/0004-licensing.md)). WordPress itself is GPL-2.0-or-later and may be ported
- Third-party plugins or themes in this repository: the release binary carries none. Ports are built and shipped as separate extension packages
- Changes that run PHP, or that load and execute code from disk

## Style

- Rust: `cargo fmt`, the workspace's lints, and the patterns of the surrounding code
- Every source file starts with `// SPDX-License-Identifier: AGPL-3.0-or-later` (the extension SDK crates: `Apache-2.0 OR MIT`)
- Markdown: one line per paragraph or list item, no horizontal rules, and no long dashes or double hyphens as punctuation
- Commit messages: a short subject saying what changed, and a body when the why is not obvious

## Licence of contributions

HyScribe is licensed under AGPL-3.0-or-later ([LICENSE](LICENSE)); the extension SDK crates are `Apache-2.0 OR MIT`. A contribution is offered under the licence of the files it changes, and by opening a pull request you confirm that you wrote it or otherwise have the right to submit it under that licence.

The HyScribe name and logo are not covered by those licences: see [brand/README.md](brand/README.md).

## Reporting security issues

Please do not open public issues for vulnerabilities. See [SECURITY.md](SECURITY.md).
