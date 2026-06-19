# specguard

> 🌐 [English](README.md) ・ **日本語**

**仕様↔実装 整合監査ハーネス (project-agnostic)**

実装が「正典 (canonical spec)」からずれていないか、また正典 doc 自体に沈黙・矛盾・
重複がないかを、LLM エージェントに **read-only** で監査させる CLI です。判定の中核は
LLM が逐語引用つきで行い、specguard はその周りの **決定的なハーネス**
(スコープ決定 → プロンプト描画 → エージェント起動 → marker 解析 → レポート/sentinel)
を担います。プロジェクト固有部分はすべて設定ファイル (TOML) に外出しされています。

```
specguard.toml ──┐
   git diff ──────┼──▶ scope (変更領域 ∪ 不変条件)
                  │         │
templates/ ───────┼──▶ プロンプト描画 ──▶ agent (read-only) ──▶ marker 解析 ──▶ report + sentinel
```

実行方式は 2 つあり、**同じ `specguard` バイナリ**を共有します:

| | standalone バイナリ | Claude Code プラグイン |
|---|---|---|
| 監査エンジン | `claude --print` を shard ごとに spawn | session 内 read-only subagent (nested claude なし) |
| 課金 | claude CLI の login 依存 | **ホストセッションの subscription** |
| read-only 強制 | `claude --print` の **Bash 引数 allowlist** (強い) | subagent の **ツール名** 制限 (やや弱い) |
| 起動 | `specguard run` (cron 等) | `/specguard:run` (対話/HOTL) |

→ 設計と不変条件の詳細は **[DESIGN.md](DESIGN.md)** / **[DESIGN-VERIFY.md](DESIGN-VERIFY.md)**、
監査ポリシー (分類・verdict 語彙・規律) の正典は `templates/audit-prompt.md` です。

---

## 導入手順

### 前提

- Rust toolchain (`cargo`)。無ければ https://rustup.rs から。
- 監査対象は **git リポジトリ**であること。
- いずれの方式でも対象リポジトリに `specguard.toml` が必要 (下記 scaffold)。

### 1. バイナリを入れる (両方式の共通前提)

```sh
./install.sh                                  # release ビルドして ~/.local/bin へ
SPECGUARD_BIN_DIR=/usr/local/bin ./install.sh # 配置先を変えるなら
```

手動なら `cargo build --release` で `target/release/specguard` が生成されます。
`~/.local/bin` が PATH 上にあることを確認してください。詳細・トラブルシュートは
**[INSTALL.md](INSTALL.ja.md)**。

### 2. 対象リポジトリに scaffold

```sh
cd /path/to/your/repo
specguard init        # specguard.toml と SessionStart hook を生成 (冪等)
```

`init` は既存の `specguard.toml` を `--force` 無しでは上書きせず、`.claude/settings.json`
の他設定を壊さず SessionStart hook (未処理ドリフトの提示) だけを足します。
**プラグイン方式では hook は同梱済み**なので、config だけ用意すれば足ります
(`cp specguard.example.toml specguard.toml` でも可)。

### 3a. standalone で使う

```sh
cd /path/to/your/repo
# specguard.toml の [[area]] / [[invariant]] / canon を対象に合わせて編集
specguard run                                 # 監査を実行
```

cron / タスクスケジューラから `specguard run` を回し、`needs_user=yes` のとき立つ
sentinel を SessionStart hook が検知して人間に促す、という Human-on-the-loop に
組み込めます。

### 3b. Claude Code プラグインとして使う (subscription-native)

このリポジトリ自体がプラグインです。`claude --print` を起動せず、各 shard を
session 内の read-only subagent (`specguard-auditor`) に委譲し、ホストセッションの
subscription で監査します。決定的ハーネスは同じ `specguard` バイナリに委譲します
(判定ロジックの二重化なし)。

```sh
cd /path/to/your/repo
claude --plugin-dir /path/to/specguard        # このセッションだけ読み込む
# 変更後は /reload-plugins、確認は /plugin
```

```
/specguard:run
  └─ specguard prompt --json    (ハーネス: scope 解決 + shard 描画)
  └─ Task(specguard-auditor) × shard   (判定: read-only subagent / subscription)
  └─ specguard ingest --from …  (ハーネス: parse → verify → report → sentinel/baseline)
```

---

## 使い方

`needs_user=yes` の指摘に人間が対応したら sentinel を消します (しないと SessionStart
hook が同じ件で促し続ける)。

### スラッシュコマンド (プラグイン)

| コマンド | 対応バイナリ | 説明 |
|---|---|---|
| `/specguard:run [--baseline <ref>]` | `prompt --json` + subagent + `ingest` | subscription-native 監査 |
| `/specguard:scope` | `scope` | 解決済みスコープを表示 (agent 呼ばない) |
| `/specguard:ack` | `ack` | 対応済み sentinel をクリア |
| `/specguard:accept-prompt <理由>` | `accept-prompt` | prompt(メタ正典)を批准して pin |
| `/specguard:decide <タイトル>` | `decide` | 決定ログ(ADR)を canon commit に pin して生成 |

### サブコマンド (バイナリ)

