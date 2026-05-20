# Release verification

Wave 7 PR 19 added cryptographic signing, build-provenance attestation,
and a CycloneDX SBOM to every Yggdrasil release. This page walks an
operator through verifying each layer.

## Inventory per release

For every supported target (`linux-x86_64`, `linux-aarch64`, …) the
release publishes:

| File | Purpose |
| --- | --- |
| `yggdrasil-node-<tag>-<arch>.tar.gz` | The binary + configs + scripts. |
| `yggdrasil-node-<tag>-<arch>.tar.gz.sha256` | SHA-256 digest. |
| `yggdrasil-node-<tag>-<arch>.tar.gz.sig` | cosign keyless signature. |
| `yggdrasil-node-<tag>-<arch>.tar.gz.crt` | cosign-issued X.509 certificate (binds the signature to the workflow's OIDC identity). |
| `yggdrasil-node-<tag>-<arch>-sbom.cdx.json` | CycloneDX 1.5 JSON SBOM. |
| `yggdrasil-node-<tag>-<arch>-sbom.cdx.json.sig` | SBOM signature. |
| `yggdrasil-node-<tag>-<arch>-sbom.cdx.json.crt` | SBOM certificate. |

Aggregate:

| File | Purpose |
| --- | --- |
| `SHA256SUMS.txt` | Concatenated per-archive checksums. |
| `SHA256SUMS.txt.sig`, `.crt` | cosign-signed aggregate. |

Build-provenance attestations are stored separately by GitHub
(`https://github.com/Yggdrasil-node/Cardano-node/attestations`) and
queried via `gh attestation verify`.

## Operator-side verification

The recommended order is checksum → cosign → attestation → SBOM →
runtime-audit. Each layer catches a different class of tampering.

### 1. Checksum

```bash
sha256sum -c SHA256SUMS.txt
```

Confirms the archive hasn't been re-encoded since publishing.

### 2. Aggregate checksum signature

```bash
cosign verify-blob \
  --certificate SHA256SUMS.txt.crt \
  --signature   SHA256SUMS.txt.sig \
  --certificate-identity-regexp '^https://github\.com/Yggdrasil-node/Cardano-node/' \
  --certificate-oidc-issuer https://token.actions.githubusercontent.com \
  SHA256SUMS.txt
```

Confirms the checksum file was produced by an authorized release
workflow run on `Yggdrasil-node/Cardano-node`.

### 3. Per-archive signature

```bash
ARCHIVE=yggdrasil-node-v0.X.Y-linux-x86_64.tar.gz
cosign verify-blob \
  --certificate "${ARCHIVE}.crt" \
  --signature   "${ARCHIVE}.sig" \
  --certificate-identity-regexp '^https://github\.com/Yggdrasil-node/Cardano-node/' \
  --certificate-oidc-issuer https://token.actions.githubusercontent.com \
  "$ARCHIVE"
```

Confirms the specific archive was signed by the same workflow.

### 4. Build provenance (SLSA Level 3)

```bash
gh attestation verify "$ARCHIVE" --repo Yggdrasil-node/Cardano-node
```

Confirms via GitHub's attestation API that the archive was produced
by the documented workflow against a tracked commit. Equivalent
output via `slsa-verifier`:

```bash
slsa-verifier verify-artifact "$ARCHIVE" \
  --provenance-path <provenance.intoto.jsonl> \
  --source-uri github.com/Yggdrasil-node/Cardano-node
```

### 5. SBOM signature

```bash
SBOM=yggdrasil-node-v0.X.Y-linux-x86_64-sbom.cdx.json
cosign verify-blob \
  --certificate "${SBOM}.crt" \
  --signature   "${SBOM}.sig" \
  --certificate-identity-regexp '^https://github\.com/Yggdrasil-node/Cardano-node/' \
  --certificate-oidc-issuer https://token.actions.githubusercontent.com \
  "$SBOM"
```

### 6. SBOM contents (CycloneDX 1.5)

```bash
jq '{
  name: .metadata.component.name,
  version: .metadata.component.version,
  dep_count: (.components | length),
  components: [.components[].name] | sort
}' < "$SBOM"
```

Feeds into operator vulnerability tooling
(`grype sbom:$SBOM`, `trivy sbom $SBOM`, …) to flag CVEs against
the exact dep set the binary was built with.

### 7. Embedded dependency manifest (`cargo-auditable`)

The binary itself embeds its dep manifest so it can be audited at
deploy-time without re-fetching the SBOM:

```bash
tar -xzf "$ARCHIVE"
cargo install --locked cargo-audit
cargo audit bin yggdrasil-node-*/yggdrasil-node
```

`cargo audit bin` reads the embedded manifest, compares it against
the live RustSec advisory DB, and reports any open CVE on the
build's resolved dep graph.

## Failure modes

| Verification failure | Likely cause |
| --- | --- |
| `sha256sum -c` fails | Archive corrupted in transit; re-download. |
| `cosign verify-blob` fails on identity | Signature not produced by the canonical workflow — possible supply-chain tampering. Stop and escalate. |
| `gh attestation verify` fails | Either the archive isn't from the repo, or the attestation was retracted. Check `gh attestation list --repo Yggdrasil-node/Cardano-node`. |
| SBOM signature OK but contents mismatch | Operator built a custom binary; SBOM was for an upstream build. Re-fetch upstream SBOM. |
| `cargo audit bin` reports unknown advisory | The binary uses a dep affected by a new RustSec advisory. Decide deployment risk in line with `SECURITY.md`'s acceptance criteria. |

## What is *not* verified by these checks

- The Rust source code's correctness. The strict-mirror gate
  (`scripts/check-strict-mirror.py`) and the parity-matrix
  (`docs/parity-matrix.json`, [Compatibility](../COMPATIBILITY.md)) carry
  that contract at development time; release verification confirms
  the binary you have matches the workflow that built it from a
  specific commit, not that the commit itself implements correct
  consensus.
- The runtime behaviour against live mainnet. The §5 hash-comparison
  and §6.5 parallel-BlockFetch rehearsals in
  [`docs/MANUAL_TEST_RUNBOOK.md`](../MANUAL_TEST_RUNBOOK.md) cover that.

## See also

- [`SECURITY.md`](../../SECURITY.md) — disclosure policy.
- [`COMPATIBILITY.md`](../COMPATIBILITY.md) — what surfaces are
  stable across releases.
- [`docs/DEPENDENCIES.md`](../DEPENDENCIES.md) — license / FFI
  posture for every dep in the SBOM.
- [`supply-chain/README.md`](../../supply-chain/README.md) —
  cargo-vet audit framework (Wave 7 PR 21).
