---
name: distill
description: context-rot 対策の能動蒸留。現在の会話を蒸留して ctxrot ストアへ退避し、main context を「要約＋リンク」に置換する。context 使用率が高い時、長い会話を畳みたい時、/compact の前に使う。
argument-hint: '[蒸留のフォーカス（任意。例: "認証まわりだけ"）]'
allowed-tools: Task, Bash(ctxrot:*), Bash(echo:*), Read, Write
---

context-rot に対抗するため、**今のセッションを能動的に蒸留**して外部ストアへ退避し、以降は
main context を軽く保つ運用に切り替える。

## 方針

重い読み込み（transcript の精査など）で **main context を汚さない**ことが最優先。原則として
**`ctxrot-distiller` subagent に Task で委譲**し、結論とノートのパスだけを受け取る。

## まず使用率を確認（使用率連動）

蒸留に入る前に、現在の context 使用率を見て挙動を変える:

- 現在の使用率: !`ctxrot usage`

上の `band` と `hint` で判断する（`band` は 0=〜50% / 1=50〜75% / 2=75〜90% / 3=90%〜）:

- **band 0（低い・〜50%）**: distill はまだ急ぎ不要。focus 引数が**無ければ**「まだ蒸留は不要（使用率 N%）。
  それでも実行しますか？」と一言確認し、ユーザが望むか focus 指定がある時だけ続行する（無駄な蒸留で
  かえって手間を増やさない）。
- **band 1（中・50〜75%）**: 通常どおり下記の手順で蒸留する。
- **band 2 以上（高い・75%〜）**: 蒸留を実行し、手順3の締めで **`/compact` を必須**で促す（最優先）。
  使用率が解決できない（"不明" が返る）時は band 1 相当として通常実行してよい。

## 手順

1. **退避先・transcript・session id を把握**:
   - 退避先ディレクトリ: !`ctxrot note dir`
   - **この（元）セッションの id**: この（メイン）セッションで `echo "$CLAUDE_CODE_SESSION_ID"` を実行して取得する。
     （ロード時のコマンド注入記法（行頭の感嘆符＋バックティック）は env 変数や PWD 等のシェル変数・
     クォート文字列を含むと静的解析を通らず skill 全体が失敗するため、env 由来の値は実行時に取得する。
     subagent は子セッションで id が異なるので、必ず**この**メインセッションで実行すること。）
     並列セッション運用で「元セッションが自分のノートに確実に戻る」ための鍵。
     ノート名に hash として埋め込まれ、`restore` がこの id で前方一致検索する。
   - 現在の transcript パスは、このセッションのもの（環境のフックが受け取っている JSONL）。
     不明なら subagent に「直近の会話から」と指示してよい。

2. **`ctxrot-distiller` subagent に委譲**する（Task）。プロンプトに次を渡す:
   - `cwd` = カレントプロジェクト
   - `session_id` = 手順1で得た**元セッションの id**（subagent 自身の env ではなくこれを使わせる。
     subagent は子セッションで id が異なりうるため）
   - 可能なら `transcript_path`
   - focus（指定があれば）: "$ARGUMENTS"
   subagent が蒸留→ストアへ保存し、**ノートのパス＋超要約だけ**を返す。

3. **main context を実際に軽くする**。重要: **ノートを保存しただけでは溜まった会話履歴の
   トークンは解放されない**（要約＋リンク運用は「以降再掲しない」という*将来の*約束にすぎない）。
   実際にトークンを解放できるのは `/compact`・`/clear`・auto-compact だけで、いずれもユーザ操作。
   スキル側（モデル）からは起動できない。よって subagent の結果を受けて、ユーザに次を伝える:
   - 保存パス（クリック可能）
   - 「以降この内容は本文に再掲しない。必要時はこのノートを読む」
   - **締めの必須ステップ**: 「context を実際に軽くするには *今* `/compact` を打ってください
     （丸ごと畳むなら `/clear`）」と明示する。任意のお願いではなく必須の最終アクションとして促す。
     `/compact` 時に走る PreCompact フック (`ctxrot rescue`) は別スラッグ (`rescue-*`) の安価な
     安全網を書くだけで、今保存した `distill-*` ノートと衝突しない → distill 直後の `/compact` は安全。

## フォールバック（subagent を使わない場合）

軽い会話で自分で蒸留できるなら、決定事項/残課題/触ったファイル/重要事実/現在地を markdown 化し
（**`## 決定事項 / Decisions` と `## 残課題 / Open todos` は必須**。空でも見出しを残し本文に
`_(なし / none)_` と書く — restore はこの2つだけを引き継ぐ）、
`printf '%s' "<本文>" | ctxrot note write --slug distill --require-sections --cwd "$PWD" --session "$CLAUDE_CODE_SESSION_ID"`
で保存してパスを報告する（`--require-sections` が必須2見出しを検査し、欠けると exit 1・未書き込みで
落ちる→見出しを足して再実行。残3節 (Files / Key facts / Where we are) は **準必須**で、欠けても
書き込みは継続するが stderr に warning が出る→該当情報があれば埋める。`--session` がノート名に
session hash を埋め restore 到達性を担保）。

要約は事実ベースで。会話に実在する内容だけを書き、推測で埋めないこと。
