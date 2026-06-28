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
| blastguard | PreToolUse フックで Bash/ファイル操作中の破壊的操作（再帰 rm・git reset --hard・全消し上書き等）を deny する。リポの設定ファイルは除外 |
| budgetguard | Stop フックでセッション/日次コスト上限を監視し超過時に制御する |
| compass | 北極星ゴール設定・次の一手導出を行う condukt の上流エージェント |
| condukt | インタープリタ/ワーカー/ベリファイア/リサーチャーの 4 エージェント + Rust バイナリによる決定論オーケストレーションエンジン |
| ctxrot | 注入ルール・救済・蒸留・キャリーオーバー制御でコンテキスト劣化をガードする |
| curate | fugu-router playbook を evalkit の versioned golden 評価データセットへ昇格する。機械的な done_criteria（`cargo test` 等）は実行可能な golden に、それ以外は人手補完用 draft に変換し `evals/curated/*.jsonl` に固定。オフライン評価ループの供給側 |
| daily | SessionStart フックで `cargo deny` セキュリティ監査を 1 日 1 回だけ実行し所見を注入する |
| deepwiki | リポジトリ構造マップから `.deepwiki/*.md` アーキテクチャ wiki を自動生成する |
| difflog | SessionStart でスナップショットを取り SessionEnd で git diff サマリを記録する |
| donegate | Stop フックで受け入れコマンドを実行し全 green まで完了を阻止する |
| evalkit | golden `*.jsonl`（ファイル内容 / CLI 出力アサーション）を実行し、プロンプト改変や CLI 契約の劣化で非ゼロ終了するオフライン回帰評価ハーネス。condukt のオンライン verifier のオフライン姉妹で、CI ゲート（`eval.yml`）として動く |
| flow | source（compass の一手 / backlog キュー / hypothesis の計測待ち仮説）→ executor（condukt）を 1 本のループで束ねる統合 driver。出荷済み仮説を計測して validate/reject する measure step を含む。SessionStart で開いている仕事があれば `/flow` を提案する |
| fugu-router | 過去タスクの検証実績から最安 Claude ティアを選ぶ per-model ルーター |
| gauge | Stop フックでトークン使用・コスト・ツール呼び出し数をローカルに計測する |
| harness-status | budgetguard・gauge・taskprog を束ねたコスト/進捗ダッシュボード |
| hypothesis | PDO 仮説ライフサイクル管理。仮説の作成・検証・棄却・compass ゴール紐づけと、出荷済み・未計測（awaiting-measurement）の追跡を行う（出荷 ≠ 検証） |
| playbook | UserPromptSubmit フックで関連アトミックノートをコンテキストに注入する |
| precommit-audit | Stop フックで設定ルールと diff を照合し問題がなくなるまで完了を阻止する |
| reviewgate | Stop フックで diff をセルフレビュー/独立レビューし合格まで完了を阻止する |
| run-book | UserPromptSubmit フックでプロンプト中の `!name` マクロを手順に展開する |
| schemaguard | source→executor 境界で LLM の構造化出力（condukt 分解 / fugu episode・playbook / scout 施策）を宣言 schema で検証し、違反時は構造化エラーで 1 回 re-ask、reject 件数をメトリクス計上して silent drop を観測可能にするゲート |
| scout | プロジェクトを5レンズ（課題/セキュリティ/業界標準/不足施策/安全性）で並列監査し施策を生成、backlog に積んで /flow へ引き渡す SOURCE |
| session-insights | ツール呼び出し・ターン数・ファイル数を集計し Obsidian vault に記録。クロスセッション backlog 管理も担う |
| specguard | 仕様と実装の整合を監査する read-only ハーネス |
| stuckguard | PostToolUse フックで繰り返し操作・編集スラッシュを検知しエスカレーションする |
| taskprog | `.claude/progress.md` をセッション間で同期し HOTL ハンドオフを支援する |
| tdd | Stop フックでテストなし実装を阻止するテストファースト・ゲート |
| tracekit | condukt run の interpreter→worker→verifier を親子リンクした span 木（phase/model/ms/cost/status）として記録し、`tracekit trace <RID>` で描画、OTel GenAI semconv JSON を export するトレーサ。失敗 run のどの段が遅い/高い/落ちたかを可視化（file-only・ネットワーク無し） |
| trajectoryeval | エージェントの tool-call 経路を検証する trajectory-match verifier。condukt の verifier が出力（done_criteria）を見るのに対し、worker が辿った経路を見る。`check` が実 tool 列を期待軌跡（strict/unordered/subsequence）と照合し {pass, missing, unexpected, out_of_order} を 0/1/2 ゲート終了で返し、`extract` が transcript をストリームして順序付き tool_use 名に変換。done-criteria 検証の経路面の姉妹（agentevals 流） |

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
