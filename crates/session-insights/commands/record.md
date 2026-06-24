---
description: 今セッションを Obsidian の record ノートに記録する。数値（コスト/トークン/ターン）は session-insights が自動充填し、散文（完了サマリ/学び/振り返り/残課題/関連）をあなたが埋める。AEGIS の /record 相当。
---

You are running the **session-insights `/record`** command: write a human-readable
session record note to the user's Obsidian vault. The deterministic numbers
(cost, tokens, turns, files, context) are produced by the bundled binary; YOUR
job is to author the Japanese prose sections based on THIS conversation.

## Steps

1. **(Re)generate the deterministic note and get its path.** Run:
   ```
   ${CLAUDE_PLUGIN_ROOT}/bin/session-insights record-now
   ```
   (If `CLAUDE_PLUGIN_ROOT` is unset, fall back to `bin/session-insights record-now`
   from the plugin dir.) It resolves the current session from
   `$CLAUDE_CODE_SESSION_ID`, refreshes the `## コスト` / `## 数値サマリ` blocks,
   and prints the **absolute path** of the note. Capture that path.
   - If it prints nothing and writes `no record note written (Obsidian vault not
     found: …)` to stderr, the vault directory does not exist. Tell the user the
     resolved `obsidian_vault` path and that they must create it (or set
     `obsidian_vault` / enable `record` in `session-insights.toml`), then stop.

2. **Read the note** at the printed path.

3. **Fill the prose.** Replace each `<!-- fill: <section> -->` placeholder with
   concise Japanese prose grounded in this session. Mirror the AEGIS record
   intent per section:
   - `## 完了サマリ` — 2–4 行で、何を達成したか（変更したファイル/コミット/動いた機能）。
   - `## つまずき / 学び` — 詰まった点と、その解決から得た学び。
   - `## 振り返り / 確立した方針` — 今後に効く方針・判断基準（**省略しない**）。
   - `## 残課題` — 未完了・先送り・次にやること。
   - `## 関連` — 関連ノートへの `[[リンク]]`（あれば）。
   If a section genuinely has nothing, write a short honest line (e.g. `特になし`)
   rather than leaving the placeholder.

   **Distill via a fresh Sonnet subagent — never switch the main model.** Spawn a
   subagent with `model: "sonnet"` and have it return just the per-section bullet
   points. Pass the transcript as a **file path**, not pasted text, so the
   subagent's fresh context only loads what it reads:

   ```
   Agent(
     subagent_type: "general-purpose",   // or ctxrot:ctxrot-distiller
     model: "sonnet",
     prompt: "Read this session transcript and return ONLY the per-section
              bullet points for a record note: 完了サマリ / つまずき・学び /
              振り返り・確立した方針 / 残課題 / 関連. Do not implement anything.
              Transcript: <abs path to $CLAUDE_CODE_SESSION_ID.jsonl under
              ~/.claude/projects/<slug>/>"
   )
   ```

   Resolve the transcript path from `$CLAUDE_CODE_SESSION_ID` (the `.jsonl` lives
   under `~/.claude/projects/<cwd-slug>/`). The subagent starts with an empty
   context, so even a long Opus session distills under Sonnet's window.

   **Do NOT distill by switching the live session to Sonnet** (e.g. `/model
   sonnet`). If this is an Opus 1M-context session whose live context already
   exceeds Sonnet's 200k window, switching mid-session overflows and errors
   ("prompt is too long"). Keep the main loop on Opus; only the subagent runs
   Sonnet. (Same spirit as the `/distill` skill.)

4. **Save** the note back to the same path.

5. **Reconcile the cross-session backlog.** The per-session `## 残課題` is a
   snapshot; the backlog is the durable, self-pruning list of what's still open
   across sessions (one global note `<vault>/backlog.md`, surfaced at SessionStart).
   Use the project name from the record note heading as `<P>`.
   1. **See what's open:**
      `${CLAUDE_PLUGIN_ROOT}/bin/session-insights backlog list --project <P> --json`
   2. **Resolve what this session closed.** For each open item that is now done
      (compare against what was actually accomplished this session), mark it:
      `… backlog resolve --id <bk-id> [--id <bk-id> …]` (resolved items are
      removed from the note entirely — exclusion is the point).
   3. **Add genuinely-open follow-ups** from this session's `## 残課題`:
      `… backlog add --project <P> --text "<one open item>"` (one call per item;
      re-adding the same text is idempotent, so existing items won't duplicate).
      Do NOT add items you just resolved, or transient notes — only durable
      "still needs doing" work.
   4. **Re-render the note:** `… backlog render` (prints the backlog note path).
   Keep backlog item text short and action-oriented ("X が必要", "Y を直す") so the
   SessionStart brief reads as a clear to-do list.

## Hard rules

- **Never edit anything between** `<!-- si:numeric:start -->` … `<!-- si:numeric:end -->`
  **or** `<!-- si:cost:start -->` … `<!-- si:cost:end -->`. Those blocks are
  machine-owned and will be overwritten on the next `record-now` / SessionEnd.
  Only replace the `<!-- fill: … -->` lines and the prose around them.
- **Do not add YAML frontmatter** — this note format is intentionally
  frontmatter-free (matching the user's hand-written AEGIS session notes).
- Keep the existing section order and headings; don't restructure the note.
- If `$ARGUMENTS` is given, treat it as a focus hint for the prose (e.g. which
  thread of the session to emphasize), not as a new filename.
- **Never switch the main session model** to do the distillation. Distill only
  via a Sonnet subagent (step 3); the main loop stays on whatever model the
  session started with.
