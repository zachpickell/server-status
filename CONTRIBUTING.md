# Contributing

Small personal project, kept small on purpose. PRs welcome, but the bar is "does this
need to exist" before "is this clean code" — see the Roadmap in the README for what's
actually planned.

## Before opening a PR

```bash
cargo test
cargo clippy --all-targets
```

Both must pass clean. No CI runs these yet, so it's on you.

## Guidelines

- Keep the agent dumb: it samples and serves JSON, nothing else. Logic belongs in `dash`.
- No new dependencies for something the standard library or an existing dependency
  already covers.
- Match the existing style (doc comments on the *why*, not the *what*; small modules).
- Add a test for new logic in `common`/`dash`'s format/config/poller code. UI rendering
  (`ui.rs`) and system sampling (`collect.rs`) are exempt — they're thin wrappers around
  `ratatui`/`sysinfo` and hard to unit test meaningfully.

## Bugs / ideas

Open an issue. If it's a security issue (token handling, the agent's Docker access),
say so in the title and keep details minimal until it's addressed.
