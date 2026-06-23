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
- condukt の Phase 6 (各タスク検証後) で `record` を呼び、結果を蓄える(次回が賢くなる)。
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

3. **record(学習信号)** — 各タスクの検証後:
   ```bash
   fugu-router record --title "<title>" --files "<touched_files>" \
     --class <class> --model <使ったモデル> --status verified|failed --cost <gauge から>
   ```

4. **可視化**: `fugu-router stats` でモデル別 pass率/平均コストを確認(HOTL)。

## 不変条件
- **gated タスクは自動ルーティングしない** — 人間承認の対象。`route` は触らない。
- **soft 依存** — `fugu-router` が無ければ condukt は interpreter の `suggested_model` で続行。壊さない。
- **検証は独立モデルで** — verifier は worker と別ティアを既定にし、同じ盲点を避ける。

## コールドスタート
ストアが空なら keyword prior(design/refactor/security/多ファイル→opus、rename/format/docs→haiku、
他→sonnet)。`min_samples` 件の実績が貯まるまではこの prior を使う。
