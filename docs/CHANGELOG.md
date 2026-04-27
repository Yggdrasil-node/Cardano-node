---
title: Changelog
layout: default
parent: Reference
nav_order: 12
---

# Changelog

The full changelog lives at [CHANGELOG.md](https://github.com/yggdrasil-node/Cardano-node/blob/main/CHANGELOG.md) in the repository root, kept in [Keep a Changelog](https://keepachangelog.com/) format.

## Releases

For installable artifacts (Linux x86_64 + aarch64 binaries with SHA256 checksums), see the [Releases page](https://github.com/yggdrasil-node/Cardano-node/releases).

To install a release, follow the steps in [Installing from Releases]({{ "/manual/releases/" | relative_url }}).

## Release process

Releases are tagged manually by maintainers and built by the
[`release.yml`](https://github.com/yggdrasil-node/Cardano-node/blob/main/.github/workflows/release.yml)
workflow, which:

1. Triggers on `git push` of any tag matching `v*`.
2. Builds release binaries for `x86_64-unknown-linux-gnu` and `aarch64-unknown-linux-gnu` in a matrix of native runners.
3. Strips and tars each binary along with the vendored network presets and operator scripts.
4. Computes per-archive `sha256sum` and aggregates them into `SHA256SUMS.txt`.
5. Generates release notes from the commit log between the previous tag and this one.
6. Publishes a GitHub Release with the archives, sidecar checksums, and the aggregated `SHA256SUMS.txt`.

Pre-release tags (`-rc`, `-beta`, `-alpha`) are flagged as pre-releases on GitHub and skipped by the install script's `latest` resolver.

## Cutting a release (maintainer reference)

```bash
$ git checkout main
$ git pull --ff-only

# Confirm CHANGELOG.md [Unreleased] section is up to date and dated.

$ git tag -a v0.2.0 -m "Yggdrasil v0.2.0"
$ git push origin v0.2.0
```

The workflow takes 20–40 minutes for the matrix build. Watch its progress in the [Actions tab](https://github.com/yggdrasil-node/Cardano-node/actions/workflows/release.yml).
