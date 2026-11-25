# Vision

## In one sentence

Keep everything that makes WordPress valuable (its data model, its content, its URLs, its ecosystem of ideas) and replace the part that has become its biggest liability: the PHP runtime and the unsandboxed plugin model built on it.

## Why HyScribe exists

WordPress powers a large share of the web, and its core team maintains it well. The problems are underneath and around it: the PHP runtime every site depends on, and a plugin model where any extension can do anything. In 2026 three trends made those problems impossible to ignore.

### 1. The runtime itself is now the attack surface

**PHP2Shell (October 2026).** Two flaws in PHP itself, CVE-2026-109618 ("RCE via OOM corruption") and CVE-2026-109619 ("RCE via stale global state"), allow unauthenticated remote code execution. PHP rates both CVSS v4.0 9.2. CISA states they affect "all PHP installations serving web requests, including Apache with mod_php ... PHP-FPM, php-cgi, FrankenPHP and LiteSpeed", and advised that unpatched PHP 8.1 or earlier be taken offline. Every WordPress site was exposed without a single plugin being at fault: WordPress announced that no WordPress update was required or planned, because there was nothing WordPress could fix.

Both bugs are memory-lifecycle failures inside the interpreter: state left half built after an out-of-memory condition, and state from one request surviving into the next. These are exactly the defect classes that a memory-safe language and a design without long-lived global request state rule out.

**PHPAfterShell (November 2026).** Four weeks later a follow-on flaw, CVE-2026-126714 (CVSS v4.0 9.2), was disclosed on 19 November 2026 in PHP's default file-based session handler, reached through session upload progress; both were enabled by default in releases before the fix. The researchers at depthfirst who found it reported it privately in late October. The October PHP2Shell releases do not contain the fix, so a server patched for PHP2Shell stayed exposed until it was patched again (8.5.13, 8.4.28, 8.3.37, 8.2.36 or 8.6.0); PHP 8.1 and older are reported affected with no fix planned. At disclosure neither PHP nor the researchers reported attacks using it. One emergency upgrade of the runtime was followed by a second within a month, again for a flaw in the runtime's own request handling that no application could fix. HyScribe has no PHP runtime, no PHP session files and no upload progress handler: its sessions are WordPress's own login tokens, kept in the database.

**The upgrade treadmill leaves sites behind.** Fixes shipped only for PHP 8.2 to 8.5. PHP 8.1 reached end of life on 31 December 2025 and 8.2 follows on 31 December 2026. At the time of writing, WordPress.org's own statistics show about a third of reporting WordPress sites on PHP 8.1 or older, branches that receive no security fixes at all. Many owners cannot upgrade PHP without breaking a plugin, so they stay exposed.

**Earlier runtime bugs show the pattern.** CVE-2024-4577, an argument injection in PHP-CGI, was listed in CISA's Known Exploited Vulnerabilities catalog and is known to have been used in ransomware campaigns.

### 2. The plugin ecosystem is where attacks land

- Patchstack counted **11,334 new vulnerabilities in the WordPress ecosystem in 2025, a 42% increase over 2024**. 91% were in plugins and 9% in themes; core had 6, all low priority.
- **46% did not receive a fix from the developer in time for public disclosure.**
- For heavily exploited vulnerabilities, the median time to mass exploitation was **5 hours**, and roughly half of high impact vulnerabilities are exploited within 24 hours.
- A recurring class is PHP object injection through `unserialize()`, for example CVE-2024-5932 in GiveWP: unauthenticated object injection chained to remote code execution, rated critical.

