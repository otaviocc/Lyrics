# Agent instructions

Instructions for any coding agent working in this repository.

## Invariants: do not break these

1. **Never write to audio files.** The only `lofty` API this codebase may call is
   `lofty::read_from_path` (see `src/meta.rs`). Do not add a call to `save_to`,
   `insert_tag`, or any other tag-mutation API, anywhere, under any flag. Every
   filesystem write must go through `sidecar_path()` in `src/sidecar.rs`, which
   structurally cannot return the input audio path. Before finishing a change that
   touches `src/`, run:

   ```sh
   grep -rn "save_to\|insert_tag\|write_to\b" src/
   ```

   It must return nothing.

2. **Never violate a provider's request contract.** Requests to any provider
   (`lrclib.net`, `api.lrcmux.dev`, or a future one) must stay sequential (never
   concurrent) and throttled (`http::Client`'s `throttle()`); a `429` response's
   `Retry-After` header must be honored, not approximated; every request must carry
   an identifying `User-Agent`. Do not add a `--jobs`/concurrency option, and do
   not add a fallback chain across providers within a run.

3. **Never print lyrics content to stdout/stderr.** Log outcomes and paths
   (`fetched <path>`), not lyric bodies.

## Module map

```text
src/
  lib.rs      : module declarations; the library crate integration tests link against
  main.rs     : thin binary: parses CLI, wires it to runner, sets the process exit code
  cli.rs      : clap derive definitions (Cli, Command, SharedOptions)
  meta.rs     : TrackMeta resolution: tag reading (lofty) + optional --path-fallback
  provider.rs : ProviderKind enum + ProviderSpec (base URLs, display name) per provider
  http.rs     : HTTP client: throttling, retry/Retry-After handling, /api/get, /api/search,
                candidate scoring; shared across every provider, parameterized by ProviderSpec
  sidecar.rs  : sidecar path derivation, on-disk state detection, atomic writes
  runner.rs   : per-track decision logic (process_track) and the scan walk
tests/
  read_only_guarantee.rs  : integration test asserting audio files are unchanged after a run
  fixtures/sample.flac    : small tagged fixture (regenerate with the ffmpeg command below)
```

Where a change belongs: CLI surface goes in `cli.rs`. Metadata resolution goes in
`meta.rs`. Adding a new LRCLIB-API-compatible provider is one match arm in
`ProviderKind::spec()` in `provider.rs`. Talking to a provider (throttling, retries,
request shape) goes in `http.rs`. What file gets written where, or what state a
sidecar is already in, goes in `sidecar.rs`. The decision of what to do with a track
goes in `runner.rs`. A provider whose response shape isn't LRCLIB-compatible doesn't
fit this seam.

## Commands

The `Makefile` is the source of truth for build/test/install commands. Use it rather
than calling `cargo` directly. `make help` lists every target.

```sh
make build        # cargo build
make release       # cargo build --release
make run ARGS="scan ~/Music -v"
make test          # cargo test
make lint          # cargo clippy --all-targets -- -D warnings
make lint-md       # markdownlint-cli2 (via npx) on every *.md file
make fmt           # cargo fmt, applies changes
make fmt-check     # cargo fmt --check, verifies only
make check         # fmt-check + lint + lint-md + test, the pre-commit gate
make audit         # cargo audit, dependency security advisories
make install       # cargo install --path . --force
make uninstall     # cargo uninstall lyrics-sidecar
make clean         # cargo clean
```

`make check` must pass before considering a change done.

## Testing conventions

- Unit tests (in `#[cfg(test)] mod tests` at the bottom of each module) run offline,
  against fixtures. No network calls in `cargo test`, ever.
- `tests/read_only_guarantee.rs` is the one integration test; it depends on the crate
  as a library (`src/lib.rs`), not on `#[path]` includes. Keep new integration tests
  the same way.
- Regenerate the fixture, if needed, with:

  ```sh
  ffmpeg -y -f lavfi -i "sine=frequency=440:duration=2" \
    -metadata title="Test Track" -metadata artist="Test Artist" -metadata album="Test Album" \
    tests/fixtures/sample.flac
  ```

- Live calls against a real provider API are for manual verification only. Never wire
  a live network call into `cargo test`.

## Style

- `anyhow::Result` at fallible boundaries (`Client`, `process_track`, `main`); no
  `unwrap()` outside tests.
- Keep comments load-bearing: explain *why*, especially around the invariants above.
- Markdown line-length limit is 100 (see `.markdownlint-cli2.jsonc`).
- Rust edition 2024, MSRV 1.89 (`Cargo.toml`).
- The HTTP client is blocking (`ureq`), not async. Sequential requests are a design
  choice, not a limitation.
