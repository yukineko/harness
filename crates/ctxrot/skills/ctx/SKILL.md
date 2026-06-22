---
name: ctx
description: context に何を載せるか/載せないかを明示制御する。/ctx load <path> で意図的にロード（巨大なら sub-agent 経由で要約だけ）、/ctx pin で次セッション以降も再浮上、/ctx unload(=drop) で除外、/ctx list で現状確認。hooks + binary だけで動き subscription で完結（API キー不要）。
argument-hint: 'load|unload|pin|unpin|drop|list [path]'
allowed-tools: Task, Bash(ctxrot:*), Read
---

ユーザが **main context に何を入れる / 入れないかを手で制御**するためのスキル。状態は
`ctxrot ctx`（プロジェクト単位の loadset = `pinned` / `dropped`）に永続化される。

**重要な前提**: hooks は「すでに context window に載っているトークンを後から降ろす（evict）」
ことはできない。だから制御は2点に集約される — **入口で止める/絞る**（load を慎重に）と
**再構成で実効化する**（drop は `/compact`・`/distill`・新セッション carryover で効く）。

## 引数の解釈

`$ARGUMENTS` の最初の語を **action**、残りを **path/label** とみなす。action が無ければ
`list` として扱う。

---

## action ごとの手順

### `list`（または引数なし / `status`）
現状を表示するだけ。

1. `ctxrot ctx list` を実行して pinned / dropped を表示。
2. `ctxrot usage` を実行して現在の context 使用率（band）も併記。
3. dropped があれば「ライブ context には残っている。降ろすには `/compact` を」と一言添える。

### `load <path>`
その場で**意図的に**ロードする。ただし丸ごと貼って rot を作らないこと。

1. ファイルサイズを確認: `ctxrot` の入口ゲート方針（既定 ~1MB 以上 / `load_deny` 一致）に
   該当しそうなら、**`Read` で直接読まず** Explore か general-purpose **sub-agent に `Task` で
   委譲**し、該当箇所・要約・結論だけを受け取る。
2. 小さく確実に必要な範囲だけなら `Read`（必要なら `offset`/`limit` で絞る）。
3. 読み終えたら「次回以降も必要か？」を確認し、yes なら `ctxrot ctx pin <path>` で pin を勧める。

### `pin <path|label>` / `unpin <path|label>`
次セッション以降も `restore`（SessionStart）が**ポインタとして再浮上**させたい物を登録/解除。

- `ctxrot ctx pin <item>` / `ctxrot ctx unpin <item>` を実行して結果を報告。
- pin は中身を貼るわけではない（パス/ラベルの一覧だけが carryover に出る）と明示する。

### `unload <path>` / `drop <path>`
context から**外したい**物を登録する（`unload` は `drop` のエイリアスとして扱う）。

1. `ctxrot ctx drop <item>` を実行。
2. **即時には消えない**ことを必ず伝える: 「ライブ context からは降りない。`/compact` するか
   新セッションを開始すると実効化され、以降の `/distill`・carryover はこれを除外する」。
3. 使用率が高い（`ctxrot usage` の band が 2 以上）なら、その場で `/compact` を勧める。

### `undrop <path>`
- `ctxrot ctx undrop <item>` を実行して報告。

---

## 方針メモ

- 重い読み込みは必ず sub-agent に逃がし、main context を汚さない（ctxrot 全体の原則）。
- このスキルが出す文章自体も rot 源にならないよう、出力は短く要点だけにする。
- 入口ゲートの恒久ルール（`load_deny` / `load_allow`）は `~/.ctxrot/config.toml` で設定する。
  一時的・対話的な pin/drop はこのスキル、恒久的なパスルールは config、と役割を分ける。