Core is not immune either. In July 2026 CISA added two WordPress core flaws to its Known Exploited Vulnerabilities catalog: CVE-2026-60137 (SQL injection, reachable through `WP_Query`'s `author__not_in` when a plugin or theme passes input to it) and CVE-2026-63030 (an interpretation conflict in REST API batch routing). In September 2026 WordPress 7.1.2 fixed CVE-2026-87902, where an unauthenticated attacker could, under conditions, make template resolution include a local PHP file, which may lead to remote code execution.

The structural problem: a WordPress plugin is arbitrary PHP running with the full privileges of the site. It can read every table, write files, and include other PHP files. One flawed plugin compromises everything.

### 3. Agents have changed the economics of finding and exploiting bugs

- PHP2Shell was co-discovered by OpenAI together with the security firms Calif and Wiz, working with frontier AI models under OpenAI's "Patch the Planet" program in August 2026. Chainguard independently found the same vulnerability the night of disclosure "using an in-house AI harness and a frontier model".
- The coordinated disclosure did not hold. CVE IDs leaked on public forums on 20 October, forcing the PHP project to bring its release forward to 21 October. Amazon's MadPot honeypots recorded a PHP2Shell exploitation attempt dated 17 October, four days before public disclosure. Amazon notes that "AI-assisted analysis makes it cheaper to turn a patch diff or advisory into a working exploit."
- VulnCheck measured the median time from CVE publication to appearance in CISA's exploited catalog falling from 120 days in 2025 to 80 days in the first half of 2026.
- Check Point's AI Security Report 2026 documents intrusions in which AI ran exploitation workflows autonomously across dozens of sessions with minimal human direction.

To be fair to the data, VulnCheck also found that AI-discovered vulnerabilities are not more likely to be exploited than others (14 of 1,061, or 1.3%). The shift is not that AI bugs are worse. It is that discovery and weaponization are now cheap, fast and plentiful, so the window between "a bug exists" and "a bug is exploited" keeps shrinking, sometimes to before the patch exists.

When finding bugs gets cheap, patching faster is not enough. What matters is how large the attack surface is and which classes of bugs the design makes impossible.

### 4. Performance and operations cost more than they should

These are the reasons HyScribe expects to be faster and simpler to run. They are targets to be proven by the benchmarks in milestone M10, not claims yet:

- **Work repeated on every request.** On an uncached request, PHP WordPress bootstraps core, loads every active plugin and the theme, and reads autoloaded options before rendering anything. Speed relies on stacking caches (OPcache, object cache, page cache plugins) on top.
- **Process per worker concurrency.** Each PHP-FPM worker handles one request at a time with its own memory, so concurrency is bought with RAM.
- **A stack, not a program.** A typical install needs a web server, PHP-FPM, the right PHP extensions, `php.ini` tuning, and a matching PHP version for every plugin.
- **Writable code directories.** PHP executes any `.php` file it can reach, and plugin and theme folders are often writable. Attackers use that to persist web shells.
- **Visitor-driven cron.** Scheduled tasks run when someone happens to visit, unless the operator sets up a real cron.

## How HyScribe answers each problem

| Problem | HyScribe design |
| --- | --- |
| Interpreter memory corruption and runtime request handling (PHP2Shell and PHPAfterShell class) | Rust: memory safety enforced by the compiler; no request state shared through interpreter globals |
| Unpatched, end-of-life runtimes | No separate runtime to keep up to date. One binary, one version, one upgrade step |
| Object injection via `unserialize()` | PHP serialized data is parsed as plain data only. Serialized objects are kept as opaque values and never turned into live objects |
| Local file inclusion to code execution | HyScribe never executes PHP or any file from disk. Templates are data, plugins are signed packages |
| SQL injection through loosely typed query arguments | Typed query builder (for example, `author__not_in` is a list of integers) and parameterized queries only |
| Any plugin can do anything | Plugins run in a WebAssembly sandbox and get only the capabilities their manifest declares (tables, options, outbound hosts, files) |
| Web shells in writable folders | Code lives only inside the binary and installed extension packages; uploads are served as static files and never executed |
| Slow, multi-layer stack | One process with async I/O, embedded assets, built-in scheduler, no PHP-FPM pool |
| Agents speeding up attacks | Remove whole bug classes by design, and turn the same agents to defense: porting with differential tests, fuzzing every parser, agent-assisted security review |

## What we keep

- **The data.** Every table, row and serialized value stays WordPress-compatible. Switching back to PHP WordPress is always possible (see [COMPATIBILITY.md](COMPATIBILITY.md)).
- **The URLs.** Every permalink, feed and media link keeps working.
- **The users.** Existing logins and passwords keep working.
- **The ecosystem's ideas.** Hooks, shortcodes, blocks and themes keep their shape, so plugins can be ported, mostly by agents, instead of rewritten from scratch.
- **The ease of install.** Download, extract, run, finish in the browser (see [DEPLOYMENT.md](DEPLOYMENT.md)).

## What we are honest about

- Rust removes memory safety bugs, not logic bugs. Authorization mistakes and injection flaws in HyScribe code remain possible. Fuzzing, review and an external audit before 1.0 are part of the plan.
- The plugin ecosystem has to be rebuilt. That is why porting by agents, verified against the original plugin, is a primary goal and not an afterthought.
- Performance claims are targets until the M10 benchmarks are published.

## Sources

Fetched and checked on 26 October 2026.

- CISA, [Critical Vulnerabilities in PHP Require Immediate Patching](https://www.cisa.gov/news-events/alerts/2026/10/21/critical-vulnerabilities-php-require-immediate-patching), 21 October 2026
- PHP, [2026 news archive: security release for CVE-2026-109618 and CVE-2026-109619](https://www.php.net/archive/2026.php), 21 October 2026
- PHP, [Supported versions](https://www.php.net/supported-versions.php) and [end of life branches](https://www.php.net/eol.php)
- WordPress, [Critical PHP Security Release](https://wordpress.org/news/2026/10/critical-php-security-release/), 21 October 2026
- WordPress.org, [PHP version statistics API](https://api.wordpress.org/stats/php/1.0/), read 26 October 2026
- TechTarget, [PHP2Shell disclosure unravels after leaks forced early patch drop](https://www.techtarget.com/cybersecurity/news/366652613/PHP2Shell-disclosure-unravels-after-leaks-forced-early-patch-drop)
- Amazon, [Amazon Threat Intelligence identifies PHP2Shell exploitation prior to public disclosure in MadPot](https://aws.amazon.com/blogs/security/amazon-threat-intelligence-identifies-php2shell-exploitation-prior-to-public-disclosure-in-madpot/)
- Wikipedia, [PHP2Shell](https://en.wikipedia.org/wiki/PHP2Shell)
- depthfirst, [phpAfterShell](https://depthfirst.com/phpaftershell), Cycode, [PHPAfterShell (CVE-2026-126714): critical flaw in PHP's default session handler](https://cycode.com/blog/phpaftershell-cve-2026-126714/) and Astra Security, [CVE-2026-126714](https://www.getastra.com/blog/security-advisory/cve-2026-126714/), read 24 November 2026
- Patchstack, [State of WordPress Security in 2026](https://www.patchstack.com/whitepaper/state-of-wordpress-security-in-2026/)
- CISA, [CISA Adds Four Known Exploited Vulnerabilities to Catalog](https://www.cisa.gov/news-events/alerts/2026/07/21/cisa-adds-four-known-exploited-vulnerabilities-catalog), 21 July 2026
- WordPress, [WordPress 7.1.2 Release](https://wordpress.org/news/2026/09/wordpress-7-1-2-release/), 22 September 2026
- NVD, [CVE-2024-5932](https://nvd.nist.gov/vuln/detail/CVE-2024-5932) (GiveWP object injection)
- Tenable, [CVE-2024-4577 PHP-CGI argument injection](https://tenable.com/plugins/nessus/200464)
- VulnCheck, [State of Exploitation 1H 2026](https://www.vulncheck.com/blog/state-of-exploitation-1h-2026)
- Help Net Security, [Check Point AI Security Report 2026](https://www.helpnetsecurity.com/2026/07/15/check-point-ai-security-report-2026/)
