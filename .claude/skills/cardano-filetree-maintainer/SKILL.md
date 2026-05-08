---
name: cardano-filetree-maintainer
description: Maintain `.claude/filetree` for the Yggdrasil repo when filetree descriptions are stale, missing, or reported by automation. Use for prompts mentioning filetree maintenance, stale file descriptions, `.claude/filetree/manifest.json`, `.claude/filetree/FILETREE.md`, or the filetree staleness automation.
---

# Cardano Filetree Maintainer

Use this skill only for filetree description maintenance.

## Inputs

- Stale paths from `python .claude/scripts/filetree.py check`, or a user
  request to refresh filetree descriptions.
- Current source files in this repository.

## Workflow

1. Run `python .claude/scripts/filetree.py check`.
2. Read only files reported as new or stale.
3. Update `.claude/filetree/manifest.json` descriptions for those paths:
   - Use one or two concise lines.
   - State the file's actual role, not implementation hopes.
   - For large JSON/config files, describe the dataset/config purpose
     without dumping contents.
   - Do not add placeholders, guesses, or optimistic parity claims.
4. Run `python .claude/scripts/filetree.py accept-current`.
5. Run `python .claude/scripts/filetree.py check` and confirm it reports
   no stale entries.

## Constraints

- Do not edit source files as part of this skill.
- Do not edit ignored paths such as `.reference-haskell-cardano-node/` or
  `target/`.
- Do not manually edit `.claude/filetree/FILETREE.md`; regenerate it
  through the script.
- If a description cannot be made accurate without deeper implementation
  or Haskell-reference research, stop and report the required research
  instead of stamping inaccurate metadata.
