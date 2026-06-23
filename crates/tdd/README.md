# tdd

> **Test-first gate for Claude Code**, written in Rust.

Agents skip tests, and "test-first" is usually just a claim. `tdd` makes both
**enforced** and **verifiable**:

| Surface | When | Guarantees |
|---|---|---|
| **`tdd gate`** (Stop hook) | every Stop | implementation code can't land **without a test** — a blocked stop feeds the reason back so the agent writes one |
| **`tdd red` / `green` / `verify`** | driven by `/tdd` | **test-first**: `red` requires the tests to *fail* first and records a proof; `green` requires a prior `red` then a *pass*; `verify` confirms RED→GREEN happened |

It is the *test-first* sibling of the harness's other gates:

| Gate | When | Asks |
|---|---|---|
| `precommit-audit` | pre-commit | does the diff obey policy? (static) |
| **`tdd`** | **on Stop / in /tdd** | **was a test written, first?** |
| `donegate` | on Stop | does it actually build & pass? (runs it) |
| `specguard` | on demand | did the impl drift from spec? (LLM) |

No API key. `tdd` is a deterministic Rust binary that reads `git` and spawns the
test command; the LLM labor (writing the test, implementing) runs in your Claude
Code subscription.

## How the Stop gate works

On each stop `tdd gate`:

1. reads the hook JSON from stdin (`session_id`, `cwd`);
2. loads `./tdd.toml` (or `~/.tdd/config.toml`, or language-aware defaults);
3. asks `git` what changed and which lines were **added**;
4. counts *added implementation lines* (impl-glob files, minus test files and
   inline test markers) and looks for **test evidence** (an added `#[test]` /
   `def test_` / `func Test…` / `it(...)`, or a changed file under `tests/`);
5. **impl added + no test** → `{"decision":"block","reason":…}` so Claude writes
   a test and continues; otherwise the stop proceeds.

A per-session attempt counter gives up after `max_attempts` so a stuck agent is
never trapped. Escape hatch: a one-line `.tdd-skip` file in the project root
(consumed once) for genuine refactors/renames/docs. Kill switch: `TDD_DISABLE=1`.

## Test-first proof (`/tdd` skill)

```
/tdd <behaviour you want>
  Phase 1  design the API (stubs: todo!())
  Phase 2  write the tests
  Phase 3  tdd red   --task <id>   →  RED  (must fail)   → .tdd/<id>.red.json
  Phase 4  implement
  Phase 5  tdd green --task <id>   →  GREEN (must pass)  → .tdd/<id>.green.json
  Phase 6  tdd verify --task <id>  →  RED→GREEN verified
```

`tdd red` refuses to record a proof if the tests already pass (that isn't
test-first); `tdd green` refuses without a prior RED proof.

## Install

Via the plugin marketplace (wires the Stop hook through `hooks/hooks.json` and
ships the `/tdd` skill). For non-plugin use: `tdd install` merges the Stop hook
into `~/.claude/settings.json`; `tdd init` writes a starter `tdd.toml`.