```sh
specguard run                      # 監査を実行 (claude --print を shard ごとに spawn)
specguard scope                    # 解決されたスコープだけ表示 (agent 呼ばない)
specguard prompt                   # 各 shard のプロンプトを表示 (agent 呼ばない)
specguard prompt --json            # shard を機械可読 JSON で出力 (プラグインが使う)
specguard ingest [--from <file>]   # 収集済み shard 出力(JSON/stdin)を食わせて
                                   #   parse→report→sentinel (agent を spawn しない)
specguard ack                      # 対応済みの sentinel をクリア
specguard decide "<title>"         # 決定ログ(ADR)を生成
specguard accept-prompt -m "理由"  # prompt(メタ正典)を批准
specguard --baseline HEAD~5 run    # baseline を上書き
specguard --config examples/aegis.toml run
```

### 出力

| パス | 内容 |
|---|---|
| `<report_dir>/<date>.md` | レポート本体 |
| `<report_dir>/.last-ref` | 最後に監査した HEAD (次回の change-triggered baseline) |
| `<sentinel>` | `needs_user=yes` のときだけ (date / report / summary) |

baseline は **ack 連動で前進**します。クリーンに監査できた回だけ `.last-ref` を HEAD へ
進め、指摘が残っている間は据え置く (未修正 drift が次回の diff から外れて検出漏れに
なるのを防ぐ)。

---

## 設定 (TOML)

`specguard.example.toml` に全項目のコメント付きサンプルがあります。要点:

- `[project]` … `name`, `root` (リポジトリルート)
- `[agent]` … `command` + `args`。既定は `claude --print` (read-only allowlist 付き)。
  任意のエージェント CLI に差し替え可 (プロンプトを stdin、レポートを stdout)
- `[scope]` … `baseline_ref` / `fallback_ref` (両方解決不能なら全 tracked file を監査)
- `[output]` … `report_dir` / `sentinel`
- `[prompt]` … `template` (省略時は埋め込み既定) / `require_ratification` (批准ゲート)
- `[[area]]` (複数) … `name` / `globs` / `canon`。**globs にマッチする変更があれば in-scope**
- `[[invariant]]` (複数) … `name` / `description` / `canon`。**毎回チェック**
- `[verify]` … 検証ゲート (既定 OFF)。`enabled` = 反証 (偽陽性除去) / `completeness` =
  網羅性批評 (偽陰性発掘)。**両方併用を推奨**。詳細は [DESIGN-VERIFY.md](DESIGN-VERIFY.md)
- `[decisions]` … 決定ログ (ADR) の鮮度・陳腐化照合を有効化

AEGIS の元実装を再現する設定例は `examples/aegis.toml`。

### 監査の 3 次元 (概要)

正典は監査プロンプト (`templates/audit-prompt.md` / `decisions-prompt.md`)。ここでは概要のみ:

- **D1 実装↔正典 drift**: 実装が正典からずれていないか (矛盾は誤読/コード違反/正典陳腐化に分類)。
- **D2 仕様品質**: 正典 doc 自体の沈黙・矛盾・重複。
- **D3 決定ログ鮮度・陳腐化**: 仕様変更の *理由* を canon commit に pin し、決定が今も成立するか照合 (`[decisions]` で有効化)。

---

## 終了コード

| code | 意味 |
|---|---|
| 0 | 正常終了 |
| 2 | 設定 / 使用法エラー |
| 3 | いずれかの shard の出力に marker が無い (レポートは保存。baseline 前進・sentinel はしない) |
| 4 | いずれかの shard のエージェントが非ゼロ終了 (真の終了コードは stderr) |
| 5 | prompt(メタ正典)が未批准/変更あり (`require_ratification` 有効時)。`accept-prompt` が必要 |

一次情報は `src/main.rs` の `EXIT_*` 定数 (この表が唯一の doc 表)。エージェント由来の
終了コードはそのまま伝播せず、常に `4` に集約し各 shard の実コードを stderr に出します。

---

## read-only の保証について

- **standalone**: 既定エージェントは allowlist (Read/Grep/Glob + `git diff/log/show/status`)
  で起動し、書き込み・ネットワーク・任意 shell を deny。`--print` モードでは allowlist
  外のツールは自動 deny されるため、監査対象リポジトリ由来の prompt injection でも破壊的
  コマンドは成功しません。プロンプトの「お願い」ではなく **権限** で担保します。
- **プラグイン**: subagent の保証は **ツール名レベル** (Edit/Write/NotebookEdit/WebFetch/
  WebSearch を剥奪 + 読み取り専用 git のプロンプト規律)。Claude Code の subagent 定義は
  Bash の *引数* allowlist (`Bash(git diff *)`) を持てないため、standalone より prompt-
  injection 耐性は弱い。強制の強さを最優先したい対象では standalone `specguard run` を選ぶ。
- 検証ゲート (`[verify]`) ON 時、`ingest` 内の反証/網羅性批評は従来どおりバイナリが agent を
  spawn します (= プラグイン経由でも nested claude が走る)。完全 native 化は今後の課題。

---

## テスト

```sh
cargo test          # unit (parse/scope/prompt/report) + integration (fake agent)
```

統合テストは `bash -c` の擬似エージェントを使うので実 LLM は不要です。

## ライセンス

MIT
