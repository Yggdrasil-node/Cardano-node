---
title: Installation
layout: default
parent: User Manual
nav_order: 2
---

# Installation

## Prerequisites

A machine with:

- **OS**: Linux (Ubuntu 22.04 LTS / Debian 12 / RHEL 9 or compatible). macOS works for development.
- **CPU**: x86_64 or aarch64; at least 4 cores for a relay, 8 for a block producer.
- **RAM**: 16 GB minimum, 32 GB recommended for mainnet block production.
- **Storage**: 500 GB SSD (NVMe preferred). The mainnet immutable + volatile chain currently grows at roughly 8–12 GB per month.
- **Network**: stable IPv4 (or IPv4 + IPv6) with at least 25 Mbit/s symmetric.
- **Time**: NTP synchronised to within ±100 ms of UTC. Drifting clocks cause `BlockFromFuture` rejection of valid peer blocks.

Software:

- A C toolchain and `pkg-config` (`build-essential` on Debian/Ubuntu).
- `git`, `curl`.
- The Rust toolchain — installed below.

## Install Rust

Yggdrasil pins the Rust toolchain version in [`rust-toolchain.toml`](https://github.com/yggdrasil-node/Cardano-node/blob/main/rust-toolchain.toml). Currently `1.95.0` (Edition 2024).

If you do not have `rustup`:

```bash
$ curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y
$ source "$HOME/.cargo/env"
```

`rustup` will read `rust-toolchain.toml` automatically when you build the workspace and download the pinned compiler if missing. You do not need to run `rustup install` manually.

Verify:

```bash
$ rustc --version
rustc 1.95.0 (...)
```

## Clone the repository

```bash
$ git clone https://github.com/yggdrasil-node/Cardano-node.git yggdrasil
$ cd yggdrasil
```

The default branch is `main` and is always at a green-gates commit.

## Build a release binary

```bash
$ cargo build --release --bin yggdrasil-node
```

First build will fetch and compile dependencies; expect 5–15 minutes on a typical server. Subsequent rebuilds are incremental and finish in seconds.

The binary lands at `target/release/yggdrasil-node`. It is statically linked against everything except `glibc`/`libc`, so you can copy it between machines of the same architecture.

## Verify the build

Run the test suite:

```bash
$ cargo test-all
```

The full workspace runs about **4,635 tests** across all crates. Failure-free output is the green-gates baseline.

For a faster smoke check before shipping the binary:

```bash
$ ./target/release/yggdrasil-node --version
yggdrasil-node 0.1.0
$ ./target/release/yggdrasil-node default-config | head -20
```

## Install the binary system-wide

For convenience:

```bash
# cp target/release/yggdrasil-node /usr/local/bin/
# chmod 755 /usr/local/bin/yggdrasil-node
```

Or run directly from the build directory if you prefer not to install globally.

## Create a system user (production)

Recommended for any production deployment:

```bash
# useradd --system --create-home --home-dir /var/lib/yggdrasil --shell /usr/sbin/nologin yggdrasil
# mkdir -p /var/lib/yggdrasil/db /var/lib/yggdrasil/config
# chown -R yggdrasil:yggdrasil /var/lib/yggdrasil
```

The node will write its chain database under `--database-path` (default: current working directory). Pointing it at `/var/lib/yggdrasil/db` keeps mutable state out of the system root.

## Open firewall ports

A relay needs:

- **3001/tcp** inbound for peer-to-peer (or whatever you set `--port` to).
- The metrics port (default disabled; if enabled with `--metrics-port`, bound to `127.0.0.1` only — open it only on your monitoring network if you want external Prometheus scraping).

A block producer should **not** accept inbound connections from the public internet. Only its own relays should connect.

Example with `ufw`:

```bash
# ufw allow 3001/tcp
# ufw reload
```

## Sanity-check the configuration before first run

```bash
$ yggdrasil-node validate-config --network mainnet --database-path /var/lib/yggdrasil/db
```

This runs the operator preflight: it loads the configuration, verifies vendored genesis hashes, checks the storage state, validates KES/Praos invariants if credentials are configured, and reports warnings (storage uninitialized on a fresh setup is expected).

## Where to go next

You have a working binary. Continue to [Quick Start]({{ "/manual/quick-start/" | relative_url }}) to sync your first node.
