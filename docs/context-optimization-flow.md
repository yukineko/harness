# コンテキスト最適化の流れ

このドキュメントは「何をどうやって最適化しているか」を概念の流れで説明する。
個別施策の詳細一覧は [`context-optimization.md`](./context-optimization.md) を参照。

---

## 前提：LLM が扱えるのはテキストだけ

condukt が情報をやり取りする手段はすべてテキストである。

- エージェントへの指示 → テキスト（プロンプト）
- エージェントからの返答 → テキスト（JSON もテキスト）
- 状態の保存 → テキスト（JSONL ファイル）
- コードの受け渡し → テキスト（worktree のファイル）

「コンテキストを最適化する」とは、**どのテキストをどのエージェントに渡すか**を制御することである。

---

## 1. sub-agent に流すことで context 増加を防ぐ

worker・verifier は新規起動の sub-agent であり、この会話を一切見ていない。渡されたプロンプトが唯一の情報源である。

sub-agent が終了すると、その実行の中身（ビルドログ・diff・grep 結果）は main loop のコンテキストには戻らない。**結果だけが返ってくる**。

これにより「重い作業の出力が main loop に積み上がらない」という効果がある。

---

## 2. 渡すテキストは取捨選択する

sub-agent に渡す情報は変換しない。**必要な部分だけ切り出す**。

| フィールド | やっていること |
|---|---|
| `interface_context` | ファイル全体ではなく grep で関数シグネチャの行だけ抜く |
| `knowledge_context` | `condukt knowledge` が返す規約テキストをそのまま渡す |
| `peer_tasks` | 隣接タスクの title + touched_files だけ渡す、done_criteria や diff は渡さない |

「どの部分がこのエージェントに必要か」を main loop が判断して、該当箇所だけを切り出してプロンプトに埋める。

---

## 3. ターン内の大きい出力は toolguard が truncate する

ツール出力が閾値を超えた場合、toolguard が head + tail だけ残して中間を切り捨てる。

toolguard が発動するのは**すでに context に入ってしまった後**なので、正確さを保存するためではない。目的は**次のターン以降の context 占有を減らす**こと。

- head/tail を残すのは「冒頭（import・構造）と末尾（エラー行など）は価値が高い」という経験則
- 中間の情報が必要なら sub-agent に再取得させる（nudge がそれを促す）

toolguard は**減らすだけ**で、引き継ぎの役割は持っていない。

---

## 4. セッションをまたぐ引き継ぎは rescue/restore が担う

exit してもファイルシステムに書いてあるので消えない。次に Claude Code を起動して `SessionStart` が発火した時点で読んで inject される。

**rescue**（`PreCompact` フック）：
- コンパクション直前に transcript を読んで「決定事項・残課題・触ったファイル」を抽出しファイルに書き出す
- LLM を使わず決定論的に抽出

**distill**（非同期・`claude -p`）：
- LLM が transcript を意味的に要約したノートを作る
- 元の会話とは別の文章になる（lossy な圧縮）

**restore**（`SessionStart` フック）：
- rescue/distill が書いたファイルを読んで `additionalContext` として inject する
- 同じプロジェクトの直近のノートを優先する

---

## 5. additionalContext は Claude Code のフック仕様

フックが stdout に `{"additionalContext": "..."}` を返すと、Claude Code がそれを inject してくれる。ctxrot はその仕組みを使っているだけである。

inject のタイミングは 3 つある：

| フックイベント | タイミング | 担当 | 何を inject するか |
|---|---|---|---|
| `SessionStart` | セッション開始時 | restore | 前セッションの決定事項・残課題 |
| `UserPromptSubmit` | 毎ターン | guard・injector | バンド状況・関連仕様セクション |
| `PreCompact` | コンパクション直前 | rescue | durable rescue note |

inject する中身も目的も違うが、仕組みは同じ `additionalContext` である。

---

## 6. injector はプロンプトに応じて関連部分だけを選ぶ

rescue note（restore）は全部 `additionalContext` に入れる。

injector は別の仕事をしている。プロンプトの内容を見て、管理している仕様セクションの中から関連するものをスコアリングして選び、`additionalContext` に inject する。全部ではなく**関連する部分だけ**が入る。

---

## まとめ：4 つの手段

| 手段 | やっていること | 担当 |
|---|---|---|
| **sub-agent 分離** | 実行の中身を main loop に返さない | condukt スキル |
| **取捨選択** | 必要な部分だけ切り出して渡す | main loop（手動） |
| **truncate** | 大きい出力の中間を捨てて次ターンの占有を減らす | toolguard |
| **rescue/restore** | セッションをまたいで決定事項・残課題を引き継ぐ | ctxrot |
| **inject** | プロンプトに関連する仕様だけを選んで注入する | injector |

変換・要約（lossy 圧縮）を行うのは distill だけで、他はすべて「選ぶ」「切る」「渡す」である。
