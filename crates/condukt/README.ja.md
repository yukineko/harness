# condukt

Claude Code 向けの**決定論的オーケストレーションエンジン**。

大きなタスクは多数の小さなタスクに分解されます。リクエストを解釈し、各ピースを実装し、検証するという判断は LLM の仕事です。しかし、*どのタスクを並列実行できるか*の決定、*git ワークツリーの管理*、*実行状態の追跡*、そして*本当に完了したかどうかの判断*は、言語モデルの目視確認に頼るべきではありません。condukt はこの二つを分離します。

```
LLM  (the /condukt skill + interpreter/worker/verifier agents)
  ├ リクエストを解釈する          ─┐
  ├ タスクへ分解する (JSON)         │   condukt バイナリ（決定論的）
  ├ 各タスクを実装する             ├──▶ スケジューリング: 競合分析 → 並列/直列バッチ
  └ 基準に照らして検証する          │    ワークツリー: 作成 / マージ / 削除 / クリーンアップ
                                  ─┘    状態管理: 実行追跡 + 完了ゲート
```

バイナリは単一の Rust 実行ファイルで、ジョブごとに 1 つのサブコマンドを公開します。**サブスクリプションネイティブ**な設計のため、プラグインユーザーは `ANTHROPIC_API_KEY` も追加インストールも不要です。処理はスキル、3 つのエージェント、1 つの SessionStart フックを介して Claude Code の中で実行されます。

## エンジンが行うこと

| サブコマンド | 目的 |
|---|---|
| `condukt schedule` | 分解 JSON を読み込み、順序付けられた並列バッチと直列/ゲートリストを出力する。2 つのタスクが同一バッチに入るのは、`touched_files` が競合せず、かつ互いに依存関係がない場合のみ。 |
| `condukt validate` | 分解 JSON を検証する（一意な ID、既知の依存関係、循環なし）。 |
| `condukt worktree create/merge/remove/cleanup/list` | git ワークツリーのライフサイクル管理。「リポジトリ外のパス」と「1 ディレクトリ = 1 ブランチ」を強制する。 |
| `condukt state init/set/show/gate/list` | 実行中のタスクステータスを永続化する。`gate` はすべてのタスクが検証済みで、ダーティなワークツリーや未削除のワークツリーがなくなるまで非ゼロで終了する。 |
| `condukt state stats` | すべての実行（完了・未完了）を集計する: 完了率、タスク数、ステータス分布。ビフォーアフターのベンチマークとして有用。 |
| `condukt state reconcile --run <id> [--dry-run]` | 対象ブランチがデフォルトブランチにマージ済み、またはワークツリーごと削除済みのタスクを自動的に `verified` へ昇格させる。手動で `state set` を呼ばずに、セッションクラッシュ後の古い状態を修正できる。 |
| `condukt state resume-context --run <id>` | 停止した実行をセッションをまたいで再開するために、保留中/失敗/完了タスクを JSON として出力する（スキルの Phase 0-alt を参照）。 |
| `condukt state test` | リポジトリルートからプロジェクトのテストスイートを実行する（`cargo test` / `npm test` / `pytest` を自動検出、または設定の `[test].command` を使用）。 |
| `condukt restore` | SessionStart フック: 未完了の実行や孤立したワークツリーを通知する。 |
| `condukt statusline` | `statusLine` 設定用の 1 行実行進捗表示。 |
| `condukt init / install / uninstall` | `~/.condukt` を作成し、手動でフックを設定する（プラグインユーザーは不要）。 |

インタープリターエージェントが出力し、`schedule` が消費する分解スキーマ:

```json
{ "goal": "...", "tasks": [
  { "id": "t1", "title": "...", "touched_files": ["path/or/glob"],
    "deps": ["t0"], "class": "parallel|serial|gated",
    "suggested_model": "sonnet|opus|haiku", "done_criteria": "observable pass condition" }
]}
```

内部の仕組みの詳細（Phase 0〜8 など）については `docs/internals.ja.md` を参照してください。

## インストール

### プラグイン（推奨）

> マーケットプレイスカタログは別の中央リポジトリにあります。condukt が公開されたら、インストールは次の通りです:

```
/plugin marketplace add <git-url-of-the-catalog-repo>
/plugin install condukt@yukineko
```

これにより、`/condukt` スキル、3 つのエージェント、SessionStart フック、ビルド済みバイナリがバンドルされます。`condukt init` を一度実行して `~/.condukt` とデフォルトの `config.toml` を作成することもできます。

### 手動（ソースからビルド）

```
cargo build --release
cp target/release/condukt ~/.cargo/bin/      # または PATH の通った場所
condukt init
condukt install --dry-run                    # settings.json の変更をプレビュー
condukt install                              # SessionStart フックをマージ（settings.json をバックアップ）
cp -r skills/condukt ~/.claude/skills/        # agents/ も ~/.claude/agents/ へ
```

削除するには `condukt uninstall` を実行します。

## 設定

`~/.condukt/config.toml`（デフォルト値を表示）:

```toml
worktree_base  = "~/.condukt/worktrees"  # リポジトリの外でなければならない
default_branch = "main"
max_parallel   = 4                        # 同時ワーカー数のアドバイザリーソフトキャップ
shared_globs   = []                       # このグロブに触れるタスクを強制的に直列実行させる

# `condukt state test` が実行するコマンド（`sh -c` 経由、リポジトリルートから）。
# 省略すると自動検出（cargo test / npm test / pytest）。
# [test]
# command = "cargo test"
```

`shared_globs` は、何もハードコードせずにプロジェクト全体のファイルをワーカーから保護する仕組みです。例: `["**/models.py", "**/migrations/**", "docs/glossary.md"]`。これに触れる並列タスクは警告とともに直列実行に降格されます。

