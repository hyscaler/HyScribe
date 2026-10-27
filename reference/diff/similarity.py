# SPDX-License-Identifier: AGPL-3.0-or-later
#
# Similarity of two HTML documents after normalization, for compare.sh:
#   python3 -I similarity.py <php.html> <ours.html> [--diff]
# Prints a percentage (100 = identical after normalization). Normalization
# puts one tag or text run per line, collapses whitespace, and blanks values
# that differ per request: nonces, asset versions and timestamps.
import difflib
import re
import sys


def normalize(html: str) -> list[str]:
    html = re.sub(r"\s+", " ", html)
    html = re.sub(r"(nonce|_wpnonce)([\"'=:\s]+)[0-9a-f]{10}", r"\1\2NONCE", html)
    html = re.sub(r"([?&]ver=)[^\"'&\s]+", r"\1VER", html)
    parts = re.split(r"(<[^>]+>)", html)
    return [part.strip() for part in parts if part.strip()]


def main() -> int:
    with open(sys.argv[1], encoding="utf-8", errors="replace") as php:
        a = normalize(php.read())
    with open(sys.argv[2], encoding="utf-8", errors="replace") as ours:
        b = normalize(ours.read())
    if not a and not b:
        print("100")
        return 0
    ratio = difflib.SequenceMatcher(None, a, b, autojunk=False).ratio()
    print(f"{ratio * 100:.0f}")
    if "--diff" in sys.argv:
        sys.stdout.writelines(f"{line}\n" for line in difflib.unified_diff(a, b, "php", "ours", lineterm=""))
    return 0


if __name__ == "__main__":
    sys.exit(main())
