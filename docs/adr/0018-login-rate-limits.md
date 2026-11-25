# 0018: Login rate limits in memory

- Status: Accepted (2026-11-16)
- Date: 2026-11-16

## Context

PHP WordPress core does not limit login attempts; sites add a plugin for it. Each wrong guess at `wp-login.php` costs a bcrypt check at cost 12 (about a quarter of a second of CPU), so unlimited guessing is both brute force and a cheap way to exhaust the server's CPU. docs/SECURITY_MODEL.md lists login rate limiting as a control HyScribe builds in.

Constraints:

- ADR 0003: HyScribe writes nothing PHP WordPress would not write, so no counter can live in the database (plugins that store them in options or their own tables are not core)
- A limit must not tell an attacker whether an account exists, or whether it is limited, beyond what PHP's own answers tell
- A limit must not become a way to lock a real user out
- Its state must not grow with what an attacker sends

## Decision

Accepted on 2026-11-16 with its defaults (5 failures per address, 10 per account, a 900 second window, refusals capped at a day), keeping the per account limit with trusted addresses, and with two additions: the limits run as a step of the `authenticate` filter chain, and concurrent password checks are capped site wide.

- `/hs-login` counts failed guesses per client address and per account, in the process's memory (`hyscribe-http`'s `login::limits`). `/hs-login` is the only path that checks a user's password
- A refused guess goes through the login screen as any other, but the password check answers "no match" without running the hash, so the page is PHP's wrong password page byte for byte
- Addresses get refusals that double for repeated abuse, up to a day; accounts only get a refusal of one window, lifted for the last 8 addresses the account logged in from since the start, so the account limit slows a spread out attack without becoming a lockout where the owner usually logs in
- One password check at a time per address: a concurrent guess is refused and counted, so a burst cannot pass the check before the first failure is recorded
- At most as many password checks at once, site wide, as the machine has CPUs, whatever their addresses; a guess beyond the cap waits up to 2 seconds in a line of at most 8 per CPU, and past the wait or with the line full is answered as a wrong password without its hash checked, and not counted
- Three `.env` keys set the limits (`LOGIN_LIMIT_ADDRESS`, `LOGIN_LIMIT_ACCOUNT`, `LOGIN_LIMIT_WINDOW`); each map is bounded at 65,536 entries, dropping spent entries and then the least recently active sixteenth when full, refusals last
- The client address is the one comment flood checks use: the connection's, or the last `X-Forwarded-For` hop under `BEHIND_PROXY`; IPv6 addresses count by their /64

### WordPress's hook points

- The limits run as a step of the `authenticate` filter chain, at the position WordPress login limiting plugins expect: ahead of the password check of core's `wp_authenticate_username_password` and `wp_authenticate_email_password` (priority 20), so a refused guess never reaches bcrypt, and with core's own steps (`wp_authenticate_cookie`, `wp_authenticate_spam_check`) at their priorities. Built 2026-11-17: the callback `hyscribe_login_limits` at priority 10, WordPress's default priority, gives a guess (a non-empty password) its address's turn to check passwords; the address and account refusals and the site wide cap are checked in `wp_check_password()`, which has the account, so no callback can skip them. The chain (`hyscribe-admin`'s `login::authenticate`) runs callbacks by priority, then in the order added; an extension adds a callback or replaces one by name
- `authenticate`, `wp_login_failed` and `wp_login` fire under their names with WordPress's arguments; until the extension runtime they are plain internal calls (docs/EXTENSIONS.md), and no hook can skip the limits' check. `wp_login_failed` fires from `wp_authenticate()` for any failure but a blank field, and counts a guess; `wp_login` fires from the `wp_signon()` port for the user found, before HyScribe sets the cookies (PHP fires it after), and trusts the address for the account

## Consequences

- HyScribe answers differently from PHP on purpose: after the limit, the right password is answered as wrong. Differential tests of logins stay under the limits
- A restart forgets every count; several HyScribe processes on one site count separately. Neither weakens the limits much, since the process is long running and a site runs one
- Timing tells a refused guess (no bcrypt) from a checked one; it does not tell whether the account exists any more than PHP's own answers do
- Concurrent checks from many addresses are bounded by the site wide cap, so a botnet cannot use more than the CPUs for bcrypt at once; it can still keep them busy, at the account limit's rate per account and the address limit's rate per address, and the account limit can be passed by as many guesses as arrive at once from different addresses, up to the cap
- Under such a flood real users' logins wait their turn for a check
- A user who submits the login form twice at once gets a wrong password answer for the second
