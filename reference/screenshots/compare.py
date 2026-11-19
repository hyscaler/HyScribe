# SPDX-License-Identifier: AGPL-3.0-or-later
#
# Compares the screenshots of the two servers, pixel by pixel, in the pinned
# Playwright image (golden.sh runs it):
#   compare.py <php folder> <hyscribe folder> <report folder> [expected.list]
# The pictures are decoded and compared by the browser's canvas, so the
# image needs no library. Tolerance: a pixel differs when one of its colour
# channels is more than CHANNEL (16 of 255) apart, which absorbs the
# anti-aliasing of text on a shifted subpixel; a screen matches when the
# pictures have the same size and at most SHARE (0.01 percent) of their
# pixels differ. The requests of the two sides (the .res files) are compared
# too: a request HyScribe answers with another status, or does not make,
# differs. A screen named in expected.list (screen<TAB>reason) differs on
# purpose: it is reported as expected, and as stale when it matches after
# all. Writes <report>/report.tsv, report.html and a diff-<name>.png for
# each differing screen (red where the pixels differ, over the dimmed PHP
# picture, a bar at the bottom where the heights differ). Exits 1 when a
# screen differs and is not expected, or is expected and matches.
import base64
import html
import os
import sys

from playwright.sync_api import sync_playwright

php, hyscribe, report = sys.argv[1:4]
expected = dict(
    line.rstrip("\n").split("\t", 1)
    for line in (open(sys.argv[4]) if len(sys.argv) > 4 else [])
    if line.strip() and not line.startswith("#")
)
CHANNEL = 16
SHARE = 0.0001

PAGE = """
async ([a, b, channel]) => {
  const load = (data) => new Promise((ok, no) => {
    const img = new Image();
    img.onload = () => ok(img);
    img.onerror = no;
    img.src = 'data:image/png;base64,' + data;
  });
  const [x, y] = [await load(a), await load(b)];
  const w = Math.max(x.width, y.width), h = Math.max(x.height, y.height);
  const draw = (img) => {
    const c = document.createElement('canvas');
    c.width = w; c.height = h;
    const g = c.getContext('2d');
    g.fillStyle = '#00ff00'; g.fillRect(0, 0, w, h);
    g.drawImage(img, 0, 0);
    return g.getImageData(0, 0, w, h);
  };
  const [p, q] = [draw(x), draw(y)];
  const out = document.createElement('canvas');
  out.width = w; out.height = h;
  const g = out.getContext('2d');
  const diff = g.createImageData(w, h);
  let count = 0;
  for (let i = 0; i < p.data.length; i += 4) {
    const bad = Math.abs(p.data[i] - q.data[i]) > channel || Math.abs(p.data[i + 1] - q.data[i + 1]) > channel
      || Math.abs(p.data[i + 2] - q.data[i + 2]) > channel;
    if (bad) count++;
    diff.data[i] = bad ? 255 : 255 - (255 - p.data[i]) / 4;
    diff.data[i + 1] = bad ? 0 : 255 - (255 - p.data[i + 1]) / 4;
    diff.data[i + 2] = bad ? 0 : 255 - (255 - p.data[i + 2]) / 4;
    diff.data[i + 3] = 255;
  }
  g.putImageData(diff, 0, 0);
  return { size: [x.width, x.height, y.width, y.height], count, area: w * h,
           png: out.toDataURL('image/png').split(',')[1] };
}
"""


def read(path):
    return [line.rstrip("\n") for line in open(path)] if os.path.exists(path) else []


rows = []
with sync_playwright() as p:
    browser = p.chromium.launch()
    page = browser.new_page()
    for file in sorted(os.listdir(php)):
        if not file.endswith(".png"):
            continue
        name = file[:-4]
        if not os.path.exists(f"{hyscribe}/{file}"):
            rows.append((name, "differs", "no HyScribe picture", "", ""))
            continue
        a = base64.b64encode(open(f"{php}/{file}", "rb").read()).decode()
        b = base64.b64encode(open(f"{hyscribe}/{file}", "rb").read()).decode()
        r = page.evaluate(PAGE, [a, b, CHANNEL])
        pw, ph, hw, hh = r["size"]
        res_php, res_hs = set(read(f"{php}/{name}.res")), set(read(f"{hyscribe}/{name}.res"))
        requests = sorted(res_php ^ res_hs)
        same = (pw, ph) == (hw, hh) and r["count"] <= SHARE * r["area"] and not requests
        note = f"{pw}x{ph} against {hw}x{hh}, {r['count']} pixels differ"
        if expected.get(name) is not None:
            note += f"; expected: {expected[name]}"
        if not same:
            with open(f"{report}/diff-{name}.png", "wb") as handle:
                handle.write(base64.b64decode(r["png"]))
        if same:
            status = "stale" if name in expected else "matches"
        else:
            status = "expected" if name in expected else "differs"
        rows.append((name, status, note, "diff-" + name + ".png" if not same else "", "; ".join(requests)))
    browser.close()

with open(f"{report}/report.tsv", "w") as handle:
    handle.write("screen\tpixels\tdetail\tdiff\trequests that differ (PHP and HyScribe)\n")
    for row in rows:
        handle.write("\t".join(row) + "\n")
with open(f"{report}/report.html", "w") as handle:
    handle.write("<!doctype html><meta charset=utf-8><title>hs-admin against wp-admin</title>"
                 "<style>body{font:14px sans-serif}td{padding:2px 8px;vertical-align:top}.differs,.stale{color:#b00}.expected{color:#960}</style>"
                 "<table><tr><th>Screen<th>Pixels<th>Detail<th>Requests that differ")
    for name, status, note, diff, requests in rows:
        link = f'<a href="{diff}">{html.escape(note)}</a>' if diff else html.escape(note)
        handle.write(f'<tr class="{status}"><td>{html.escape(name)}<td>{status}<td>{link}<td>{html.escape(requests)}')
failing = [row for row in rows if row[1] in ("differs", "stale")]
for row in rows:
    print(f"{row[1]:8} {row[0]}  {row[2]}  {row[4]}")
count = lambda status: len([r for r in rows if r[1] == status])
print(f"{count('matches')} of {len(rows)} screens match, {count('expected')} differ as expected, "
      f"{count('differs')} differ, {count('stale')} expected but match")
sys.exit(1 if failing else 0)
