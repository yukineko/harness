---
description: 現在の会話を蒸留して ctxrot ストアへ退避し、main context を「要約＋リンク」に置換する（context-rot 対策の能動蒸留）
argument-hint: [蒸留のフォーカス（任意。例: "認証まわりだけ"）]
allowed-tools: Bash(ctxrot:*), Bash(date:*), Task, Read, Write
---

context-rot に対抗するため、**今動いているセッションを能動的に蒸留**して外部ストアへ退避し、
以降は main context を軽く保つ運用に切り替えてください。

## 手順

1. **退避先ディレクトリを取得**（無ければ作成される）:
   !`ctxrot note dir --cwd "$PWD"`

2. **蒸留ノートを作成**。フォーカス指定があれば優先: "$ARGUMENTS"
   - これまでの会話から、後続の作業に本当に必要な情報だけを抽出する。試行錯誤の経過・
     生ログ・全文引用は**含めない**（それらが rot の原因）。
   - 重い再読み込みが必要なら、自分の context を使わず **Task で Explore / general-purpose
     sub-agent に委譲**し、結論だけ受け取ってからノートに落とす。
   - 以下の見出しで markdown を作る（空の節は省略可。frontmatter を付ける）:

     ```
     ---
     type: ctxrot-distill
     created: <ISO8601>
     focus: <フォーカス or all>
     ---

     # ctxrot distill <ISO8601>

     ## 決定事項 / Decisions
     - 確定した方針・設計判断（理由を一言添える）

     ## 残課題 / Open todos
     - 次にやること（着手順）

     ## 触ったファイル / Files
     - path:line — 何をしたか

     ## 重要な事実 / Key facts
     - 後で効く制約・前提・数値・外部リンク

     ## 現在地 / Where we are
     - 1〜3行で「今ここ」を要約
     ```

3. **ストアへ書き込む**。手順1で得たディレクトリに `distill-<日付時刻>.md` を Write する。
   （`ctxrot note write --slug distill` に本文を渡してもよい）

4. **main context を圧縮**。ノート保存後、ユーザに次を伝える:
   - 保存パス（クリック可能）
   - 「以降この内容は本文に再掲しない。必要時はこのノートを読む」
   - 必要なら `/compact` を促す（蒸留済みなので安全に畳める）

要約は事実ベースで。会話に実在する内容だけを書き、推測で埋めないこと。
