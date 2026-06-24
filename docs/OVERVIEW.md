# harness — 全体概要

Claude Code 上で動く自作プラグイン群のモノレポ。「判断は LLM、決定論的な作業は Rust バイナリ」という分業を軸に、長いエージェントセッションを安全・継続的に動かすための基盤を提供する。

---

## なぜこのハーネスが存在するか

素の Claude Code は 1 セッション完結で動く。長いセッションでは:

- コンテキストが膨張して初期の指示や決定が埋もれる（context rot）
- 並列実装でファイルが競合する
- LLM が同じ失敗を繰り返す
- コストが見えない
- セッションをまたいで作業が引き継がれない

harness はこれらを **Claude Code の hooks と Rust バイナリと skill** の組み合わせで解決する。外部 API キー不要、Claude subscription の範囲内で完結する。

---

## 3 つの設計原則

**1. LLM と決定論の分離**  
LLM は「解釈・実装・判断」だけを担う。スケジューリング・状態管理・ゲート・worktree 管理は Rust バイナリが決定論的に処理する。LLM の非決定性が副作用を持つ範囲を最小化する。

**2. フック不変条件（never break a turn）**  
`harness-core::hook::run_hook` がパニックを捕捉して常に exit 0 で返す。どのプラグインのバグも Claude Code のターンを壊さない、という不変条件がコアライブラリに焼き込まれている。

**3. Subscription-native**  
外部 API キーを不要とし、既存の transcript / git diff / ローカルファイルを読む。メモリ上限を守るためトランスクリプトのフル読み込みは禁止し、常にストリーミング処理する。

---

## 共通基盤 — harness-core

全プラグインがビルド時に依存する共有ライブラリ。バイナリに静的リンクされるためランタイム依存はない。

| モジュール | 役割 |
|---|---|
| `hook.rs` | `HookInput` パース、`run_hook` 不変ラッパー |
| `store.rs` | 並列セッション安全な note ストア（session-tag 付きファイル名、fallback ロジック、prune/GC） |
| `transcript.rs` | JSONL トランスクリプトのストリーミング読み込み（全ロード禁止） |
| `pricing.rs` | Opus/Sonnet/Haiku/Fable の USD コスト推定テーブル |
| `install.rs` | `~/.claude/settings.json` の load/backup/write |
| `interrogate.rs` | ゴール/仕様精緻化の問答ループ（純粋関数） |

---

## プラグイン一覧

### オーケストレーション

#### condukt
課題を「解釈→タスク分割→合意→並列実装→検証→完了ゲート」の 1 サイクルで回す。

- **LLM が担う**: 解釈（interpreter agent）・実装（worker agent）・検証（verifier agent）
- **バイナリが担う**: ファイル衝突解析・parallel/serial スケジュール・git worktree 管理・state 追跡・完了ゲート
- **スキル**: `/condukt <課題>`
- **フック**: なし（スキルから呼ぶ）

#### fugu-router
過去のタスク実績（episodes.jsonl）から k-NN で「最安で検証を通過できるモデル」を選び、condukt の `suggested_model` を上書きする。haiku < sonnet < opus のティア順で最安を選ぶ。Thompson sampling で未実績ティアも探索する。

- **スキル**: `/fugu-router`（condukt 内から自動呼び出し）
- **フック**: UserPromptSubmit（ルーティング記憶のサマリー注入）

#### compass
「今何をすべきか」を決める上流コンポーネント。北極星ゴールを彫り直し、現状との gap から condukt へ渡す「右サイズの一手」を選ぶ。

- **スキル**: `/compass`
- **フック**: SessionStart（目標リマインド注入）、Stop（breadcrumb 更新）

---

### コンテキスト管理

#### ctxrot
長いセッションで context が膨張・腐敗するのを防ぐ。5 つのサブコマンドが異なるタイミングで動く。

| サブコマンド | フック | 役割 |
|---|---|---|
| `guard` | UserPromptSubmit | 大きなファイル参照・context 使用率バンドを検出し警告注入 |
| `rescue` | PreCompact | `/compact` 直前に決定事項・残課題・触ったファイルを markdown ノートに保存 |
| `restore` | SessionStart | 前セッションのノートから決定事項・残課題を抽出して注入 |
| `preguard` | PreToolUse | 1MB 超の Read を禁止（sub-agent 経由を促す） |
| `toolguard` | PostToolUse | 巨大なツール出力を検知して警告 |

- **スキル**: `/distill`（能動蒸留）、`/ctx`（load/pin/unload/use-note/list）
- ノートは `~/.ctxrot/store/` に Obsidian 互換 markdown として保存。`store_dir` を vault に向ければそのまま閲覧できる

#### playbook
プロジェクト固有の知識（`.playbook/*.md`）をプロンプトごとにキーワードスコアリングで関連ノートを自動注入する。

- **フック**: UserPromptSubmit

#### run-book
プロンプト中の `!name` を `.runbook/<name>.md` の内容に展開して注入する。繰り返し使う手順をマクロ化する。

- **フック**: UserPromptSubmit

#### taskprog
`.claude/progress.md` をセッション間でつなぎ、複数セッション跨ぎの進捗を管理する。