### 環境変数

設定ファイルのキーはすべて実行時に環境変数で上書きできます。`CONDUKT_DISABLE` はフック専用のキルスイッチであり、設定ファイルには対応する項目がありません。

| 変数 | デフォルト | 説明 |
|---|---|---|
| `CONDUKT_WORKTREE_BASE` | `~/.condukt/worktrees` | ワークツリーを作成するディレクトリ（リポジトリの外である必要がある）。 |
| `CONDUKT_DEFAULT_BRANCH` | `main` | 完了した作業をマージするブランチ。 |
| `CONDUKT_MAX_PARALLEL` | `4` | 同時ワーカー数のアドバイザリーソフトキャップ。 |
| `CONDUKT_DISABLE` | _（未設定）_ | `1` に設定すると、SessionStart/statusline フックを無効化する（CI で有用）。 |

### `condukt loop` — テスト修正サイクル

指定されたモジュールタイプに対してテスト修正サイクルの 1 イテレーションを実行し、JSON 結果を出力します。`/condukt-loop` スキルはすべてのテストが通るか、進捗がなくなるまで、修正ステップを挟みながらこれを繰り返し呼び出します。

```
condukt loop --module <server|client|e2e> [--iteration N] [--prev-failures N]
```

**サイクルシーケンス**（`config.toml` の `[loop]` で設定）:

| `--module` | ステップ |
|---|---|
| `server` | deploy → test |
| `client` | build → test |
| `e2e` | build → deploy → test |

**JSON 出力**（呼び出しごとに 1 オブジェクト）:

```json
{
  "iteration": 1,
  "module": "client",
  "failure_count": 3,
  "success": false,
  "stop": false,
  "stop_reason": "",
  "output": "<combined stdout+stderr>"
}
```

`stop=true` になるのは、`failure_count == 0`（`stop_reason: "all tests pass"`）または `failure_count == prev_failures`（`stop_reason: "no progress: failure count unchanged"`）の場合です。

**設定:**

```toml
[loop]
build_command  = "npm run build"
deploy_command = "kubectl rollout restart deployment/api && kubectl rollout status deployment/api"
max_iters      = 10   # 安全キャップ; スキルが強制する
```

### `condukt state test`

リポジトリルートからプロジェクトのテストスイートを実行し、その終了コードを伝播します。

```
condukt state test --run <run-id>
```

コマンドの解決優先順位:

1. `~/.condukt/config.toml` の `[test].command`
2. リポジトリルートから自動検出: `cargo test`（Cargo.toml）、`npm test`（package.json）、`pytest`（pyproject.toml / setup.py）、それ以外は `cargo test` にフォールバック。

コマンドは `sh -c` 経由で実行されるため、クォート付き引数、パイプ、環境変数展開がすべて機能します（例: `command = "pytest -k 'unit or smoke'"`）。呼び出し元の cwd ではなくリポジトリルートから実行されるため、自動検出は呼び出し元がサブディレクトリにいる場合でも常にプロジェクトマニフェストを参照します。

## 制約

- **マシンごとのマーケットプレイス登録。** 各ユーザーは `/plugin marketplace add <url>` を一度実行する必要があります。Claude Code はチェックインされたリポジトリからマーケットプレイスを自動登録しません。
- **プラットフォームごとのバイナリ。** Linux x86_64 は `bin/` にコミットされています。macOS arm64 / x86_64 は GitHub Actions の macOS ランナーでビルドされます（Linux からは Apple SDK でクロスビルドできないため）。ホストに対応するバイナリがない場合、ランチャーはビルドのヒントを表示して 0 で終了するため、フックがターンを壊すことはありません。
- **実行ビット。** バイナリとランチャーは git インデックス内で実行ビットを保持する必要があります（`git update-index --chmod=+x bin/condukt bin/condukt-*`）。リポジトリが `core.filemode=false` のマウント上でチェックアウトされることが多いためです。

## 開発

```
cargo test          # ユニットテスト（スケジューリング、ゲート、プロジェクトキー）
cargo clippy --all-targets
scripts/build-plugin-bin.sh        # ホスト用の bin/condukt-<os>-<arch> をステージング
```

### 真実のソース: キャッシュではなくリポジトリを編集する

`crates/condukt/`（このディレクトリ）が**唯一の真実のソース**です。`/plugin install` は `~/.claude/plugins/cache/<owner>/condukt/<version>/` にプレーンコピー（`.git` なし）としてコピーし、実行中の `/condukt` スキルはそこからエージェントと `SKILL.md` を読み込みます。condukt 自身を改善するために condukt を使う際に、そのキャッシュコピーを誤って編集してしまいがちですが、そうすると git の外に変更が残り、リポジトリと静かに乖離してしまいます。

ルール: **キャッシュは絶対に手動編集しない。** ここのファイルを編集してから、ローカルインストールを更新してください。condukt が**自身の**プラグインへの変更をオーケストレーションする場合は、ワーカーをこのリポジトリ（その git ワークツリー）に向け、キャッシュパスには向けないでください。

```
scripts/sync-plugin-assets.sh           # リポジトリ -> キャッシュ: ローカルインストールを更新
scripts/sync-plugin-assets.sh --check   # ドリフトを報告; キャッシュ != リポジトリなら終了コード 1
```

コミット前に `--check` を実行（またはプレプッシュフックに組み込む）して、キャッシュがリポジトリからドリフトしていないか、またはキャッシュで作成されたがコミットされていない新しいエージェント/スキルファイルがないかを確認してください。

## ライセンス

MIT
