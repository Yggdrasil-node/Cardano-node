# Lessons

- Start by restoring local parity infrastructure before making behavior claims. A green Rust build does not prove upstream parity when the Haskell reference tree is absent.
- Treat "complete" in project docs as evidence-scoped. Code-level implementation, upstream byte/wire evidence, and operator soak evidence are separate states.
- On Windows, byte-parity fixtures need explicit `.gitattributes` LF rules and current-checkout normalization before raw hash or `include_str!` assertions are meaningful.
