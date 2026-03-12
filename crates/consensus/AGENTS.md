---
name: consensus-crate-agent
description: Guidance for Ouroboros consensus work
---

Focus on deterministic chain selection, epoch math, rollback handling, and leader-election boundaries.

## Rules
- Prefer explicit types for slots, epochs, and density inputs.
- Keep Praos-specific logic separate from future Genesis extensions.
- Require reproducible fixtures before claiming parity with Cardano behavior.
