## Summary

What does this change and why?

## Related

Closes #

## Checklist

- [ ] Tests are in the same commit as the change; a bug fix starts with a failing test
- [ ] Behaviour claims about WordPress are backed by a differential or golden test recorded from PHP WordPress
- [ ] `cargo fmt --all --check` and `cargo clippy --workspace --all-targets -- -D warnings` pass
- [ ] `cargo test --workspace` passes and coverage stays at 100% lines and regions
- [ ] No WordPress table schema changed, and nothing written that PHP WordPress would not write
- [ ] No secrets committed; docs, `PROGRESS.md` and `HISTORY.md` updated where the change calls for it
- [ ] I have the right to submit this under the licence of the files it changes
