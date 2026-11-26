# Coverage exclusions

Every piece of shipped code that is excluded from the 100% coverage gate ([ADR 0007](docs/adr/0007-full-test-coverage.md)) is listed here with its reason. An exclusion needs `#[cfg_attr(coverage_nightly, coverage(off))]` on the item and review.

| File | Item | Reason | Reviewed |
| --- | --- | --- | --- |

None so far.

## Not measured

- `crates/*/tests/`: integration test files. Their only unexecuted code is failure reporting, which runs when a test fails
- `fuzz/`: fuzz targets, run by the fuzzer rather than the test suite
- Code compiled only for macOS: the coverage gate runs on Linux, so it never sees it. The only such item is the constant `SYSTEM` for `target_os = "macos"` in `crates/hyscribe-admin/src/site_health.rs` (the build target's name on Site Health), which holds no code to run; the macOS job of CI compiles it for both architectures. New macOS-only code needs a reason here and a test on the macOS runner

## Mutants excluded

Mutants that no input tells apart from the code, each searched for one that does, and excluded in `.cargo/mutants.toml`. `#[mutants::skip]` would need the `mutants` crate, which cargo vet has not audited.

| File | Item | Reason |
| --- | --- | --- |
| `crates/hyscribe-render/src/text_diff.rs` | `chunks` | The number of chunks `_diag()` splits into changes its speed, not the subsequence it finds: 800,000 random pairs gave no other table |
| `crates/hyscribe-render/src/text_diff.rs` | `corresponding_end` | When `$j` meets `$other_len`, both ends leave the run where the forward slide put it |
| `crates/hyscribe-image/src/getid3/mp3.rs` | `within_a_percent` | `<` against `<=`: a Xing or VBRI header's bitrate is a byte count over a frame count by a sample rate over 576 or 1152, none of which make it exactly one percent from a frame's bitrate |
| `crates/hyscribe-image/src/pixels.rs` | `window` | The triangle filter weighs the samples at the window's edges 0: moving either edge outwards by the half pixel adds weights of 0, and inwards by it drops them, so no image resizes otherwise |
| `crates/hyscribe-render/src/text_diff.rs` | `up` | Only its first row's count is ever reached: 800,000 random pairs gave no other table |
| `crates/hyscribe-render/src/site.rs` | `Site::viewer` | The default, a site without users, is a visitor, which is what `Viewer::default()` gives: the mutant returns the same value |
