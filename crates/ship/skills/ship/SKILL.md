---
name: ship
description: Ship the current work through the commit・merge・push ritual. Detects unshipped git and plugin-cache state; gated on explicit user approval.
allowed-tools: Bash(ship:*)
---

# /ship — shipping ritual

The shipping ritual has four stages: `ship check` (diagnostic), `ship check --run-safe` (auto-rebuild), commit, merge, push.
**Only the rebuild step is automatic. Commit, merge, and push REQUIRE explicit user approval — never auto-run them.**

## Stages

### 1. Diagnostic: `ship check`

Run this to see the unshipped state (dirty git, uncommitted plugin-cache changes, stale plugins):

```sh
ship check
```

This prints a checklist of what must be done before the repo is clean. It does NOT modify anything.

### 2. Auto-rebuild (safe): `ship check --run-safe`

The ONLY automatic operation:

```sh
ship check --run-safe
```

This runs `scripts/rebuild-plugins.sh` to rebuild the plugin cache from source. Nothing else is auto-executed.

### 3. Commit (GATED — requires user approval)

**Do NOT auto-run.** Get explicit user approval, then:

```sh
git add <files> && git commit -m "<message>"
```

Example:
```sh
git add -A && git commit -m "feat: ...commit message..."
```

### 4. Merge (GATED — requires user approval)

**Do NOT auto-run.** Get explicit user approval, then:

```sh
git merge <branch>
```

or (on a feature branch):
```sh
git checkout main && git merge <feature-branch>
```

### 5. Push (GATED — requires user approval)

**Do NOT auto-run.** Get explicit user approval, then:

```sh
git push [origin <branch>]
```

## GATED invariant

commit, merge, and push are NEVER auto-run. Before executing any of these, you MUST:

1. Show the user what will happen (e.g., `git log --oneline`, `git diff`, `git status`).
2. Get explicit approval ("yes", "confirm", "go ahead", etc.). If the user says "no" or does not confirm, stop and do not proceed.
3. Only after confirmation, run the command.

This gate is non-negotiable. The shipping ritual is user-driven. You provide the checklist and prepare the commands, but the user decides when to ship.

## SessionEnd hook

On every SessionEnd, the ship hook runs `ship session-end` to remind you if there is unshipped work. The reminder is informational only and does not block anything.

## Example flow

```
/ship

→ outputs unshipped state (dirty git, stale plugin-cache)

User: please rebuild
/ship check --run-safe

→ runs scripts/rebuild-plugins.sh

User: commit and push
→ Agent: "I can commit with the following. Approve?"
→ Show: git status, git diff, proposed commit message
User: "Yes, commit."
→ Agent: git add -A && git commit -m "..."
→ git push origin main  (with user approval)
```

## Note

The agent's role is to guide and prepare. The user controls the shipping ritual. If you are unsure whether the user has approved, ask again.
