---
name: condukt-loop
description: test→fix→test ループを自動で回すスキル。server/client/e2e の3サイクルに対応し、差分0件 (failure_count 不変) またはテスト全件パスで自動停止する。
argument-hint: --module <server|client|e2e> [--run <RUN_ID>] [--max-iters <N>]
allowed-tools: Task, Bash(condukt:*), Bash(git:*), Read, Write, Edit, Grep, Glob
---

# /condukt-loop — test→fix→test 自動ループ

`/condukt-loop --module <server|client|e2e>` で、テスト失敗→コード修正→再テストを
自動で繰り返す。テスト全件パス、または進捗ゼロ (failure_count 不変) で自動停止する。

## サイクル定義

モジュール種別によって実行ステップが異なる:

| `--module` | ステップ順 |
|---|---|
| `server` | deploy → test |
| `client` | build → test |
| `e2e` | build → deploy → test |

build / deploy コマンドは `~/.condukt/config.toml` の `[loop]` セクションで設定する:

```toml
[loop]
build_command = "npm run build"
deploy_command = "kubectl rollout restart deployment/api"
max_iters = 10
```

## CLI 呼び出し例

```bash
# server サイクル (deploy → test) を最大10回ループ
condukt loop --module server

# client サイクル (build → test)、特定 run に紐付け
condukt loop --module client --run <RUN_ID>

# e2e サイクル (build → deploy → test)、最大イテレーション数を上書き
condukt loop --module e2e --max-iters 5
```

1 イテレーション分の出力 (JSON):

```json
{
  "iteration": 1,
  "failure_count": 3,
  "success": false,
  "stop": false,
  "stop_reason": "",
  "output": "<combined stdout+stderr>"
}
```

`stop=true` のとき `stop_reason` は以下いずれか:

| stop_reason | 意味 |
|---|---|
| `"all tests pass"` | failure_count が 0 になった (正常完了) |
| `"no progress: failure count unchanged"` | failure_count が前回と変わらなかった (差分0件で自動停止) |

## 動作フロー

```
開始
 │
 ▼
condukt loop --module <type> を実行 (1 イテレーション)
 │
 ├─ stop=true ──────────────────────────────────────────────► 終了
 │     └─ stop_reason をユーザーに報告する
 │
 └─ stop=false
       │
       ├─ failure_count == 0 ────────────────────────────────► 終了 (全件パス)
       │
       └─ failure_count > 0
             │
             ▼
           output (stdout+stderr) を参照して失敗テストを特定
             │
             ▼
           condukt-worker subagent に修正を依頼する (Task ツール)
             │
             ▼
           condukt loop --module <type> を再実行 (次のイテレーション)
             │
             └─ max-iters 到達 ─────────────────────────────► 強制終了
```

## 手順

### Step 1 — 引数を受け取る

`$ARGUMENTS` から `--module`, `--run`, `--max-iters` を解析する。
`--module` は必須。未指定の場合はユーザーに確認する。

### Step 2 — ループ開始

```bash
condukt loop --module <server|client|e2e> [--run <RUN_ID>] [--max-iters <N>]
```

JSON を stdout から受け取り、フィールドを読む。

### Step 3 — 停止判定

`stop=true` であれば `stop_reason` をユーザーに伝えて終了する。

- `"all tests pass"` → 成功で終了
- `"no progress: failure count unchanged"` → **差分0件のため自動停止**。
  同じ failure_count が続いており修正が機能していないことを報告する。

### Step 4 — 失敗テストの修正

`stop=false` かつ `failure_count > 0` のとき:

1. `output` フィールドから失敗しているテスト名・エラーメッセージを抽出する。
2. `Task` ツールで **condukt-worker subagent** を起動し、以下を渡して修正を依頼する:
   - 失敗テストの出力 (抜粋)
   - 修正対象ファイルのスコープ
   - 現在のイテレーション番号
3. subagent が修正を完了したら Step 2 へ戻る。

### Step 5 — 強制終了

`--max-iters` (未指定時は config.toml の `max_iters`、デフォルト 10) に達したら
ループを打ち切り、残存 failure_count とともに報告して終了する。

## サイクル別の使い分け

### server サイクル (`--module server`)

**deploy → test** の順で実行する。
アプリケーションサーバーのコード変更後、デプロイを経てインテグレーションテストを検証する場合に使う。
ビルドステップが不要な (スクリプト言語などの) サーバーサイド修正に適している。

```bash
condukt loop --module server --max-iters 8
```

### client サイクル (`--module client`)

**build → test** の順で実行する。
フロントエンドやクライアントライブラリのコード変更後、ビルドを経てユニット/コンポーネントテストを検証する場合に使う。
デプロイが不要な純粋なビルド+テストサイクルに適している。

```bash
condukt loop --module client
```

### e2e サイクル (`--module e2e`)

**build → deploy → test** の順で実行する。
フロントエンドとサーバーの両方を変更するような場合や、エンドツーエンドテストでシステム全体を検証したい場合に使う。
3ステップすべてを経るため最も重いが、最も広い範囲をカバーする。

```bash
condukt loop --module e2e --max-iters 5
```

## 不変条件

1. **差分0件での自動停止** — `no progress: failure count unchanged` が返ったら無条件に停止し、
   同じ修正を繰り返すループには入らない。
2. **max-iters の強制終了** — イテレーション上限を超えたら修正中でも打ち切る。
3. **subagent への移譲** — コード修正は必ず condukt-worker subagent (Task ツール) に委譲し、
   このスキル自体はオーケストレーションに徹する。
4. **JSON のみ信頼** — `condukt loop` の stdout JSON のみを判断基準にする。
   テキスト出力の独自解析で停止判定を行わない。
