---
name: distill
description: context-rot 対策の能動蒸留。現在の会話を蒸留して ctxrot ストアへ退避し、main context を「要約＋リンク」に置換する。context 使用率が高い時、長い会話を畳みたい時、/compact の前に使う。
argument-hint: [蒸留のフォーカス（任意。例: "認証まわりだけ"）]
allowed-tools: Task, Bash(ctxrot:*), Read, Write
---

context-rot に対抗するため、**今のセッションを能動的に蒸留**して外部ストアへ退避し、以降は
main context を軽く保つ運用に切り替える。

## 方針

重い読み込み（transcript の精査など）で **main context を汚さない**ことが最優先。原則として
**`ctxrot-distiller` subagent に Task で委譲**し、結論とノートのパスだけを受け取る。

## 手順

1. **退避先と transcript を把握**:
   - 退避先ディレクトリ: !`ctxrot note dir --cwd "$PWD"`
   - 現在の transcript パスは、このセッションのもの（環境のフックが受け取っている JSONL）。
     不明なら subagent に「直近の会話から」と指示してよい。

2. **`ctxrot-distiller` subagent に委譲**する（Task）。プロンプトに次を渡す:
   - `cwd` = カレントプロジェクト
   - 可能なら `transcript_path`
   - focus（指定があれば）: "$ARGUMENTS"
   subagent が蒸留→ストアへ保存し、**ノートのパス＋超要約だけ**を返す。

3. **main context を圧縮**。subagent の結果を受けて、ユーザに次を伝える:
   - 保存パス（クリック可能）
   - 「以降この内容は本文に再掲しない。必要時はこのノートを読む」
   - 蒸留済みなので必要なら `/compact` を促す

## フォールバック（subagent を使わない場合）

軽い会話で自分で蒸留できるなら、決定事項/残課題/触ったファイル/重要事実/現在地を markdown 化し、
`printf '%s' "<本文>" | ctxrot note write --slug distill --cwd "$PWD"` で保存してパスを報告する。

要約は事実ベースで。会話に実在する内容だけを書き、推測で埋めないこと。
