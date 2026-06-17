# specguard

**仕様↔実装 整合監査ハーネス (project-agnostic)**

実装が「正典 (canonical spec)」からずれていないか、また正典 doc 自体に
沈黙・矛盾・重複がないかを、LLM エージェントに **read-only** で監査させる
CLI です。判定の中核は LLM が逐語引用つきで行い、specguard はその周りの
**決定的なハーネス** — スコープ決定 (git diff) → プロンプト描画 → エージェント
起動 → marker 解析 → レポート / sentinel 出力 — を担います。

もともと AEGIS プロジェクト内で bash + Claude プロンプトとして実装されていた
仕組みを、**プロジェクト固有部分をすべて設定ファイル (TOML) に外出し** して
任意プロジェクトで使えるよう Rust で書き直したものです。

## 設計

```
specguard.toml ──┐
   git diff ──────┼──▶ scope (変更領域 ∪ 不変条件)
                  │         │
                  │         ▼
templates/  ──────┼──▶ プロンプト描画 ──▶ agent (claude --print, read-only)
                  │                              │ stdout
                  │                              ▼
                  └──────────────── marker 解析 ──▶ report.md + .last-ref
                                          │
                                  needs_user=yes なら ──▶ sentinel
```

- **判定は LLM**: エージェントは Read/Grep/Glob + read-only git で **生の正典と実装を
  読み**、逐語引用できないものは `不明` に降格する (hallucination で `矛盾` を捏造しない)。
- **read-only はハーネスで強制**: 既定エージェントは allowlist (Read/Grep/Glob +
  `git diff/log/show/status`) で起動し、書き込み・ネットワーク・任意 shell を deny する。
  `--print` モードでは allowlist 外のツールは自動 deny されるため、監査対象リポジトリの
  内容による prompt injection でも破壊的コマンドは成功しない。プロンプトの「お願い」では
  なく権限で担保する。
- **正典の中身はコピーしない**: 設定には「どこを読むか」(canon ポインタ) だけ書く。
  中身を写すとドリフトの種になるため。
- **shard 並列 (context 分離)**: in-scope 領域ごと + 不変条件を **別プロセス (fresh
  context) で並列監査** し、レポートを統合する。1 セッションに無関係なファイルを溜め込んで
  判定が劣化する (context rot) のを防ぐ。並列度は最大 4 プロセス。
- **change-triggered + invariant**: 毎回フルリポは見ない。baseline 以降に変更が
  あった領域 + 毎回チェックする不変条件だけに絞る。baseline は
  `--baseline`/env → `[scope].baseline_ref` → `.last-ref` → `[scope].fallback_ref`
  (既定 `HEAD~20`) の順で解決し、**どれも解決できなければ最終 fallback として
  全 tracked file を監査** する (`baseline: (all tracked files)`)。若い/浅い repo の
  初回でも hard-error しない。diff は two-dot (`baseline..HEAD`) の **committed 状態**
  を対象とする (未コミットの作業ツリーは見ない)。領域は **glob にマッチする実装変更**
  *または* **canon ポインタのファイルが変更された** とき in-scope になる (仕様だけ
  変わって実装を誰も再照合しない、を防ぐ)。
- **baseline は ack 連動で前進**: クリーンに監査できた回 (`needs_user=no` かつ未処理
  sentinel なし) だけ `.last-ref` を HEAD へ進める。**指摘が残っている間は baseline を
  据え置く** ので、未修正の drift が次回の diff から外れて検出漏れになることがない。
  人間が対応して `specguard ack` するまで同じ範囲を再監査し続ける。
- **provenance**: レポートは監査時の **canon commit (HEAD)** を pin する。過去レポートの
  再現と、B/C 分類 (コードが新か doc が新か) の時間的な接地に使う。

監査の 3 次元:
- **D1 実装↔正典 drift**: 実装が正典からずれていないか。矛盾は `A 誤読 / B コード違反 /
  C 正典が陳腐化 / 判別不能` に分類。
- **D2 仕様品質**: 正典 doc 自体の **沈黙・矛盾・重複**。
- **D3 決定ログ (ADR) の鮮度・陳腐化**: 仕様変更の *理由* を canon commit に pin して
  記録 (`specguard decide`) し、決定が指す canon が今も一致するか (鮮度) と、決定の
  driver/review_when が今も成立するか (陳腐化＝理由より長生きした規則) を照合する。
  決定ログは *証拠* であって権威ではなく、canon が常に正 (`[decisions]` で有効化)。

## インストール

