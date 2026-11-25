# Risks

| ID | Risk | Impact | Mitigation |
| --- | --- | --- | --- |
| R1 | WordPress core surface is very large (hooks, template tags, edge cases) | Schedule | Hook catalogue generated from core source; prioritize by what fixture sites and pilot plugins actually use |
| R2 | Plugins depend on hooks or globals HyScribe does not reproduce | Ports fail | Analyzer reports missing hooks early; core work queue driven by port failures |
| R3 | Agent-generated ports look right but behave subtly differently | Data corruption, silent bugs | Ports only accepted through differential tests; DB side-effect diffs, not just HTML |
| R4 | License obligations for ports and core code derived from WordPress source | Legal | AGPL-3.0-or-later chosen (ADR 0004); refuse GPLv2-only sources; keep provenance of every port; no premium ports without vendor consent |
| R12 | AGPL network clause deters some hosting providers | Adoption | Built-in source offer link; clear docs that unmodified use carries no extra duty; extension licensing terms |
| R5 | Writes that PHP WordPress reads differently (serialization, dates, counters) | Breaks reversibility | Write-compatibility suite; shadow mode as default first run; refuse writes on unknown `db_version` |
| R6 | WordPress keeps evolving (new hash formats, block APIs, schema bumps) | Drift | Pin supported range; nightly tests against WordPress trunk; registry tracks plugin versions |
| R7 | WASM call overhead on hot filters such as `the_content` | Performance | Batch hook calls, native fast path for core, benchmark early in M6 |
| R8 | Block editor and admin are large front-end projects | Schedule | Reuse GPL `@wordpress/*` editor packages against a compatible REST API (ADR 0008) |
| R9 | Real `wp-config.php` files contain PHP logic (env lookups, includes) | Wizard fails | Best-effort static parse, flag dynamic values, manual entry fallback |
| R10 | Users expect PHP plugins to "just work" | Adoption | Clear compatibility report; the bundled plugins and themes ported first; no PHP proxy |
| R11 | Security bugs in a new codebase handling auth and uploads | Security (highest priority) | Security-first process from M1 ([SECURITY_MODEL.md](SECURITY_MODEL.md)): fuzzing in CI, per-PR security review, threat model per milestone, external audit before 1.0, sandboxed extensions |
| R14 | Agent-generated ports introduce new vulnerabilities | Security | Blocking security review per port, minimal capabilities, known-CVE regression tests, attack payloads in differential tests |
| R13 | Embedded assets make the binary large | Download size, memory | Compress embedded assets, measure size per release, keep the default theme lean |
