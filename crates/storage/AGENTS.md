---
name: storage-crate-agent
description: Guidance for durable storage and snapshot work
---

Focus on rollback-aware persistence interfaces and stable on-disk boundaries.

## Rules
- Design storage traits before committing to file formats.
- Keep immutable and volatile concerns separate.
- Preserve a path toward crash recovery and future migrations.
