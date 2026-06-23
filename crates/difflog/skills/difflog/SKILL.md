---
name: difflog
description: 直近のセッション diff-log を読んで、開発者向けの平易な変更ナラティブを生成する。「何をどう変えたか」「なぜか」を一枚の文章にまとめる。
argument-hint: [--session <id>] [--all]
allowed-tools: Bash(difflog:*), Read
---

# /difflog — セッション差分ナラティブ

`difflog last` または `difflog list` でログを特定し、その内容を読んでナラティブを生成します。

## 手順

1. `difflog last` で直近のログを取得（引数に `--session <id>` があれば `difflog list` から探す）。
2. ログの **Commits / Files changed / Stat / Diff** セクションを読む。
3. 以下の構造でナラティブを出力する（日本語、エンジニア向け）:

```
## セッションサマリ — <date>

**変更の概要**: <1〜3文で何を実装・修正したか>

**主な変更点**:
- <ファイル名>: <何を変えたか・なぜか>
- …

**コミット**:
- <sha> <message>

**残課題** (あれば): <次にやること、ブロッカー>
```

4. ナラティブは Markdown で出力し、コードブロックや詳細は最小限にする（スキャンで読める分量に）。
5. ログに diff body がある場合、コードの変更意図を読み取って「なぜか」の説明を補足する。

## 注意

- ナラティブは **生成のみ**。ログファイルは書き換えない。
- コスト節約のため、diffbody が大きい場合は stat と name-status だけ読んで概要を書く。
