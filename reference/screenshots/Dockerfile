# SPDX-License-Identifier: AGPL-3.0-or-later
#
# The screenshot browser: Playwright's image, pinned by digest, with the
# Python package of the same version (the image carries the browsers only).
# golden.sh builds it once as hyscribe-shots:1.49.1.
FROM mcr.microsoft.com/playwright/python:v1.49.1-noble@sha256:42e251ec3238672c931be6c3565ff279690ae072c3e22058128d0412494f6866
RUN pip install --no-cache-dir --break-system-packages playwright==1.49.1
