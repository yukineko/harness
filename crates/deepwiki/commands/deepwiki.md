---
description: Generate or refresh the repository's architecture wiki under .deepwiki/
---

You are running the **deepwiki** command: build or refresh a concise, accurate
architecture wiki for this repository under `.deepwiki/`, committed with the code
(the same idea as Devin Wiki). Do the heavy repo reading in a **subagent** so the
main conversation stays clean.

## Steps

1. **Check freshness.** Run:
   ```
   ${CLAUDE_PLUGIN_ROOT}/bin/deepwiki status
   ```
   - If it reports "no wiki yet" → this is a first full build.
   - If it reports "✅ fresh" → tell the user the wiki is already current and
     stop (unless they asked to force a rebuild).
   - If it reports "⚠ stale" → note the changed source files; this is an
     incremental refresh focused on those areas.

2. **Map the repo.** Run:
   ```
   ${CLAUDE_PLUGIN_ROOT}/bin/deepwiki scan
   ```
   Capture the markdown map (languages, top-level layout, entry points, key
   files, readmes).

3. **Delegate writing to the `deepwiki-writer` subagent.** Pass it: the scan
   output, the repo root, the existing `.deepwiki/` pages (if any), and — for a
   refresh — the list of changed files from step 1. Instruct it to write/update
   pages under `.deepwiki/` and return the list of page filenames it wrote.
   - Always include `.deepwiki/overview.md` (the index: what the project is, the
     big-picture architecture, how the parts fit, where to start reading).
   - Add one page per major module/subsystem when the repo is non-trivial.
   - Every claim should cite real `path:line` references; do not invent files.

4. **Stamp the build** with the pages the subagent reported:
   ```
   ${CLAUDE_PLUGIN_ROOT}/bin/deepwiki stamp overview.md <other-pages…>
   ```

5. **Report** to the user: which pages were written/updated and a one-line
   summary of the architecture. Remind them the wiki lives in `.deepwiki/` and is
   meant to be committed.

## Notes
- Keep pages concise and skimmable — this is a map, not a re-listing of the code.
- Never write secrets or generated/vendored detail into the wiki.
- If `scan` shows an empty or tiny repo, say so and write only `overview.md`.
