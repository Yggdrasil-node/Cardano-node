---
title: Installing from Releases
layout: default
parent: User Manual
nav_order: 2.5
---

# Installing from Releases

Yggdrasil ships pre-built Linux binaries with every tagged release. If you do not need to build from source, this is the fastest path to a running node.

For source builds (recommended for development, custom CPU targets, or platforms without a prebuilt binary), see [Installation]({{ "/manual/installation/" | relative_url }}).

## Available platforms

| Platform              | Archive name suffix          | Build runner          |
|-----------------------|------------------------------|------------------------|
| Linux x86_64 (glibc)  | `yggdrasil-node-<tag>-linux-x86_64.tar.gz`  | `ubuntu-latest`       |
| Linux aarch64 (glibc) | `yggdrasil-node-<tag>-linux-aarch64.tar.gz` | `ubuntu-24.04-arm`    |

macOS and Windows builds are not currently published. Build from source on those platforms.

## Quick install (one-liner)

```bash
$ curl -fsSL https://raw.githubusercontent.com/yggdrasil-node/Cardano-node/main/node/scripts/install_from_release.sh \
    | bash
```

This pulls the latest tagged release for your detected architecture, verifies the SHA256 against the published `SHA256SUMS.txt`, and installs to `/usr/local/bin/yggdrasil-node`.

## Manual install (verifiable)

If you prefer to inspect each step:

```bash
$ TAG=v0.1.0
$ ARCH=$(uname -m | sed 's/x86_64/x86_64/; s/aarch64\|arm64/aarch64/')
$ ARCHIVE="yggdrasil-node-${TAG}-linux-${ARCH}.tar.gz"
$ BASE="https://github.com/yggdrasil-node/Cardano-node/releases/download/${TAG}"

# Download the archive and the aggregated checksums.
$ curl -fsSL -O "${BASE}/${ARCHIVE}"
$ curl -fsSL -O "${BASE}/SHA256SUMS.txt"

# Verify the SHA256 — exits non-zero on mismatch.
$ grep " ${ARCHIVE}\$" SHA256SUMS.txt | sha256sum -c -
yggdrasil-node-v0.1.0-linux-x86_64.tar.gz: OK

# Extract.
$ tar -xzf "${ARCHIVE}"
$ cd "yggdrasil-node-${TAG}-linux-${ARCH}"

# Inspect.
$ ls
yggdrasil-node       # binary
configuration/       # vendored mainnet/preprod/preview presets
scripts/             # operator scripts
README.md
LICENSE              # if present
```

Install where convenient:

```bash
# sudo install -o root -g root -m 0755 yggdrasil-node /usr/local/bin/
# yggdrasil-node --version
```

## Verifying the install

```bash
$ yggdrasil-node --version
yggdrasil-node 0.1.0 (commit abc1234)
$ yggdrasil-node validate-config --network mainnet --database-path /tmp/empty
```

If `validate-config` reports zero errors and a few normal warnings (storage uninitialised, peer snapshot missing), you are ready to run.

## Bundled artifacts

Each release archive contains:

- **`yggdrasil-node`** — the binary.
- **`configuration/`** — the vendored mainnet, preprod, and preview presets including genesis files, `config.json`, and `topology.json`. These match the SHAs pinned at release time.
- **`scripts/`** — operator scripts: `install_from_release.sh`, `healthcheck.sh`, `backup_db.sh`, `check_upstream_drift.sh`, `compare_tip_to_haskell.sh`, `parallel_blockfetch_soak.sh`, `restart_resilience.sh`, `run_mainnet_real_pool_producer.sh`, `run_preprod_real_pool_producer.sh`, `yggdrasil-node.service` (systemd unit template).
- **`README.md`** and any LICENSE files.

## Pre-release tags

Tags ending in `-rc`, `-beta`, or `-alpha` are flagged as pre-releases on GitHub. The `install_from_release.sh` script's "latest" resolution skips pre-releases by default. To install one explicitly:

```bash
$ ./install_from_release.sh v0.2.0-rc1
```

## Verifying provenance

Every release artifact is built by [`.github/workflows/release.yml`](https://github.com/yggdrasil-node/Cardano-node/blob/main/.github/workflows/release.yml) running on a hosted GitHub Actions runner. Each archive's SHA256 appears both in its `.sha256` sidecar and in the aggregated `SHA256SUMS.txt`. The workflow run associated with a release is linked from the release page on GitHub.

For higher provenance assurance, build from source and verify against the same source SHA the release was tagged from:

```bash
$ git clone https://github.com/yggdrasil-node/Cardano-node yggdrasil
$ cd yggdrasil
$ git checkout v0.1.0
$ cargo build --release --bin yggdrasil-node
$ sha256sum target/release/yggdrasil-node
# Compare with the release archive's contained binary.
```

Reproducible builds across hosts are not yet a guaranteed property — the binaries contain timestamps and rustc-version-specific code generation. Use the source path for byte-level reproducibility.

## Where to go next

- [Quick Start]({{ "/manual/quick-start/" | relative_url }}) — sync your first node.
- [Running a Node]({{ "/manual/running/" | relative_url }}) — systemd unit and graceful shutdown.
- [Maintenance]({{ "/manual/maintenance/" | relative_url }}) — version upgrades.
