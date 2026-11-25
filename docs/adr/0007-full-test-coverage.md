# 0007: Full test coverage

- Status: Accepted
- Date: 2026-10-26

## Context

HyScribe promises byte-level compatibility with WordPress data and puts security first ([ADR 0006](0006-security-first.md)). Much of its code will be written or ported by agents. Untested code is where both compatibility drift and vulnerabilities hide, and agent-written code needs an objective bar it cannot talk its way past.

## Decision

- The entire application is held to full test coverage: 100% line and region coverage for all Rust code, 100% for the admin UI, branch coverage enforced as soon as the stable toolchain supports it
- Mutation testing (`cargo-mutants`) runs on every pull request; surviving mutants in changed code fail the build
- Coverage can never decrease; exclusions need an attribute, a reason and review, and are listed in `coverage-exclusions.md`
- Extension ports are measured natively against the SDK test host and also tested as real components
- Details in [TESTING.md](../TESTING.md)

## Consequences

- The gate is in place from the first line of code in M1, so there is never a backlog of untested code to pay down
- Slower pull requests and more CI time, accepted as the price of the compatibility and security promises
- Design pressure toward small, injectable, deterministic units, which also helps security review
- Coverage alone does not prove quality; mutation testing and differential tests carry that weight
