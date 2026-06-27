# harness

Cargo ワークスペース・モノレポ。`yukineko` の Claude Code ハーネス一家を単一ソースで管理する。

- 共通基盤: `crates/harness-core`（ビルド時依存。各プラグインのバイナリに静的に焼き込まれる）
- 各プラグイン: `crates/<plugin>/` — Rust クレートかつ Claude Code プラグイン（`.claude-plugin/plugin.json` + `hooks/` + 同梱 `bin/`）

配布はこの repo 自身で完結する。リポ root の `.claude-plugin/marketplace.json` が marketplace カタログで、各プラグインを `git-subdir`（`yukineko/harness.git` の `crates/<plugin>`、`ref=main`）で指す。利用側は `/plugin marketplace add yukineko/harness` → `/plugin install <plugin>@yukineko`。別リポへの切り出しは行わない。

## プラグイン一覧

| プラグイン | 説明 |
|---|---|
| autoflow | Stop フックで `/record` と `/condukt` を自動ループするセッション終了オートフローゲート |
| backlog | SessionStart フックで未完了タスクを通知するクロスプロジェクト・タスクキュー |
| beacon | Stop/Notification フックでデスクトップ・Slack 通知を送る |
| budgetguard | Stop フックでセッション/日次コスト上限を監視し超過時に制御する |
| compass | 北極星ゴール設定・次の一手導出を行う condukt の上流エージェント |
| condukt | インタープリタ/ワーカー/ベリファイア/リサーチャーの 4 エージェント + Rust バイナリによる決定論オーケストレーションエンジン |
| ctxrot | 注入ルール・救済・蒸留・キャリーオーバー制御でコンテキスト劣化をガードする |
| daily | SessionStart フックで `cargo deny` セキュリティ監査を 1 日 1 回だけ実行し所見を注入する |
| deepwiki | リポジトリ構造マップから `.deepwiki/*.md` アーキテクチャ wiki を自動生成する |
| difflog | SessionStart でスナップショットを取り SessionEnd で git diff サマリを記録する |
| donegate | Stop フックで受け入れコマンドを実行し全 green まで完了を阻止する |
| flow | source（compass の一手 / backlog キュー）→ executor（condukt）を 1 本のループで束ねる統合 driver。SessionStart で開いている仕事があれば `/flow` を提案する |
| fugu-router | 過去タスクの検証実績から最安 Claude ティアを選ぶ per-model ルーター |
| gauge | Stop フックでトークン使用・コスト・ツール呼び出し数をローカルに計測する |
| harness-status | budgetguard・gauge・taskprog を束ねたコスト/進捗ダッシュボード |
| hypothesis | PDO 仮説ライフサイクル管理。仮説の作成・検証・棄却・compass ゴール紐づけを行う |
| playbook | UserPromptSubmit フックで関連アトミックノートをコンテキストに注入する |
| precommit-audit | Stop フックで設定ルールと diff を照合し問題がなくなるまで完了を阻止する |
| reviewgate | Stop フックで diff をセルフレビュー/独立レビューし合格まで完了を阻止する |
| run-book | UserPromptSubmit フックでプロンプト中の `!name` マクロを手順に展開する |
| session-insights | ツール呼び出し・ターン数・ファイル数を集計し Obsidian vault に記録。クロスセッション backlog 管理も担う |
| specguard | 仕様と実装の整合を監査する read-only ハーネス |
| stuckguard | PostToolUse フックで繰り返し操作・編集スラッシュを検知しエスカレーションする |
| taskprog | `.claude/progress.md` をセッション間で同期し HOTL ハンドオフを支援する |
| tdd | Stop フックでテストなし実装を阻止するテストファースト・ゲート |

## 開発

```sh
cargo build --workspace
cargo test  --workspace
```

各プラグインの検証:

```sh
claude plugin validate crates/<plugin>
```

## バージョニング

git-subdir 配布では `version` 省略時に「コミット SHA = バージョン」となり、モノレポでは無関係なコミットで全プラグインが新バージョン扱いになりうる。
**各 `crates/<plugin>/.claude-plugin/plugin.json` に明示 `version` を置き、そのプラグインが変わった時だけ bump する。**
