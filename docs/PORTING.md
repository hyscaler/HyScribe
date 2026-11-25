# Agent-driven porting

WordPress has tens of thousands of plugins. Hand-porting does not scale, so porting is a pipeline run by AI agents, gated by tests rather than by trust.

## Pipeline

```
 PHP plugin/theme source
        |
   1. Analyze  -----> inventory.json (hooks, shortcodes, blocks, routes, options, tables, cron, assets)
        |
   2. Specify  -----> port-spec.md (behaviour to reproduce, scope, known gaps, SDK mapping)
        |
   3. Fixture  -----> fixture site with the plugin active + recorded HTTP/DB traces from PHP WordPress
        |
   4. Generate -----> Rust crate against hyscribe-sdk
        |
   5. Verify   -----> differential tests: same requests, compare normalized HTML, JSON, DB side effects; known-CVE regression tests and attack payloads
        |
   6. Iterate  -----> agent fixes failures until green or reports blocking gaps
        |
   7. Review   -----> human review, license check, registry entry with compatibility level
```

## Stage details

### 1. Analyze

A deterministic static analyzer (not an LLM) parses the PHP with a real parser and extracts:

- every `add_action` / `add_filter` call with hook name, priority and callback
- `add_shortcode`, `register_block_type`, `register_rest_route`, `register_post_type`, `register_taxonomy`
- options and meta keys read and written
- custom tables created in activation hooks (`dbDelta` statements)
- `wp_schedule_event` calls, admin pages, enqueued assets
- external HTTP calls and third-party services
- hooks used that HyScribe core does not yet fire (these become core work items)

### 2. Specify

An agent turns the inventory plus readme and source into a port spec: what the plugin does for site visitors, what it does in admin, what data it stores, and what is out of scope. A human can edit the spec before generation.

### 3. Fixture

Spin up PHP WordPress with the plugin in the reference environment, apply scripted scenarios (create a form, submit it, view a shortcode page), and record requests, responses and database diffs. These recordings are the ground truth.

### 4. Generate

An agent writes the Rust crate using only the SDK. Data formats must match the original exactly (same option names, same serialized shapes, same custom tables), because the database must remain usable by the PHP plugin after switching back.

### 5 and 6. Verify and iterate

The differential harness ([TESTING.md](TESTING.md)) replays the recorded scenarios against HyScribe with the port. Failures go back to the agent with the diff. The loop has a budget; if it is exhausted the port is filed as partial with the failing scenarios listed.

### 7. Review

- Full test coverage of the port, measured natively against the SDK test host, with no surviving mutants
- Security review is mandatory and blocking (see [SECURITY_MODEL.md](SECURITY_MODEL.md#porting-and-security)): minimal capabilities, input handling, no string-built SQL, no `unsafe`, regression tests proving the original plugin's published vulnerabilities are not reproduced
- License check (ADR 0004): "GPLv2 or later" plugins are ported as AGPL-3.0-or-later with original copyright notices kept; "GPLv2 only" and non-GPL plugins are refused unless the copyright holder relicenses; premium plugins need explicit permission from the vendor. The analyzer reads the license header in stage 1 so refusals happen before any generation work
- Registry entry: slug, supported plugin versions, compatibility level, known gaps

## Compatibility levels

| Level | Meaning |
| --- | --- |
| full | All recorded scenarios pass, 100% coverage |
| partial | Visitor-facing scenarios pass; listed admin or edge features missing |
| experimental | Generated, partially verified, not recommended for production |

## Themes

Same pipeline with a theme-specific analyzer: template files, template hierarchy coverage, `functions.php` hooks, customizer settings, menus, widget areas, `theme.json`. Differential tests compare rendered pages across the Theme Unit Test content.

## Agent tooling

- Built on the Claude Agent SDK, with the analyzer, fixture recorder and differential harness exposed as tools
- Runs headless in CI so ports can be regenerated when a plugin releases a new version
- Each run stores its inventory, spec, diffs and transcript for audit

## Starting set

The first ports are chosen for coverage of SDK features, not popularity alone. A proposed pilot:

- a shortcode-heavy plugin (forms)
- an SEO plugin (head output, sitemaps, metadata)
- a custom post type plugin
- a block library plugin
- an anti-spam plugin (external HTTP, comment hooks)
