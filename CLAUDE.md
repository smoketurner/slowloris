# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## What this is

`slowloris` is a single-binary slow-HTTP resilience tester for **authorized** testing of your own servers. It reimplements three slow-connection techniques (slow-headers / slow-POST / slow-read) as a dependency-light, statically-linkable Rust CLI. The entire README carries an authorization warning; keep user-facing output (the engine's startup banner in `engine.rs`) honest about that intent.

## Commands

```sh
make build                      # cargo build --release
make run ARGS="http://127.0.0.1:8080/ -c 200 -l 30"
cargo test                      # all unit tests (cli.rs + rng.rs)
cargo test --locked target      # run a single test by name substring, e.g. `target`
cargo fmt --all --check         # formatting gate (CI enforces)
cargo clippy --all-targets -- -D warnings   # lint gate (CI enforces, zero warnings)
```

CI (`.github/workflows/ci.yml`) runs fmt/clippy/`cargo test --locked` and builds two artifacts: a static `aarch64-unknown-linux-musl` binary and a `aarch64-apple-darwin` binary. There is no x86_64 build — this project targets arm64 only. The musl build needs `musl-tools` and `cmake` (aws-lc-rs compiles C); see README "Building" for the exact `CC_aarch64_unknown_linux_musl=musl-gcc` invocation and the x86→arm64 cross path.

## Architecture

Flow: `main.rs` parses `Args` → builds a `Target` → constructs an `Engine` → `engine.run()` loops forever (or until `--duration`).

- **`cli.rs`** — clap `Args`, the `Mode` enum (`Headers`/`Body`/`Read`), and a hand-rolled `Target::parse` URL splitter (no url crate). A bare `host:port` with no scheme is treated as plain HTTP.
- **`engine.rs`** — the core. The run loop is: `replenish()` (open connections up to `--connections`, capped at `--rate` per tick, stop on connect refusal), `report()`, `sleep(interval)`, `keepalive()`. The three modes diverge in exactly two places: `open_request()` (what initial bytes get sent) and the `keepalive()` match arm (what one "slow tick" does — one more header line / one body byte / one small read). This is the first place to look for any behavior change.
- **`stream.rs`** — `Stream` enum unifies `TcpStream` and a boxed rustls `StreamOwned` behind `Read`/`Write`. `Connector` builds sockets via `socket2` (blocking, per-socket read/write timeouts, `TCP_NODELAY`, optional shrunk `SO_RCVBUF` for slow-read). The nested `tls` module builds the rustls `ClientConfig`; `--insecure` swaps in `NoVerifier` which accepts any cert.
- **`agents.rs`** — static pool of desktop User-Agent strings, picked at random so trivial same-UA heuristics don't bucket all connections together.
- **`rng.rs`** — splitmix64 for cosmetic request jitter only (query strings, bogus header names). Seeded once from `aws-lc-rs` entropy, with a time-based fallback. Not security-sensitive.

### Deliberate design constraints — preserve these

- **Single-threaded, blocking by design.** One thread iterates the whole connection list; the traffic is a few bytes per socket per interval, so this is intentionally enough. Do not reach for async/threads/tokio.
- **Dependency discipline.** Every dependency in `Cargo.toml` is pinned to an exact version (`=x.y.z`) with `default-features = false` and only needed features re-enabled. Each dep is attack surface — justify any addition.
- **TLS is rustls + aws-lc-rs only** — never ring or OpenSSL. aws-lc-rs is also reused as the RNG entropy source, which is why there is no separate rand crate.
- **Size-optimized release profile** (`opt-level = "z"`, `lto`, `codegen-units = 1`, `strip`, `panic = "abort"`) and `mimalloc` in hardened "secure" mode as the global allocator. `panic = "abort"` means no unwinding — don't write code that relies on catching panics.
- Edition 2024, MSRV 1.98.1.
