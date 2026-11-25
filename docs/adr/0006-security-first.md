# 0006: Security is the first priority

- Status: Accepted
- Date: 2026-10-26

## Context

HyScribe exists largely because of security: PHP runtime flaws such as PHP2Shell, the vulnerability volume of unsandboxed plugins, and AI agents making discovery and exploitation fast and cheap ([VISION.md](../VISION.md)). Competing goals (compatibility, performance, features) will regularly pull against security.

## Decision

- Security is the top priority. Conflicts are settled in this order: security, data compatibility, behaviour compatibility, performance, convenience
- Data compatibility is never traded away; WordPress behaviours that are known attack vectors are off by default with an explicit opt-in
- Ports never reproduce known vulnerabilities of the plugin they replace
- Security work runs from M1 onward, not in a final hardening phase; every milestone exit includes a threat model update and security review
- Controls and release blockers are defined in [SECURITY_MODEL.md](../SECURITY_MODEL.md); the reporting policy is in `SECURITY.md`

## Consequences

- Some WordPress behaviours differ by default (for example, XML-RPC off), which the compatibility report must show clearly
- Extra CI cost from fuzzing, dependency vetting, dynamic scanning and reviews
- Slower feature delivery in exchange for a defensible security story, which is the product's main reason to exist

## Amendment (2026-11-25): author archives behind `AUTHOR_ARCHIVES`

- Decision (2026-11-25): author archives are the first of the planned opt-in keys to be built. `AUTHOR_ARCHIVES` is off by default, read as every yes or no key is, and an invalid value stops the start
- Off, every author-shaped request (`?author=`, `?author_name=`, `/author/<nicename>/` with its pages and feeds) answers the same 404 as before the key existed
- On, author archives answer as PHP WordPress 7.1.3 does: the main query by author, `handle_404()`'s rule for an author with no posts, the `?author=N` redirect of `redirect_canonical()` for an author with a published post, the template hierarchy, the title, body classes, feed links and feeds, checked against PHP by the page goldens. It exposes what PHP exposes and no more
- The users sitemap stays off, a separate question
