---
name: fugu-router
description: condukt の分解 JSON に対し、過去の実装結果(どのモデルが検証を通ったか・コスト)から学習した方策で各タスクの suggested_model を決定論的に上書きする。fugu のコーディネータ相当を、重み学習ではなく実績の検索(k-NN)で近似する。condukt と併用し、検証後に結果を record で書き戻す。
argument-hint: [decomp.json | 課題文]
allowed-tools: Bash(fugu-router:*), Bash(condukt:*), Read
---

# /fugu-router — fugu 風モデルルーティング

fugu (Sakana AI) は**訓練済みコーディネータ**がリクエストをモデル群へ役割割り当てする。
重み学習はできないので、ここでは **実績ストアの検索**で方策を作る:過去に似たタスクで
どのモデルが検証を通ったか(とコスト)を k-NN で引き、**しきい値を満たす最安ティア**を選ぶ。

## いつ使うか
- condukt の Phase 2 (validate と schedule の間) で `route` を噛ませ、`suggested_model` を学習方策で上書きする。
- condukt の Phase 6 (各タスク検証後) で `record` を蓄える(次回が賢くなる)。**condukt と併用する場合、
  発火は手動ではなく condukt の Stop hook が `condukt state record-run --all` で決定論的・冪等に行う**
  (下記 3 の手書き snippet は単発/condukt 非併用時のフォールバック)。
- 単発でモデルの当たりを知りたいとき `suggest`。

## 手順

1. **route(決定論ルーティング)**
   ```bash
   condukt validate --file decomp.json
   fugu-router route --file decomp.json --report /tmp/route.json > decomp.routed.json
   condukt schedule --file decomp.routed.json
   ```
   - stdout = `suggested_model` を更新した decomposition(そのまま condukt へ)。
   - `--report` = タスク id ごとの `{worker_model, verifier_model, basis, confidence, rationale}`。
     condukt のスキーマに無い **verifier_model**(独立した検証モデル)はここから読む。

2. **実装(condukt)**
   worker は `suggested_model`、verifier は report の `verifier_model` で起動する。

3. **record(学習信号)** — 各タスクの検証後 (condukt 併用時は Stop hook が自動発火するので手書き不要):
   ```bash
   fugu-router record --title "<title>" --files "<touched_files>" \
     --class <class> --model <使ったモデル> --status verified|failed --cost <gauge から>
   ```
   `--files` に絶対パスを渡すと、リポジトリルートからの相対パスに自動変換される
   (`/Users/yuki/src/harness/crates/x.rs` → `crates/x.rs`)。
   マシン固有のパスセグメントがストアに混入しないため、stores を git 管理して
   別マシンへ同期しても k-NN の精度が落ちない。

4. **可視化**: `fugu-router stats` でモデル別 pass率/平均コストを確認(HOTL)。

5. **import(クロスマシン同期)** — 別マシンの stores を取り込む:
   ```bash
   # 別マシンから持ってきた episodes.jsonl をローカルにマージ
   fugu-router import --episodes /path/to/synced/episodes.jsonl [--playbooks ...] [--dry-run]
   # ローカルの重複を除去(content-hash 一致を first-seen 優先で削除)
   fugu-router import --dedup
   ```
   `--dry-run` は書き込みをせず件数だけ表示する。

## 不変条件
- **gated タスクは自動ルーティングしない** — 人間承認の対象。`route` は触らない。
- **soft 依存** — `fugu-router` が無ければ condukt は interpreter の `suggested_model` で続行。壊さない。
- **検証は独立モデルで** — verifier は worker と別ティアを既定にし、同じ盲点を避ける。
- **stores は append-only** — `import` は既存ストアに追記のみ。`--dedup` 書き換えは temp+rename で原子的。

## 設定 (`~/.fugu-router/config.toml`)
| キー | デフォルト | 説明 |
|---|---|---|
| `store_file` | `~/.fugu-router/episodes.jsonl` | エピソードストアパス |
| `playbook_file` | `~/.fugu-router/playbooks.jsonl` | プレイブックストアパス |

両ファイルを git リポジトリ内のパスに向けると、`git pull` 後に `import --dedup` するだけで
マシン間同期が完結する。

## コールドスタート
ストアが空なら keyword prior(design/refactor/security/多ファイル→opus、rename/format/docs→haiku、
他→sonnet)。`min_samples` 件の実績が貯まるまではこの prior を使う。
