# Glossary

| Term | Meaning |
| --- | --- |
| Core tables | The tables WordPress creates on install, named with `$table_prefix` |
| Differential test | Same input sent to PHP WordPress and HyScribe, outputs compared |
| Extension | A HyScribe plugin or theme, a Rust crate compiled to a WASM component, installed in `hyscribe/plugins/<slug>` or `hyscribe/themes/<slug>` |
| Hook | A WordPress action or filter; the main extension point in both systems |
| Hook catalogue | Generated list of every core hook with its arguments and HyScribe status |
| Port | A HyScribe extension that replaces a specific WordPress plugin or theme |
| Port spec | Agent-written, human-editable description of the behaviour a port must reproduce |
| Primary mode | HyScribe serves the site and writes to the database |
| Reference environment | Containerized PHP WordPress and database used as the test oracle |
| Registry | Data mapping WordPress slugs to ports and compatibility levels |
| Round trip | Reading a stored value and writing it back produces identical bytes |
| Service | The binary running detached in the background, managed with `hyscribe service ...` |
| Setup mode | First start without `.env`: only the wizard is served, behind a one-time secret |
| Shadow mode | HyScribe serves read-only beside PHP WordPress on the same database |
| SDK | `hyscribe-sdk`, the API extensions are written against |
| WIT | WebAssembly Interface Types, used to define the host and extension interface |
