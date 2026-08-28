# Contributing to accreta

Thanks for taking a look at the project. This document covers the basics of getting set up and
what's expected of a change before it's reviewed.

## Getting set up

```sh
git clone <repo-url>
cd accreta
cargo build
cargo test
```

Run the examples to sanity-check the full ingest -> rollup -> query path:

```sh
cargo run --example basic_usage
cargo run --example custom_aggregate
```

## Before opening a PR

- **`cargo test`** passes, including doctests (`cargo test --doc`).
- **`cargo fmt`** has been run.
- **`cargo clippy --all-targets -- -D warnings`** is clean.
- **`cargo doc --no-deps`** builds without warnings. Every public item (struct, enum, trait,
  function, method, and enum variant) should have a doc comment — see [Documentation
  conventions](#documentation-conventions) below.

## Documentation conventions

This crate leans heavily on rustdoc as the primary source of truth for how the API behaves, so
doc comments are treated as part of the change, not an afterthought:

- Every `pub` item needs a `///` doc comment. For structs and enums, document the type itself
  and, where it isn't obvious from the name, its fields or variants.
- If a doc comment describes behavior — panics, error conditions, complexity, allocation
  behavior — that description has to match what the code actually does. A doc comment that
  drifted out of sync with its implementation is treated as a bug, not a style nit.
- Code examples inside doc comments (` ```rust ` blocks) should compile as written. Prefer a
  runnable example over `ignore`/`no_run` unless the example genuinely can't be self-contained
  (e.g. it needs a value only available at runtime).
- New aggregates, whether built-in or added via examples, should explain *why* their `Monoid`
  merge is correct (e.g. citing the algorithm, as `examples/custom_aggregate.rs` does for
  Welford's algorithm), not just restate what the code does line by line.

## Adding a new built-in aggregate

New aggregates should never require changes to `engine`, `bucket`, or `aggregate_set` — if
you find yourself needing to touch one of those to add an aggregate, that's a sign something's
wrong with the design, not that the abstraction needs to bend. Implement `Monoid` and
`Aggregator` for your state type and register it on a `Schema` alongside the built-ins, the same
way `examples/custom_aggregate.rs` does. Consider adding your aggregate there (or a similar
example) if its merge behavior isn't obvious from the implementation alone.

## Commit messages

Keep the first line under ~72 characters and written in the imperative mood ("Add retention
pruning for weekly buckets", not "Added" or "Adds"). Explain *why* in the body when the change
isn't self-evident from the diff.
