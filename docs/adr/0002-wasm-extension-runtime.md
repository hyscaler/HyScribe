# 0002: WebAssembly components as the extension runtime

- Status: Accepted (2026-11-14)
- Date: 2026-10-26

## Context

Plugins and themes must be Rust packages, installable without rebuilding the core binary, and must not be able to crash or compromise the server. Rust has no stable ABI, which rules out safe native dynamic loading. See the comparison in [EXTENSIONS.md](../EXTENSIONS.md).

## Decision

- Extensions are Rust crates compiled to `wasm32-wasip2` WebAssembly components
- The host embeds wasmtime and defines the host and guest interfaces in WIT
- `hyscribe-sdk` wraps the generated bindings in a WordPress-shaped Rust API
- Each extension declares capabilities in `hyscribe.ext.toml`; the host enforces them
- Core features may be implemented natively in the host behind the same hook API

## Consequences

- Sandboxing and hot loading come for free; extensions are portable across OS and CPU
- Per-call overhead on hot hooks needs measurement and batching (risk R7)
- The WIT interface is a public API and must be versioned carefully
- Extension authors need the `wasm32-wasip2` target installed
