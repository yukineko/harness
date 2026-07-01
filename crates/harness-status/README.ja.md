# harness-status

> 🌐 [English](README.md) ・ **日本語**

**Claude Code 向けの統合 HOTL ステータスダッシュボード (Rust 製)**

harness の各プラグインはそれぞれ独自の状態ストアを持つ。harness-status はそれらを
横断して読み取り、human-on-the-loop の視点 — 介入すべきかどうかを一目で判断するために
眺めるもの — を 1 画面にまとめて表示する:

- **予算 (budgetguard)**: 今日の支出額とセッション数。`~/.budgetguard/state/ledger.json`
  から読む。
- **直近セッション (gauge)**: 直近 N 件のセッションの turn 数・トークン数・USD コスト。
  `~/.gauge/store/sessions/` から読む。
- **進捗ファイル (taskprog)**: カレントディレクトリの `.claude/progress.md` のプレビュー。

**read-only** であり、書き込みは一切せず、他プラグインのストアを集約するだけである。hook も
API キーも不要で、必要なときに手動で (または `/status` コマンドから) 実行する単一バイナリである。

**活性化スコープ: 手動 (CLI 専用)、これは意図的である。** harness-status は統合された
手動 human-on-the-loop 検査ダッシュボードである。**hook を一切登録しない** — `SessionStart`
すら登録しない — のは、毎セッション自動でダッシュボードを注入すれば、まさにこのツール
(`hooks` / `inject`) と [ADR 0001](../ctxrot/docs/adr/0001-cross-harness-injection-budget.md)
が抑えようとしている always-on の注入/hook 予算を膨らませてしまうからである。3 つのスコープの
分類体系と全プラグインの現在の分類については
[`docs/plugin-activation-scopes.md`](../../docs/plugin-activation-scopes.md) を参照。

## 出力

```
╔══════════════════════════════════════════════╗
║         harness-status  (2026-06-23)         ║
╚══════════════════════════════════════════════╝

── Budget (budgetguard) ──────────────────────────
  Today spend:  $1.8420  (3 session(s))

── Recent sessions (gauge) ───────────────────────
  Session          Project              Turns       Tokens  Cost USD
  ----------------------------------------------------------------------
  3c8d91a2         harness                 12        35000    0.1850

── Progress file (taskprog) ──────────────────────
  cwd: /repo
  /repo/.claude/progress.md
  │ # Progress
  │ ## Pending
  │ - specforge ⑤ worktree merge
```

## インストール (プラグイン)

```
/plugin install harness-status@yukineko
```

導入後、任意のセッションで `/status` を実行する。

## 手動インストール

```sh
cargo install --path .
harness-status            # フルダッシュボード
```

## コマンド

```sh
harness-status                       # フルダッシュボード
harness-status budget                # 今日の支出のみ
harness-status sessions --sessions 10  # 直近セッション (件数 N を指定)
harness-status progress              # 進捗ファイルのみ
harness-status hooks                 # Stop-hook レイテンシ集約 (予算モニタ)
harness-status inject                # UserPromptSubmit 注入サイズ集約 (予算モニタ)
harness-status plugins               # 全プラグインを活性化スコープで分類
harness-status --json                # 機械可読出力 (任意のサブコマンドに付与可)
```

`plugins` サブコマンドはモノレポをスキャンし、全プラグインを
**always-on** / **event-scoped** / **manual** に分類する
([`docs/plugin-activation-scopes.md`](../../docs/plugin-activation-scopes.md) を参照)。これは
dev/HOTL 用ツールであり、リポジトリのレイアウトから分類するため、チェックアウトから実行すること。

## 補足

- あるセクションが "not installed" と出るのは、そのプラグインのストアが存在しないだけで、
  エラーではない。
- `harness-status hooks` は、レイテンシが `HARNESS_HOOK_LATENCY_BUDGET_MS` (デフォルト
  `30000`) を超える Stop-hook を警告する。
- `harness-status inject` は、`HARNESS_INJECT_BUDGET_CHARS` (デフォルト `20000`) を超える
  UserPromptSubmit 注入を警告する。
- 日付はクロックに依存せず導出される。テスト時は `HARNESS_DATE=YYYY-MM-DD` で上書きできる。

## ライセンス

MIT
