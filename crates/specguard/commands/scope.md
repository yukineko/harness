---
description: このランで specguard が監査する解決済みスコープ (baseline / in-scope 領域 / 不変条件 / 決定ログ) を表示する。エージェントは呼ばない。
argument-hint: "[--baseline <ref>]"
allowed-tools: Bash
---

`specguard scope $ARGUMENTS` を Bash で実行し、出力をそのままユーザーに提示してください。
これは read-only の確認用で、エージェント (subagent / claude) は一切呼びません。
非ゼロ終了なら stderr を提示する (config 不在なら `specguard init` を案内)。
