# ship

**Shipping ritual for Claude Code**, written in Rust.

On `SessionEnd`, it detects unshipped work (uncommitted git changes, stale plugin-cache) and prints a reminder.
Guides you through the shipping workflow: check, rebuild plugins (safe), commit, merge, push.

**GATED invariant**: commit, merge, and push require explicit user approval. Only `scripts/rebuild-plugins.sh` 
(via `ship check --run-safe`) is auto-runnable. The shipping ritual is user-driven; the agent provides the checklist 
and prompts for approval before each gate.

Subscription-native: one Rust binary, one hook (SessionEnd), no API key.

## Commands

```sh
ship check             # print unshipped state (dirty git, stale plugin-cache)
ship check --run-safe  # run scripts/rebuild-plugins.sh (SAFE: the only auto-step)
ship session-end       # SessionEnd hook (reads hook JSON from stdin, prints reminder if unshipped)
```

## Workflow

1. **Diagnostic**: `ship check` to see what is unshipped.
2. **Auto-rebuild**: `ship check --run-safe` to rebuild the plugin cache from source.
3. **Commit** (user approval required): `git add && git commit -m "..."`
4. **Merge** (user approval required): `git merge <branch>`
5. **Push** (user approval required): `git push origin <branch>`

## SessionEnd hook

On SessionEnd, ship runs automatically and reminds you if there is unshipped work. The reminder is informational 
and never blocks anything.

## /ship skill

After the session, use `/ship` to walk through the shipping ritual step by step. The skill ensures you have 
visibility into what will be committed, merged, or pushed before any action is taken.

## GATED invariant — critical

- **commit, merge, push**: NEVER auto-run. ALWAYS get explicit user approval first. Show diffs, ask "approve?", wait for "yes".
- **rebuild-plugins.sh**: Only this step is auto-runnable via `ship check --run-safe`.

## Install (plugin)

```
/plugin install ship@yukineko
```

## Manual install

```sh
cargo install --path .
ship session-end  # test it
```

## License

MIT
