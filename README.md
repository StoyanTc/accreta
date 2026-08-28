# accreta

A Rust library for aggregating time series data, built around `Monoid` and
`Aggregator` traits. This repository is a Cargo workspace containing the
core library and its language bindings.

## Workspace layout

```
accreta/
├── accreta/           # core library — the source of truth
├── accreta-ffi/        # C ABI, used as the basis for the other bindings
├── accreta-node/        # Node.js bindings (napi-rs)
├── accreta-py/           # Python bindings (PyO3 / maturin)
└── accreta-go/            # Go bindings (planned)
```

Each subdirectory is its own published package with its own README, changelog,
and version — see the table below for links and install instructions.

## Packages

| Package | Language | Package manager | Version |
|---|---|---|---|
| [`accreta`](./accreta) | Rust | [crates.io](https://crates.io/crates/accreta) | see crate |
| [`accreta-ffi`](./accreta-ffi) | C ABI | — | see crate |
| [`accreta-node`](./accreta-node) | Node.js | npm | see package |
| [`accreta-py`](./accreta-py) | Python | PyPI | see package |
| [`accreta-go`](./accreta-go) | Go | — | planned |

## Development

This is a Cargo workspace. Standard commands work from the repo root:

```bash
cargo build --workspace
cargo test --workspace
```

Each binding may have its own build tooling on top of Cargo (`maturin` for
Python, `napi build` for Node) — see that package's own README for details.

## History

This repository consolidates what were previously four separate repositories
(`accreta-rs`, `accreta-ffi`, `accreta-node`, `accreta-py`) into a single
workspace, preserving full commit history via `git subtree`. The old
repositories are archived; see their README for a pointer here.

## Contributing

Contributions are welcome. A few notes specific to this workspace:

- This is a Cargo workspace with multiple published packages (core lib plus
  language bindings) — changes to `accreta/` (the core) usually require
  updating the affected bindings too, so please run the full workspace test
  suite (`cargo test --workspace`) before opening a PR, not just the crate
  you touched.
- Bug fixes and small improvements: open a PR directly.
- Larger changes (new public API, breaking changes, a new language binding):
  please open an issue first to discuss the approach before investing time
  in an implementation.
- Each binding (`accreta-ffi`, `accreta-node`, `accreta-py`) has its own
  build tooling on top of Cargo — see that package's README for how to build
  and test it locally.

## License

Licensed under either of

- Apache License, Version 2.0 (([LICENSE-APACHE](LICENSE-APACHE)))
- MIT license ([LICENSE-MIT](LICENSE-MIT))

at your option.