- **フック**: SessionStart（progress.md 注入）、Stop（更新）
- **スキル**: `/taskprog`

---

### 検証ゲート（Stop フック群）

Stop フックはエージェントがターンを終えようとする前に実行される。FAIL なら停止を延長して修正を促す。

#### donegate
`build / test / lint` コマンドが全 green になるまで Stop をブロックする。設定したコマンドが失敗すると Claude に修正を求めて再試行させる。

#### reviewgate
コードレビューゲート。inject モード（Claude が自己レビュー）または subprocess モード（独立レビュアーを別プロセスで起動）で差分を評価し、基準を満たさなければ Stop をブロック。

#### precommit-audit
差分に対して静的監査を実行する。シークレット混入・例外の握りつぶし・禁止 API・巨大ファイル追加などを検査。

#### tdd
テストファースト強制ゲート。実装変更にテストが伴わない Stop をブロックする。`red/green/verify` のサブコマンドで TDD ライフサイクル（RED → GREEN → VERIFY）を証明させる。

- **スキル**: `/tdd`

#### budgetguard
セッション・日次のコスト上限を設定し、超過すると Stop をブロックする。gauge の記録を読んでコストを計算する。

---

### 仕様管理

#### specguard
仕様（正典ドキュメント）と実装のドリフトを監査する。shard に分割して read-only subagent が逐語引用つきで差異を報告する。

- **スキル**: `/specguard:run`（監査実行）、`/specguard:brief`（着手前ブリーフィング）、`/specguard:decide`（ADR 記録）など

---

### 観測・通知

#### gauge
ローカル LLMOps テレメトリ。トークン・キャッシュ率・ツールコール・コストを JSONL に記録する。他プラグイン（budgetguard・harness-status・session-insights）の数値源。

- **フック**: Stop（セッション記録）

#### session-insights
セッション作業メトリクス（ターン数・ツールコール・ファイル数・サイズクラス・カテゴリ）を集計し、Obsidian vault へ書き出す。

- **フック**: PostToolUse（逐次カウント）、Stop（Obsidian 書き出し）、SessionEnd（record note 更新）
- **スキル**: `/session-insights:record`

#### difflog
SessionStart で HEAD をスナップショットし、SessionEnd で `git diff` の構造化ログを書き出す。

- **スキル**: `/difflog`

#### beacon
Stop と Notification フックでデスクトップ通知・Slack・webhook に通知する。長いエージェント実行が終わったときに外から気付ける。

- **フック**: Stop、Notification

#### stuckguard
同一ツールコールの反復や編集のオシレーションを PostToolUse で監視し、スタックを検知したら Claude にエスカレーション注入する。

- **フック**: PostToolUse

#### deepwiki
Rust スキャナーがリポジトリ構造をマップし、`.deepwiki/*.md` にアーキテクチャ wiki を生成・鮮度追跡する。

- **スキル**: `/deepwiki`

#### harness-status
budgetguard の支出台帳 + gauge のセッション記録 + taskprog の progress.md を 1 画面に集約表示する。hooks なし、read-only。

- **スキル**: `/harness-status:status`

---

## プラグイン間の連携図

```
[上流]
  compass（目標再接地）
      ↓ 一手の課題文
  condukt（実行の背骨）
      ├─ [前段] playbook / run-book / ctxrot / taskprog が context を整備
      ├─ [Phase 5 並列実装] fugu-router がモデルを選ぶ
      │    stuckguard / ctxrot / budgetguard / gauge が並走監視
      ├─ [Phase 6 検証] donegate / tdd / precommit-audit / reviewgate / specguard
      └─ [Phase 7 完了] beacon で通知
[横断]
  gauge → budgetguard, harness-status, session-insights が数値を読む
  harness-status → budgetguard + gauge + taskprog の集約ビュー
```

---

## フック発火タイミング早見表

| Hook | 発火タイミング | 主な登録プラグイン |
|---|---|---|
| SessionStart | セッション開始時 | ctxrot(restore), condukt(restore), compass, difflog, specguard, taskprog |
| UserPromptSubmit | プロンプト送信前 | ctxrot(guard), fugu-router, playbook, run-book |
| PreToolUse | ツール実行前 | ctxrot(preguard) |
| PostToolUse | ツール実行後 | ctxrot(toolguard), session-insights, stuckguard |
| PreCompact | /compact 直前 | ctxrot(rescue) |
| Stop | ターン終了前 | beacon, budgetguard, compass, donegate, gauge, precommit-audit, reviewgate, session-insights, taskprog, tdd |
| SessionEnd | セッション終了時 | difflog, session-insights |
| Notification | 外部通知時 | beacon |

---

## インストール方法

```
/plugin marketplace add yukineko/harness
/plugin install <plugin>@yukineko   # 必要なプラグインを個別に
```

各プラグインは `crates/<plugin>/` にあり、`.claude-plugin/plugin.json` で管理されている。バージョンが変わったプラグインだけ bump する（全プラグイン一斉更新は不要）。

---

## 開発

```sh
cargo build --workspace
cargo test  --workspace
claude plugin validate crates/<plugin>
```

各プラグインの詳細は `crates/<plugin>/README.md` を参照。
