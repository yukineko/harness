# GitHub Issues / チケット連携 — クロスセッション競合可視化 設計メモ

## 背景と動機

`condukt state list` は同一マシン上の複数セッションが衝突しているかを検知できるが、
異なるマシン・異なる開発者間の競合は見えない。GitHub Issues (または Linear 等のトラッカー)
と連携することで、「誰が今どのファイル/タスクを触っているか」を cross-machine で可視化し、
merge conflict や設計衝突を事前に防ぐことが目的。

---

## 競合チェックの接続ポイント

### Phase 3.5 (conflict-check) での拡張

`condukt state conflict-check` は現在ローカル state のみを参照する。
GitHub Issues を追加のソースとして参照する場所:

```
condukt state conflict-check --file <json.routed>
                     ↑
                     ここで GitHub Issues を検索して
                     類似タイトルの open issue を "similar_goal_runs" に追加する
```

実装上の接続点: `crates/condukt/src/conflict.rs`（新規作成）または
`run_state` の `StateAction::ConflictCheck` ブランチ (`main.rs:415`付近)。

### run 初期化時の Issue 作成 (Phase 4)

`condukt state init` 実行時に `--github-issue` フラグ（または設定）があれば、
run の goal を title とした GitHub Issue を自動作成し、run state に issue番号を記録する:

```
condukt state init --file <json> [--github-issue]
# → run state に github_issue_number: 123 を追加
# → Issue body に goal/touched_files/run_id を記載
```

run 完了時 (`state gate` PASS) に Issue を自動 close する。

---

## GitHub API アプローチ

### エンドポイント

| 操作 | API |
|---|---|
| Issue 作成 | `POST /repos/{owner}/{repo}/issues` |
| Issue 検索 | `GET /repos/{owner}/{repo}/issues?state=open&labels=condukt` |
| Issue close | `PATCH /repos/{owner}/{repo}/issues/{number}` `{"state":"closed"}` |
| label 付与 | `PUT /repos/{owner}/{repo}/issues/{number}/labels` |

### リクエスト形式

```json
{
  "title": "[condukt] <goal>",
  "body": "**Run ID**: run-20260625-150904\n**Files**: crates/condukt/src/main.rs\n**Session**: @/dev/pts/1",
  "labels": ["condukt"]
}
```

### 類似度チェック (conflict detection)

open issue の title と current run の goal をローカルで比較:
- 現状の `condukt state conflict-check` が使っているのと同じ `cosine_sim` ロジック
  (`crates/condukt/src/sim.rs` 等) を Issue タイトルに適用する
- 閾値: 0.6 以上を "similar" とし `similar_goal_runs` に追加

---

## 認証ハンドリング

### 方針: gh CLI 委譲

`gh` (GitHub CLI) が `$PATH` にある場合は API 呼び出しを `gh api` に委譲することで
トークン管理をユーザーの `gh auth login` 済み設定に任せる:

```bash
gh api repos/{owner}/{repo}/issues \
  --method POST \
  -f title="[condukt] $GOAL" \
  -f body="..."
```

`gh` が無い場合: `GITHUB_TOKEN` 環境変数を直接 `Authorization: Bearer` ヘッダーに使う。

### フォールバック順序

1. `gh api` (gh CLI)
2. `GITHUB_TOKEN` 環境変数 + `reqwest` による直接 HTTP
3. 設定ファイル `~/.condukt/config.toml` の `[github] token`
4. 認証なし → conflict-check の GitHub 部分をスキップ (エラーにしない)

### 設定スキーマ案 (`~/.condukt/config.toml`)

```toml
[github]
enabled = false          # デフォルト無効。明示的に有効化が必要
owner = "yukineko"       # リポジトリオーナー
repo = "harness"         # リポジトリ名
label = "condukt"        # Issues に付けるラベル
create_on_init = true    # state init 時に Issue 自動作成
close_on_complete = true # gate PASS 時に Issue 自動 close
similarity_threshold = 0.6
```

---

## 実装候補タスク (次セッション)

優先度順:

1. **`gh api` ラッパー関数** (`crates/condukt/src/github.rs`)
   - `create_issue(goal, run_id, files) -> Result<u64>`
   - `close_issue(number) -> Result<()>`
   - `list_open_condukt_issues() -> Result<Vec<Issue>>`

2. **conflict-check への統合** (`StateAction::ConflictCheck` ブランチ)
   - `list_open_condukt_issues()` を呼び、goal との cosine 類似度で filter
   - `similar_goal_runs` に GitHub issue 由来エントリを追加 (`source: "github"`)

3. **state init への統合** (config `create_on_init = true` 時)
   - `RunState` に `github_issue_number: Option<u64>` フィールドを追加
   - init 時に create_issue を呼んで番号を保存

4. **gate PASS 時の close** (`StateAction::Gate` ブランチ)
   - config `close_on_complete = true` かつ `github_issue_number` があれば close

5. **ラベル自動作成** (`gh label create condukt --color "#0075ca"`)
   - 初回 init 時に label 存在確認 → なければ作成

### 手を付けないもの (スコープ外)

- PR 連携 (condukt branch と GitHub PR の自動紐付け) — 別設計が必要
- Linear / Jira などの他トラッカー — 抽象 trait 化は後回し
- Issue コメントへの進捗書き込み — ノイズになりやすいので要検討
