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

- **判定は LLM**: エージェントは Read/Grep/Glob/Bash で **生の正典と実装を読み**、
  逐語引用できないものは `不明` に降格する (hallucination で `矛盾` を捏造しない)。
- **正典の中身はコピーしない**: 設定には「どこを読むか」(canon ポインタ) だけ書く。
  中身を写すとドリフトの種になるため。
- **change-triggered + invariant**: 毎回フルリポは見ない。baseline 以降に変更が
  あった領域 + 毎回チェックする不変条件だけに絞る。baseline は
  `--baseline`/env → `[scope].baseline_ref` → `.last-ref` → `[scope].fallback_ref`
  (既定 `HEAD~20`) の順で解決し、**どれも解決できなければ最終 fallback として
  全 tracked file を監査** する (`baseline: (all tracked files)`)。若い/浅い repo の
  初回でも hard-error しない。

監査の 2 次元:
- **D1 実装↔正典 drift**: 実装が正典からずれていないか。矛盾は `A 誤読 / B コード違反 /
  C 正典が陳腐化 / 判別不能` に分類。
- **D2 仕様品質**: 正典 doc 自体の **沈黙・矛盾・重複**。

## ビルド

```sh
cargo build --release
# 生成物: target/release/specguard
```

## 使い方

1. 設定を用意 (`specguard.example.toml` をコピーして編集):

   ```sh
   cp specguard.example.toml /path/to/your/repo/specguard.toml
   ```

2. リポジトリルートで実行:

   ```sh
   cd /path/to/your/repo
   specguard run                      # 監査を実行
   specguard scope                    # 解決されたスコープだけ表示 (agent 呼ばない)
   specguard prompt                   # 描画されたプロンプトだけ表示 (agent 呼ばない)
   specguard --baseline HEAD~5 run    # baseline を上書き
   specguard --config examples/aegis.toml run
   ```

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
| 3 | エージェント出力に marker が無い (レポートは保存。sentinel は立てない) |
| 4 | エージェント非ゼロ終了のうち、終了コードを伝播できない場合 (signal kill / >255) の fallback |

- エージェントが非ゼロ終了したときは **その終了コードをそのまま伝播** する (元の AEGIS
  runner の `exit $rc` と同じ)。signal kill や 255 超で u8 化できないコードのみ `4` に丸める。

## 定期実行 / HOTL 連携

`specguard run` を cron / Windows タスクスケジューラ等から起動し、`needs_user=yes`
のとき立つ sentinel を SessionStart hook 等で検知して「修正に着手?」を促す、という
Human-on-the-loop ループに組み込めます (sentinel フォーマットは元の AEGIS 実装と互換)。

## テスト

```sh
cargo test          # unit (parse / scope / prompt / report) + integration (fake agent)
```

統合テストは `bash -c` の擬似エージェントを使うので実 LLM は不要です。

## ライセンス

MIT
