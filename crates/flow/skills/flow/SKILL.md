---
name: flow
description: 課題の供給（compass の次の一手 / backlog のキュー）から解決手段の実行（condukt、fugu-router がモデル選択）までを1本のループで貫く統合 driver。source→executor を束ねる「フレームワーク層」。SessionStart で開いている仕事があれば自動で提案され（承認後に起動）、手動でも `/flow` で起動できる。判定（どの source を引くか・止め時）は LLM、状態維持・ロック・モデル選択は既存バイナリ（compass/backlog/condukt/fugu-router）が担う。
argument-hint: "[任意: 直接の課題文。省略時は compass→backlog から自動でピック]"
allowed-tools: Task, AskUserQuestion, Bash(backlog:*), Bash(compass:*), Bash(condukt:*), Bash(fugu-router:*), Bash(git:*), Read
---

# /flow — 統合 source→executor driver

`/flow` は **課題の供給 → 解決手段の実行** を1本のループで回す。

```
SOURCE（課題の供給）              EXECUTOR（解決手段の実行）
  compass  … 次の右サイズの一手   ─┐
  backlog  … 確定済みキュー        ├─▶  condukt（fugu-router がモデル選択）─▶ verify
  prompt   … ユーザー直の課題文   ─┘
```

**役割分担（外さない）**: ループ制御（どの source を引くか・実行・検証・止め時の判定）は **この skill（LLM）**。
状態維持・ロック・size routing・モデル選択は **既存バイナリ**（`compass` / `backlog` / `condukt` / `fugu-router`）。
この skill は新しい状態を持たず、**既存の決定論レイヤを束ねるだけ**。

## いつ使うか

- SessionStart で「開いている仕事がある」と提案され、ユーザーが承認したとき（L2: propose-then-confirm）。
- 「次の課題を自分で選んで実行し続けてほしい」とき（手動 `/flow`）。
- `$ARGUMENTS` に課題文を直接渡せば、source 選択を飛ばしてその課題を condukt に流す。

## 競合しない理由（重要）

- **source と executor は直交**し、state ディレクトリも別（compass / backlog / condukt はそれぞれ独立ストア）。
- `/flow` は **backlog のロックを共有**して直列化する。`/backlog` と同時に走らせない（両方が condukt run を生むため）。**`/flow` は `/backlog` の上位互換**（compass ゲート＋複数 source を足したもの）。
- compass は **ゲート兼優先順位付け**、backlog は **確定キュー**、condukt は **executor** という分担を崩さない。

---

## 手順

### Step 0 — 引数分岐

`$ARGUMENTS` に課題文があれば → **Step 3（その課題文で condukt 実行）へ直行**。ループはせず1件だけ実行して終了（明示課題は「今これをやれ」の意味）。
引数が空なら → Step 1 へ（source から自動ピックするループ）。

### Step 1 — compass ゲート（盲目実行の防止）

source を引く前に、ゴールが鮮明かを確認する:

```bash
compass gap     # ゴール−現状の gap と候補の一手を出す
```

- charter が **陳腐・矛盾・抽象すぎて一手が引けない**場合 → **自動実行しない**。
  ユーザーに「先に `/compass` で再オリエンテーションが必要」と伝えて**停止**する（権威で自動解決しない）。
- charter が鮮明で **右サイズの一手が引ける**場合 → その一手を `to_condukt` 候補として保持し、Step 2 へ。

> compass は「ONE に絞り残りは parked」が思想。`/flow` はそれを尊重し、compass の主筋を**最優先 source** として扱う。

### Step 2 — ロック取得（クロスセッション直列化）

backlog のロックを使って二重ループを防ぐ:

```bash
backlog lock status
backlog lock acquire --session-id <SESSION_ID> --project <CWD>
```

- 別セッションがアクティブにロック保持中 → `AskUserQuestion`（待機 / 強制奪取 `--force` / 中止）。
- stale なら Step で `--force` 取得。
- 取得失敗時は理由を報告して終了。

### Step 3 — 実行ループ（繰り返し）

「source が尽きる / 予算超過 / ユーザー中断」まで以下を繰り返す。

#### 3-1. 次のタスクを優先度順にピック

1. **compass の主筋**（Step 1 の `to_condukt`）が未消化なら → それを最優先で選ぶ。
2. なければ **backlog**:
   ```bash
   backlog next [--project <path>]
   ```
   結果が空（0件）かつ compass 主筋も無し → **ループを抜けて Step 4 へ**。
3. ピックしたタスクのタイトル＋ notes（仕様・制約・参照ファイル）を**課題文**に組み立てる。

#### 3-2. condukt で実行（fugu-router がモデル選択）

課題文を `/condukt` に渡す。condukt が分解 JSON を出したら、`fugu-router` が各タスクの `suggested_model` を実績から上書きする（併用時）:

```
/condukt <課題文>
```

- `/condukt` は **`Task` ツールで非同期起動**（オーケストレーション継続のため）。
- compass 由来の一手なら、`north_star / current_gap / measuring_stick` を文脈として課題文に添える。

#### 3-3. 検証 → sink（結果の書き戻し）

condukt の完了ゲートを通ったら結果を source に書き戻す:

- **成功**:
  - backlog 由来 → `backlog done <id>`
  - compass 由来 → compass の一手を完了として記録し、次サイクルの gap を取り直す（`compass gap`）。
  - fugu-router 併用時 → 検証結果（どのモデルが通ったか・コスト）を `record` で書き戻して方策を更新。
- **失敗**（blocked / needs-serial 等）:
  - backlog 由来 → `backlog fail <id> --reason "<概要>"`、スキップして次へ。
  - ユーザーに失敗を通知するが、ループは続行。

#### 3-4. ループ継続判定

3-1 に戻る。早期脱出条件（下記）に当たれば Step 4 へ。

### Step 4 — ロック解放とサマリ

source が尽きた / ユーザー中断 / 予算超過のいずれかで:

```bash
backlog lock release
```

**早期脱出時もロック解放は必須**。最後に「処理件数・成功・失敗・残キュー・次に取り直した gap」を報告する。

## 早期脱出

| 状況 | 対応 |
|---|---|
| ユーザーが中断を指示 | 直ちに Step 4（ロック解放）へ |
| 連続失敗が 3 件以上 | `AskUserQuestion` で「続行 / 中止」 |
| budgetguard が予算超過を返す | ループ終了（Step 4）。残キューはそのまま次セッションへ |
| compass ゲートが「再スコープが必要」を示す | ループを止め、`/compass` をユーザーに促す |
| `backlog next` が予期しないエラー | 報告して Step 4 へ |

## ハードルール

- **source/executor の役割を混ぜない**: 課題の選定は compass/backlog、実行は condukt。`/flow` 自身は判定とループだけ。
- **driver は1本**: `/flow` 実行中は `/backlog` を併走させない（backlog ロックで物理的に直列化されるが、ユーザーにも明示する）。
- **盲目実行しない**: compass ゲートが鮮明でない限り、自動でキューを流し始めない。
- **ロック解放を絶対に飛ばさない**（早期脱出・エラー時も）。
