# SPDX-License-Identifier: AGPL-3.0-or-later
#
# Photographs the screens of screens.list on one server, in the pinned
# Playwright image (golden.sh runs it):
#   capture.py <side: php|hyscribe> <base url> <screens.list> <out folder>
# For each line it writes <out>/<name>.png (the whole page, 1280 pixels wide,
# UTC, en-US, animations off) and <out>/<name>.res, one line per request
# (status, method, path and query, the admin prefix as wp-admin's and no .php). A role logs in once, as
# the browser does, with the fixture's admin password.
import re
import sys
from urllib.parse import urlsplit

from playwright.sync_api import sync_playwright

side, base, listing, out = sys.argv[1:5]
PASSWORD = "reference-only-password"
USERS = {"administrator": "admin", "editor": "editor", "subscriber": "subscriber"}
FIXED_TIME = "2026-11-19T10:00:00Z"


def target(path):
    """The path as this side spells it."""
    if side == "php":
        return path
    path = path.replace("/wp-login.php", "/hs-login", 1)
    return re.sub(r"/wp-admin/([a-z-]+)\.php", r"/hs-admin/\1", path, count=1)


def name_of(role, path):
    return re.sub(r"[^a-z0-9]+", "-", f"{role}-{path}".lower()).strip("-")


def normal(url):
    """A request's path and query, spelled as wp-admin does without .php."""
    parts = urlsplit(url)
    path = parts.path.replace("/hs-admin/", "/wp-admin/").replace("/hs-login", "/wp-login")
    path = path.removesuffix(".php")
    query = re.sub(r"_ajax_nonce=[0-9a-f]+", "_ajax_nonce=N", parts.query)
    query = re.sub(r"&?[0-9]{13}$", "", query)
    return path + ("?" + query if query else "")


def settle(page):
    """Wait until the block editor's spinners are gone, in the page and its canvas frame.

    The editor loads the navigation fallback, menus and template parts after
    the load event; a slow machine used to photograph it half drawn.
    """
    script = "() => [...document.querySelectorAll('.components-spinner')].filter(e => e.getClientRects().length).length"
    stable = 0
    for _ in range(120):
        busy = 0
        for frame in page.frames:
            try:
                busy += frame.evaluate(script)
            except Exception:
                busy += 1
        stable = 0 if busy else stable + 1
        if stable >= 3:
            return
        page.wait_for_timeout(250)


def log_in(browser, role):
    context = browser.new_context(
        viewport={"width": 1280, "height": 800}, locale="en-US", timezone_id="UTC",
        reduced_motion="reduce", color_scheme="light",
    )
    page = context.new_page()
    page.goto(base + target("/wp-login.php"), wait_until="load")
    try:
        page.fill("#user_login", USERS[role], timeout=10000)
        page.fill("#user_pass", PASSWORD)
        page.click("#wp-submit")
        page.wait_for_load_state("load")
    except Exception:
        page.screenshot(path=f"{out}/login-failed-{role}.png")
        raise
    if "login" in urlsplit(page.url).path:
        sys.exit(f"{side}: login as {role} failed at {page.url}")
    page.close()
    return context


def visitor(browser):
    return browser.new_context(
        viewport={"width": 1280, "height": 800}, locale="en-US", timezone_id="UTC",
        reduced_motion="reduce", color_scheme="light",
    )


with sync_playwright() as p:
    browser = p.chromium.launch(args=["--font-render-hinting=none", "--disable-lcd-text"])
    contexts = {}
    for line in open(listing):
        if line.startswith("#") or not line.strip():
            continue
        role, path, mask = line.rstrip("\n").split("\t")
        if role not in contexts:
            contexts[role] = visitor(browser) if role == "visitor" else log_in(browser, role)
        context = contexts[role]
        context.clock.set_fixed_time(FIXED_TIME)
        page = context.new_page()
        requests = []
        page.on("response", lambda r: requests.append(f"{r.status}\t{r.request.method} {normal(r.url)}")
                if urlsplit(r.url).netloc == urlsplit(base).netloc else None)
        name = name_of(role, path)
        page.goto(base + target(path), wait_until="load")
        try:
            page.wait_for_load_state("networkidle", timeout=8000)
        except Exception:
            pass
        page.wait_for_timeout(500)
        if "site-editor.php" in path:
            settle(page)
        if "post-new.php" in path:
            # The editor draws the starter patterns' previews one idle frame
            # at a time, so a picture taken early shows a different stage.
            page.wait_for_timeout(6000)
        masks = [] if mask == "-" else [page.locator(s) for s in mask.split(",")]
        page.screenshot(path=f"{out}/{name}.png", full_page=True, animations="disabled",
                        caret="hide", mask=masks, mask_color="#ff00ff")
        with open(f"{out}/{name}.res", "w") as handle:
            handle.write(f"final\t{normal(page.url)}\n")
            for r in sorted(set(requests)):
                handle.write(r + "\n")
        page.close()
        print(f"{side}: {name}", flush=True)
    browser.close()
