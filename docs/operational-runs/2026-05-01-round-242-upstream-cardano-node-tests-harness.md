# Round 242 - Upstream cardano-node-tests harness smoke

Date: 2026-05-01
Phase: E.2 operator evidence hardening
Scope: upstream system-test harness integration evidence and local
devcontainer prerequisites, no consensus/runtime behavior change

### Summary

The official upstream documentation at
<https://tests.cardano.intersectmbo.org/> and the current
`IntersectMBO/cardano-node-tests` repository were checked against
Yggdrasil's runbook guidance. Upstream still documents:

- custom `cardano-node` / `cardano-cli` binaries under `.bin/`;
- selective pytest execution through `PYTEST_ARGS`;
- containerized execution through `runner/runc.sh`.

The current upstream `runner/runc.sh` also validates `.bin/` before it
starts the container. It rejects dynamically linked binaries outside
`/nix`, so a normal GNU release build of `yggdrasil-node` is not
accepted for local container runs. A MUSL/static Yggdrasil binary was
built and accepted by that validation.

### Commands and observations

Static build:

```sh
sudo apt-get install -y --no-install-recommends musl-tools file
rustup target add x86_64-unknown-linux-musl
cargo build --release --target x86_64-unknown-linux-musl --bin yggdrasil-node
```

Static artifact:

```text
target/x86_64-unknown-linux-musl/release/yggdrasil-node
ELF 64-bit LSB pie executable, x86-64, static-pie linked
```

Upstream checkout:

```text
IntersectMBO/cardano-node-tests ef825dd
```

Harness no-op smoke:

```sh
./runner/runc.sh -- true
```

Result: PASS. The NixOS runner image built, `.bin/` validation passed
with the static Yggdrasil binary, and the no-op command exited 0.

### Local devcontainer blocker

Attempting to invoke `.bin/cardano-cli` through `runner/runc.sh` from a
checkout created inside this devcontainer failed because the
Docker-outside-of-Docker daemon could not see devcontainer-created
checkout contents at bind-mount time:

```text
ls: cannot access '.bin': No such file or directory
cardano-cli: command not found
```

This is host/container mount visibility, not a Yggdrasil runtime
failure. GitHub Actions and bare-host Docker runs remain the preferred
paths for real upstream pytest slices. Local devcontainer runs need the
upstream checkout and `.bin/` wrappers on a path visible to the host
Docker daemon, or a Docker-in-Docker setup.

### Documentation and devcontainer changes

- `docs/MANUAL_TEST_RUNBOOK.md` now documents the static/MUSL binary
  requirement for local `runner/runc.sh` runs.
- The runbook wrapper now translates upstream `cardano-cli --version`
  to Yggdrasil's `cardano-cli version` shim and copies Yggdrasil's
  vendored network configs into the mounted upstream checkout.
- `.github/workflows/upstream-cardano-node-tests.yml` now follows the
  same static-binary and copied-config wrapper pattern, so the
  manual-only workflow does not fail upstream `.bin/` validation before
  reaching pytest.
- `.devcontainer/post-create.sh` now installs `musl-tools`, `file`,
  and the `x86_64-unknown-linux-musl` Rust target so rebuilt
  devcontainers can produce upstream-compatible static binaries.

### Status impact

- Upstream `runner/runc.sh` container layer: PASS for no-op smoke.
- Static Yggdrasil binary accepted by upstream `.bin/` validation: PASS.
- Local devcontainer pytest execution: BLOCKED by Docker-outside-of-Docker
  bind-mount visibility for devcontainer-created checkout contents.
- Required long operator gates remain unchanged: preprod §6.5 6h/24h
  and mainnet 24h rehearsals before flipping the BlockFetch default.
