---
name: backlog
description: クロスプロジェクト backlog のオープンアイテムを順に実装するループコントローラー。ロック取得 → アイテムピック → /condukt で実装 → backlog done で解決、を繰り返し、キューが空になったらロックを解放する。並行セッションとの競合はロックステータスで検出し、ユーザーに確認する。
argument-hint: [オプション: --project <path> でプロジェクト絞り込み]
allowed-tools: Task, AskUserQuestion, Bash(backlog:*), Bash(git:*), Read
---

# /backlog — バックログループコントローラー

`/backlog` で、backlog のオープンアイテムを全件消化するまで **ロック取得 → ピック → 実装 → 完了マーク** を繰り返す。

**役割分担**: ループ制御 (ロック管理・アイテム選択・完了マーク) はこの skill、実装は `/condukt` サブスキル。

## いつ使うか

- `backlog list --status open` でオープンアイテムが 1 件以上ある場合
- 複数プロジェクトにまたがるタスクキューをまとめて消化したいとき
- `--project <path>` を指定すれば特定プロジェクトのアイテムだけを対象にできる

## 手順

### Step 1 — ロックステータスの確認

別セッションがすでにバックログループを実行中でないかを確認する:

```bash
backlog lock status
```

出力に応じて分岐する:

| 状態 | 対応 |
|---|---|
| ロックなし (unlocked) | そのまま Step 2 へ進む |
| ロックあり・セッションが自分 | すでに自分がロックを保持している。Step 2 へ進む |
| ロックあり・別セッション (アクティブ) | `AskUserQuestion` でユーザーに確認 (下記) |
| ロックあり・別セッション (stale) | stale とみなされる場合は Step 2 で強制取得する |

**ロック競合時の `AskUserQuestion`**:

別セッション `<session_id>` がプロジェクト `<project>` でロックを保持している場合、以下の選択肢を提示する:

| 選択肢 | 動作 |
|---|---|
| 待機する | ユーザーが手動で再実行するまで終了する |
| ロックを強制奪取して続行 | Step 2 で `--force` フラグを使ってロックを取得し続行 |
| 中止する | `/backlog` セッションを終了する |

### Step 2 — ロック取得

```bash
backlog lock acquire --session-id <SESSION_ID> --project <CWD>
```

- `<SESSION_ID>` は現在のセッション識別子 (環境変数 `$CLAUDE_SESSION_ID` または `pid-$$` 等)。
- `<CWD>` はカレントディレクトリ (プロジェクトルート)。`--project` 引数が指定されていればその値を使う。
- 強制奪取の場合は `--force` を追加する。

ロック取得に失敗した場合は、理由をユーザーに報告して終了する。

### Step 3 — アイテムループ (繰り返し)

以下のループを「オープンアイテムがなくなる」まで繰り返す:

#### 3-1. 次のアイテムをピック

```bash
backlog list --status open [--project <path>]
```

結果が空（0件）なら → **ループを抜けて Step 4 へ**。

結果が 1 件以上あれば、最上位のアイテム（優先度タグ p0 > p1 > p2、次いで追加順）を選ぶ:

```bash
backlog next [--project <path>]
```

#### 3-2. /condukt で実装

ピックしたアイテムのタイトルと notes を課題文として `/condukt` に渡す:

```
/condukt <アイテムのタイトル>
```

notes に追加コンテキスト（仕様・制約・参照ファイル等）があれば課題文に含める。

`/condukt` は非同期で `Task` ツールを使って起動する（オーケストレーション継続のため）。

#### 3-3. 完了マークまたは失敗マーク

`/condukt` が正常完了したら:

```bash
backlog done <id>
```

`/condukt` が失敗した (blocked / needs-serial 等) 場合:

```bash
backlog fail <id> --reason "<失敗の概要>"
```

失敗アイテムはスキップして次のアイテムへ進む。ユーザーに失敗を通知するが、ループは続行する。

#### 3-4. ループ継続判定

- 3-1 に戻ってオープンアイテムを再確認する。
- ユーザーがループを中断したい場合は Step 4 へ抜ける（途中終了時もロック解放は必須）。

### Step 4 — ロック解放

オープンアイテムが 0 件になった、またはユーザーが中断を選んだ場合:

```bash
backlog lock release
```

ロック解放後、処理したアイテム数・完了数・失敗数をサマリとして報告する。

## ループの早期脱出

以下の状況でユーザーに確認なく（または確認後に）ループを抜けることがある:

| 状況 | 対応 |
|---|---|
| ユーザーが Ctrl-C / 中断を指示 | 直ちに Step 4（ロック解放）へ移行する |
| 連続失敗が 3 件以上続く | `AskUserQuestion` で「続行 / 中止」を確認する |
| `backlog next` が予期しないエラーを返す | エラーを報告し、Step 4 へ移行する |

**早期脱出時もロック解放は必ず実行する。**

## 具体的なコマンド列（全フロー例）

```bash
# 1. 競合チェック
backlog lock status

# 2. ロック取得
backlog lock acquire --session-id pid-1234 --project /home/user/myproject

# 3a. オープンアイテム確認
backlog list --status open --project /home/user/myproject
# → ID=abc-1 "認証モジュールのリファクタリング" が表示された

# 3b. 実装（/condukt に渡す）
# Task: /condukt 認証モジュールのリファクタリング

# 3c. 完了マーク
backlog done abc-1

# 3d. 次のアイテムを確認（もう 0 件なら Step 4 へ）
backlog list --status open --project /home/user/myproject
# → 0 件

# 4. ロック解放
backlog lock release
```

## 失敗モード

- **`backlog` コマンド不在** → README の plugin 導入手順を案内する。
- **ロック取得失敗（競合アクティブ）** → Step 1 の `AskUserQuestion` でユーザーに確認する。
- **`/condukt` が `blocked`** → `backlog fail <id>` でスキップし次のアイテムへ。
- **ループが終わらない（同じアイテムが繰り返しピックされる）** → `backlog done` または `backlog fail` の実行漏れを確認する。実行済みなのに繰り返す場合はエラーとして報告し終了する。
- **途中でセッションが切れた** → 次回 `/backlog` 起動時に Step 1 でロックステータスを確認し、自分の stale ロックなら再取得して続きから再開する。
