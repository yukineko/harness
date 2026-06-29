# harness-status

> 🌐 [English](README.md) ・ **日本語**

**Claude Code 向けの統合 HOTL ステータスダッシュボード (Rust 製)**

## 目的

harness の各プラグインはそれぞれ独自の状態ストアを持つ。harness-status はそれらを
横断して読み取り、human-on-the-loop (HOTL) — 「自分が介入すべきかどうかを一目で
判断するための視点」 — を 1 画面にまとめて表示する **read-only** のバイナリである。

集約する情報は次の 3 つ:

- **予算 (budgetguard)**: 今日の支出額とセッション数。`~/.budgetguard/state/ledger.json`
  から読む。
- **直近セッション (gauge)**: 直近 N 件のセッションの turn 数・トークン数・USD コスト。
  `~/.gauge/store/sessions/` から読む。
- **進捗ファイル (taskprog)**: カレントディレクトリの `.claude/progress.md` のプレビュー。

書き込みは一切行わず、他プラグインのストアを集めて表示するだけである。hook も API
キーも不要で、subscription で完結する。

## どうして必要か

予算・セッション履歴・進捗はそれぞれ別プラグインの別ストアに散在している。状況を
把握するには budgetguard・gauge・taskprog をそれぞれ個別に確認しなければならず、
「今日いくら使ったか」「直近のセッションは高コストでなかったか」「今のタスクの残りは
何か」を一望できない。harness-status はこれらを 1 つのビューに集約し、人間が
「このまま任せるか / 介入するか」を素早く判断できるようにする。

read-only に徹しているため、状態を壊す心配なくいつでも実行できる。あるセクションが
"not installed" と出るのは、そのプラグインのストアが存在しないだけでエラーではない。

## どう使うか

プラグインとして導入すると、任意のセッションで `/status` コマンドが使える。

```
/plugin install harness-status@yukineko
```

`/status` は同梱の Rust バイナリ (`${CLAUDE_PLUGIN_ROOT}/bin/harness-status`) を実行し、
今日の支出・コスト付きの直近セッション・進捗ファイルを 1 画面で表示する。引数で
特定セクションだけに絞れる:

- `/status budget`   → 今日の支出のみ
- `/status sessions` → 直近セッションのみ
- `/status progress` → 進捗ファイルのみ
- `--json` を付けると機械可読出力 (例: `/status --json`)

バイナリを直接使う場合 (手動インストール):

```sh
cargo install --path .
harness-status                         # フルダッシュボード
harness-status budget                  # 今日の支出のみ
harness-status sessions --sessions 10  # 直近セッション (件数 N を指定)
harness-status progress                # 進捗ファイルのみ
harness-status --json                  # 機械可読出力 (任意のサブコマンドに付与可)
```

日付はクロックに依存せず導出される。テスト時は `HARNESS_DATE=YYYY-MM-DD` で上書き
できる。

## ライセンス

MIT
