# slowloris

A small, statically-linkable **slow-HTTP resilience tester** written in Rust.

It reimplements the classic [Slowloris](https://en.wikipedia.org/wiki/Slowloris_%28computer_security%29)
technique — and two of the additional slow-connection modes popularized by
[`slowhttptest`](https://github.com/shekyan/slowhttptest) — as a single,
dependency-free binary you can drop onto an `arm64` box (a Raspberry Pi, a
Graviton instance, a minimal container) to test how your own web server behaves
under connection-pool exhaustion.

> [!WARNING]
> **Authorized testing only.** Slow-HTTP techniques deny service by holding a
> server's connections open. Run this **only** against servers you own or have
> explicit, written permission to test. Using it against systems you are not
> authorized to test is likely illegal in most jurisdictions. You are
> responsible for how you use it.

## Why this instead of `slowhttptest`?

`slowhttptest` is excellent and does more (CSV/HTML graphs, the Apache range
attack). This project exists for one reason: **deployment**. It builds to a
single statically-linked `musl` binary with no runtime dependencies — no
`apt install`, no Python, no shared libraries to chase — so testing from an
arbitrary `arm64` host is `scp` + run. (`wrk`, by contrast, is a *throughput*
benchmark that sends complete requests as fast as possible; it does not
exercise the slow-connection failure mode at all.)

## Modes

| Mode | Flag | What it does |
|------|------|--------------|
| `headers` (default) | `-m headers` | Classic Slowloris: opens connections and sends request **headers** slowly, never sending the terminating blank line, so the server keeps waiting. |
| `body` | `-m body` | Slow POST: sends complete headers with a large `Content-Length`, then dribbles the request **body** out far slower than advertised. |
| `read` | `-m read` | Slow read: sends a complete, valid request but advertises a tiny receive window and reads the **response** a few bytes at a time. |

## Usage

```
slowloris <URL> [OPTIONS]

Arguments:
  <URL>  Target URL, e.g. http://127.0.0.1:8080/ or https://example.test

Options:
  -m, --mode <MODE>                 headers | body | read   [default: headers]
  -c, --connections <N>             concurrent connections to maintain [default: 150]
  -i, --interval <SECONDS>          seconds between follow-up data      [default: 15]
  -r, --rate <N>                    new connections per second while ramping [default: 50]
  -l, --duration <SECONDS>          stop after N seconds (0 = until Ctrl-C) [default: 0]
      --path <PATH>                 request path override
      --content-length <BYTES>      advertised body size for `body` mode [default: 8192]
      --timeout <SECONDS>           per-socket connect/IO timeout        [default: 10]
      --user-agent <STRING>         User-Agent (random desktop UA if unset)
  -k, --insecure                    skip TLS certificate verification
  -v, --verbose                     print per-connection detail
  -h, --help                        Print help
  -V, --version                     Print version
```

### Examples

```sh
# Classic Slowloris against a local test server, 500 connections
slowloris http://127.0.0.1:8080/ -c 500

# Slow POST against an HTTPS host with a self-signed cert, for 60 seconds
slowloris https://staging.internal/ -m body -c 200 -k -l 60

# Slow read
slowloris http://127.0.0.1:8080/big-file -m read -c 300
```

You'll know the server is vulnerable when a normal client can no longer get a
timely response while the test is running; a resilient server (sensible
connection/header timeouts, a hardened reverse proxy in front) will shed or
refuse the slow connections and stay responsive.

## Building

Requires a C compiler and CMake (for the `aws-lc-rs` TLS backend).

```sh
# Native debug / release
cargo build --release
```

### Static `aarch64` (arm64) musl binary

No `cross` required. The simplest path is to build **on an arm64 Linux host**
(including GitHub's `ubuntu-24.04-arm` runners) with the musl toolchain
installed natively:

```sh
sudo apt-get install -y musl-tools cmake
rustup target add aarch64-unknown-linux-musl

CC_aarch64_unknown_linux_musl=musl-gcc \
  cargo build --release --target aarch64-unknown-linux-musl

file target/aarch64-unknown-linux-musl/release/slowloris
# -> ELF 64-bit LSB executable, ARM aarch64, statically linked
```

To cross-compile from an x86_64 host without `cross`, install an
`aarch64-linux-musl` cross toolchain (e.g. the `aarch64-linux-musl-cross`
tarball from <https://musl.cc>) and point the target's `CC`/`AR` at it:

```sh
export PATH="$PWD/aarch64-linux-musl-cross/bin:$PATH"
export CC_aarch64_unknown_linux_musl=aarch64-linux-musl-gcc
export AR_aarch64_unknown_linux_musl=aarch64-linux-musl-ar
export CARGO_TARGET_AARCH64_UNKNOWN_LINUX_MUSL_LINKER=aarch64-linux-musl-gcc
rustup target add aarch64-unknown-linux-musl
cargo build --release --target aarch64-unknown-linux-musl
```

CI (`.github/workflows/ci.yml`) builds and uploads static `x86_64` and
`aarch64` musl binaries on every push.

## License

Dual-licensed under either of [Apache-2.0](LICENSE-APACHE) or [MIT](LICENSE-MIT)
at your option.
