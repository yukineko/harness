# harness 使い方パターン集

各プラグインの詳細は `crates/<plugin>/README.md`、全体設計は [OVERVIEW.md](OVERVIEW.md) を参照。
ここでは「セッションを開いてから何を打てばよいか」を典型パターンで示す。

---

## パターン 1 — "次に何をすべきか分からない" (Phase 0-next)

```
/condukt 次は何をする
```

open run が 0 件 かつ引数が「次の一手を探す」系のとき、condukt は以下を自動で確認して
最優先の一手を選ぶ:

1. `session-insights backlog list` — open バックログ項目
2. `compass gap` — 北極星ゴールとの gap が示す next_action
3. `git log --oneline -10` — 直近コミットから自明な続き

候補が複数あり判断できない場合は `AskUserQuestion` で提示する。

**例: バックログに未解決項目がある場合**

```
セッション開始
  └─ SessionStart hook: condukt(restore) が "未完了 run 1件" を通知
                        specguard pending が drift sentinel を提示 (あれば)
                        compass が北極星ゴールを再提示

/condukt 次は何をする
  └─ backlog に [open] decision-log プラグイン実装 が 1 件 → それを課題文として採用
  └─ Phase 1: interpreter が分割
  └─ Phase 3: AskUserQuestion で確認
  └─ Phase 5-6: 並列実装 → 検証
  └─ Phase 8: specguard が自動で drift 監査
```

---

## パターン 2 — compass で目標を再接地してから実装

北極星ゴールが曖昧になってきた、または久しぶりにセッションを開いたときに使う。

```
/compass
```

compass は現状のコードベースと charter (目標ドキュメント) を照合して gap を示し、
"右サイズの一手" を提案する。その提案文をそのまま condukt に渡す。

```
/condukt <compass が提案した一手>
```

**例: 新機能の方向性を compass に聞いてから実装**

```
/compass
  └─ gap: "PDO ワークフローに外部 MCP 認証が未統合"
  └─ next_action: "Linear MCP の authenticate を設定し condukt に組み込む"

/condukt Linear MCP の authenticate を設定し condukt に組み込む
  └─ Phase 0.5: researcher が Linear MCP API を調査
  └─ Phase 1: interpreter が分割 (認証設定 / condukt hook 追加 / テスト)
  └─ ...
```

---

## パターン 3 — backlog から直接選んで実装

`session-insights backlog list` でバックログを確認し、特定の項目を指定する。

```
session-insights backlog list
/condukt <バックログ項目の内容>
```

または `バックログ ID` を使って:

```
/condukt bk-b297184b の decision-log プラグインを実装する
```

---

## パターン 4 — 具体的な課題を直接渡す (最もシンプル)

何を実装するか決まっているとき。

```
/condukt specguard の bin/ にある pre-compiled binary を使うよう hooks.json を修正する
```

condukt は Phase 0 で open run をチェックし、無ければそのまま Phase 0.5 → 1 → 3 と進む。

**dry-run で計画だけ確認したい場合:**

```
/condukt --dry-run <課題>
```

Phase 3 の schedule 提示まで進んで停止する。

---

## パターン 5 — 中断した run を再開

セッションが途中で切れたり、`/stop` したりした場合。

**open run が 1 件だけなら引数なしで自動再開:**

```
/condukt
```

Phase 0 で open run を検知し `AskUserQuestion` なしで自動的に Phase 0-alt (resume) に入る。

**run ID を明示して再開:**

```
/condukt --resume run-20260626-xxxxxx
```

**stuck worker がいる場合 (running 状態が残っている):**

```
condukt state abandon --run <RID> --all-stuck
/condukt --resume <RID>
```

---

## パターン 6 — 実装前に仕様を確認する (specguard brief)

コードを書き始める前に「この変更は既存の仕様と整合しているか」を確認する。

```
/specguard:brief specguard の hooks.json を CLAUDE_PLUGIN_ROOT 形式に変更する
```

specguard は関連する canon ドキュメントを read-only で読み、変更が仕様と矛盾しないか・
仕様に沿ったアプローチを採っているかをブリーフィングする。その後 `/condukt` で実装。

---

## パターン 7 — drift が見つかったセッション開始

```
SessionStart hook: specguard pending
  → "修正候補あり: condukt-interpreter.md のスキーマに linked_hypotheses が未追加"

(修正する)

/specguard:ack    # sentinel をクリア
```

drift を放置したまま実装を続けると次回の監査でも同じ指摘が出る。
`ack` は修正が完了してから呼ぶ。

---

## セッション開始チェックリスト

セッションを開いたとき SessionStart フックが以下を順に提示する:

| フック | 何を表示するか |
|---|---|
| `ctxrot restore` | 前セッションの決定事項・残課題 (ctxrot ノート) |
| `condukt restore` | 未完了 run・orphan worktree の有無 |
| `compass` | 北極星ゴールのリマインド |
| `specguard pending` | 未処理 drift sentinel (あれば) |
| `taskprog` | progress.md の現状 |

これらを読んでから上記パターンのどれかで作業を始める。

---

## よく使うコマンド早見表

| やりたいこと | コマンド |
|---|---|
| 次の一手を自動決定 | `/condukt 次は何をする` |
| 北極星を再接地 | `/compass` |
| バックログ確認 | `session-insights backlog list` |
| 具体的な課題を実装 | `/condukt <課題>` |
| 計画だけ確認 | `/condukt --dry-run <課題>` |
| 中断した run を再開 | `/condukt` (1件のみ) or `/condukt --resume <RID>` |
| 実装前に仕様確認 | `/specguard:brief <課題>` |
| drift 監査を手動実行 | `/specguard:run` |
| drift sentinel を解除 | `/specguard:ack` |
| コスト・進捗を確認 | `/harness-status:status` |
