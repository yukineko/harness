---
name: add
description: 新しい仮説を hypothesis ストアに追加し、ID を返す。テキストと任意の compass ゴールキーワードを受け取る。
argument-hint: "<仮説テキスト> [--goal <compassキーワード>]"
allowed-tools: Bash(hypothesis:*)
---

# /hypothesis:add — 仮説を追加する

引数のテキストで仮説を新規登録する。`--goal` で compass charter の north_star / definition_of_done キーワードに紐づけられる。

## 手順

1. 引数からテキストを取得する。省略されていれば `AskUserQuestion` で確認する。
2. 以下を実行:
   ```
   hypothesis add "<テキスト>" [--goal "<goal>"]
   ```
3. 返された 8 桁の ID をユーザーに報告する。
4. `--goal` を指定していない場合、compass charter とのリンクを促す一言を添える。

## 出力例

```
追加しました: a1b2c3d4
テキスト: "ユーザーはオンボーディングに時間がかかりすぎると感じている"
ゴールリンク: なし（`--goal` で compass charter と紐づけることを推奨）
```
