---
description: Check `.claude/filetree/manifest.json` for stale or missing descriptions.
allowed-tools: Bash(python3 .claude/scripts/filetree.py:*)
---

Run `python3 .claude/scripts/filetree.py check` and report stale or
missing entries.

If the manifest does not yet exist (fresh checkout), run
`python3 .claude/scripts/filetree.py scan` to generate the initial
manifest skeleton, then `accept-current`, then `check`. Note that the
initial scan generates entries for every git-tracked file (~660+ paths
in this repo). Pre-populated `DESCRIPTION_OVERRIDES` cover the workspace
core (~25 entries); the rest are blank `description_lines: []` and
flagged stale until a maintainer authors descriptions for them.

If the script reports stale entries:

- Use the `cardano-filetree-maintainer` skill or delegate to the
  `filetree-reviewer` subagent for the actual description authoring.
- Do not edit source files or any path outside `.claude/filetree/`.

If the script returns clean: stop, no further action.
