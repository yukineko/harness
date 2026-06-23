---
name: taskprog
description: プロジェクトの進捗ファイル (.claude/progress.md) を更新する。「何が完了したか」「何が残っているか」「ブロッカーは何か」を箇条書きで簡潔に整理して書き込む。
argument-hint: [--reset]
allowed-tools: Bash(taskprog:*), Read, Write
---

# /taskprog — 進捗ファイル更新

`.claude/progress.md` を最新の状態に更新します。

## 手順

1. 現在の進捗ファイルを確認: `taskprog show`
2. このセッションで完了したタスク・残タスク・ブロッカーを把握する。
3. 以下の構造で `.claude/progress.md` を Write ツールで書き込む:

```markdown
# Progress

Updated: <date>

## Completed
- <task>

## Pending
- <task>

## Blockers
- <issue> (if any)

## Notes
- <context for next session>
```

4. 書き込み後、`taskprog show` で確認。

## オプション

- `--reset`: 進捗ファイルを空にしてから書き直す。

## 注意

- 完了・残・ブロッカーの3セクションは必須。空なら "(none)" と書く。
- ナラティブは不要。箇条書きのみ。
