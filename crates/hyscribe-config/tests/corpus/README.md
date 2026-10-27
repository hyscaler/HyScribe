# Test corpus

Input files for the `wp-config.php` reader. They are data, never executed by HyScribe; `reference/wpconfig/golden.sh` runs them in PHP to record the expected results in `../golden/`.

| File | Source | License |
| --- | --- | --- |
| `wp-config/sample.php` | `wp-config-sample.php` from WordPress 7.1.3 | GPL-2.0-or-later |
| `wp-config/environment.php` | Written for HyScribe, in the style of container images that read the environment | AGPL-3.0-or-later |
| `wp-config/wpcli.php` | `wp config create --skip-salts` output from WP-CLI with WordPress 7.1.3 | GPL-2.0-or-later |
| `wp-config/edge-cases.php` | Written for HyScribe | AGPL-3.0-or-later |