導入手順の詳細は **[INSTALL.md](INSTALL.md)**（前提条件・設定・定期実行・トラブル
シューティング）を参照。最短手順:

```sh
./install.sh                 # release ビルドして ~/.local/bin へ配置
# 配置先を変えるなら:
SPECGUARD_BIN_DIR=/usr/local/bin ./install.sh
```

`install.sh` は `cargo build --release` 後にバイナリを PATH 上へ置くだけ。手動なら
`cargo build --release` で `target/release/specguard` が生成される。

導入後、監査したいリポジトリに **scaffold** する:

```sh
cd /path/to/your/repo
specguard init        # specguard.toml と .claude/ の SessionStart hook を生成
```

`specguard init` は冪等で、既存の `specguard.toml` は `--force` 無しでは上書きせず、
`.claude/settings.json` に他の設定があっても壊さず SessionStart hook だけを足す
(hook は `.specguard-pending` を検知してセッション開始時に未処理ドリフトを提示する)。

## 使い方

1. `specguard init` で生成された `specguard.toml` を編集 (`[[area]]` / `[[invariant]]`
   / `canon` を対象リポジトリに合わせる)。手動で用意するなら
   `cp specguard.example.toml /path/to/your/repo/specguard.toml`。

2. リポジトリルートで実行:

   ```sh
   cd /path/to/your/repo
   specguard init                     # config + SessionStart hook を scaffold
   specguard run                      # 監査を実行
   specguard scope                    # 解決されたスコープだけ表示 (agent 呼ばない)
   specguard prompt                   # 各 shard のプロンプトを表示 (agent 呼ばない)
   specguard ack                      # 対応済みの sentinel をクリア
   specguard decide "<title>"         # 決定ログ(ADR)を canon commit に pin して生成
   specguard --baseline HEAD~5 run    # baseline を上書き
   specguard --config examples/aegis.toml run
   ```

   `needs_user=yes` の指摘に人間が対応したら `specguard ack` で sentinel を消す
   (これをしないと SessionStart hook が同じ件で促し続ける)。

出力:

| パス | 内容 |
|---|---|
| `<report_dir>/<date>.md` | レポート本体 |
| `<report_dir>/.last-ref` | 最後に監査した HEAD (次回の change-triggered baseline) |
| `<sentinel>` | `needs_user=yes` のときだけ (date / report / summary) |

## 設定 (TOML)

`specguard.example.toml` に全項目のコメント付きサンプルがあります。要点:

- `[project]` … `name`, `root` (リポジトリルート)
- `[agent]` … `command` + `args`。既定は `claude --print`(read-only)。任意のエージェント
  CLI に差し替え可 (プロンプトを stdin で受け、レポートを stdout に返すもの)
- `[scope]` … `baseline_ref` / `fallback_ref` (両方解決不能なら全 tracked file を監査)
- `[output]` … `report_dir` / `sentinel`
- `[prompt]` … `template` (省略時は埋め込み既定テンプレート)
- `[[area]]` (複数) … `name` / `globs` / `canon`。**globs にマッチする変更があれば in-scope**
- `[[invariant]]` (複数) … `name` / `description` / `canon`。**毎回チェック**

AEGIS の元実装を再現する設定例は `examples/aegis.toml`。

## 終了コード

| code | 意味 |
|---|---|
| 0 | 正常終了 |
| 2 | 設定 / 使用法エラー |
| 3 | いずれかの shard の出力に marker が無い (レポートは保存。baseline 前進・sentinel はしない) |
| 4 | いずれかの shard のエージェントが非ゼロ終了 (真の終了コードは stderr に出力) |

- エージェント由来の終了コードは **そのまま伝播しない**。specguard 自身が `2`(usage) /
  `3`(no-marker) を予約しており、生伝播すると「agent が 3 で死んだ」のか「marker 欠落」
  なのか区別できなくなるため。エージェント失敗は常に `4` に集約し、各 shard の実際の
  コードは stderr に出す。

## 定期実行 / HOTL 連携

`specguard run` を cron / Windows タスクスケジューラ等から起動し、`needs_user=yes`
のとき立つ sentinel を SessionStart hook で検知して「修正に着手?」を促す、という
Human-on-the-loop ループに組み込めます (sentinel フォーマットは元の AEGIS 実装と互換)。
SessionStart hook は `specguard init` が `.claude/settings.json` に設定します。

## テスト

```sh
cargo test          # unit (parse / scope / prompt / report) + integration (fake agent)
```

統合テストは `bash -c` の擬似エージェントを使うので実 LLM は不要です。

## ライセンス

MIT
