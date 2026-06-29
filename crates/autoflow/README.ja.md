# autoflow

> 🌐 [English](README.md) ・ **日本語**

セッション終了時の auto-flow ゲート — やり残しを抱えたままセッションが終わるのを防ぐ単一の **Stop** フック。

## 目的

autoflow は、ターンが終わったときに「まだ片付いていない仕事」が残っていればセッションの終了をブロックする Stop フックである。具体的には、ターン完了時にまず一度だけ `/record`（`/session-insights:record`）を促し、続いてプロジェクトの保留タスクが片付くまで `/condukt` をループで促し、最後にクロスプロジェクトの backlog を消化させる。

中身はセッションごとの状態機械で、各フェーズが「ターンをブロックして `/` コマンドで誘導するか」「そのまま終了させるか」を決める。

| フェーズ | 条件 | autoflow の動作 |
|---|---|---|
| **Idle** | このセッションで十分なターン数とツールイベントがある | block → `/session-insights:record` |
| **RecordRequested / Continuing** | condukt タスクがまだ保留中 | block → `/condukt`（自動は 4 回まで、5 回目以降はユーザーに確認） |
| **Continuing** | condukt は片付き、backlog に未消化アイテムがあり、compass charter が新鮮 | block → `/backlog <次のアイテム>` |
| **Continuing** | backlog は未消化だが compass charter が**陳腐化** | `/compass` を促して撤退 |
| **Done** | 保留なし | ターンの終了を許可 |

暴走防止として、ブロックは自動では 4 回までで、5 回目のプロンプト以降は続行前にユーザーへ確認する。compass はソフト依存であり、存在しない／パースできない場合は charter を新鮮とみなして処理を進める。また別の生きたセッションが backlog ロックを保持している間は autoflow は完全に撤退し、稼働中の `/flow` や `/backlog` driver を二重に駆動しない。

サブスクリプションネイティブな設計で、1 つのフックと同梱の Rust バイナリだけで動き、**API キーは不要**、デーモンも不要。フックは理由付きの `block` 判定を出すだけで、自身が作業を実行することはない。状態ファイルが無い場合や stdin が空の場合は exit 0 で抜けるため、ターンが壊されることはない。

## どうして必要か

長いセッションは、record を取り忘れたり、condukt の保留タスクを残したり、backlog に積んだアイテムを放置したまま終わりがちである。これらは「ターンが終わった」というだけの理由で人間にもエージェントにも気付かれず、床に落ちたまま忘れられる。autoflow はセッション終了という決定論的なタイミングに「やり残し検査」を割り込ませ、record → condukt → backlog の連鎖を確実に回す。

判断（どう作業するか）は引き続き各スキルと LLM が担い、autoflow が担うのは「終わらせてよいか」のゲートだけである。だからこそ暴走しないよう、自動ブロックには上限があり、上限を超えたらユーザーに判断を委ね、他セッションがロックを握っていれば手を引く。

## どう使うか

プラグインマーケットプレイス経由でインストールすると、同梱の `hooks/hooks.json` が **Stop** フックを `${CLAUDE_PLUGIN_ROOT}/bin/autoflow stop` に自動配線する。ほかに設定は要らず、ゲートはデフォルトで有効。しきい値（最小ターン数・最小ツールイベント数・backlog プロンプトの最大回数）は config のデフォルト値から来る。

スタンドアロン（cargo）で使う場合:

```sh
cargo install --path .
autoflow stop        # Stop フック: record→condukt→backlog の状態機械を実行
```

`autoflow stop` は stdin でフック JSON を読み、`block` 判定を出力する（または何も出力しない）。`AUTOFLOW_DISABLE=1` でゲートを無効化できる。

同梱の `bin/autoflow-*` バイナリがプラグインの出荷物であり、エンドユーザーは cargo も API キーも不要。フックが依存する挙動を変えたときは、ワークスペースをビルド（`cargo build --workspace --release`）して再コミットする。テストは `cargo test` で実行する。
