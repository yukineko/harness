---
name: flow
description: 課題の供給（compass の次の一手 / backlog のキュー）から解決手段の実行（condukt、fugu-router がモデル選択）までを1本のループで貫く統合 driver。source→executor を束ねる「フレームワーク層」。SessionStart で開いている仕事があれば自動で提案され（承認後に起動）、手動でも `/flow` で起動できる。判定（どの source を引くか・止め時）は LLM、状態維持・ロック・モデル選択は既存バイナリ（compass/backlog/condukt/fugu-router）が担う。
argument-hint: "[任意: 直接の課題文。省略時は compass→backlog から自動でピック]"
allowed-tools: Task, AskUserQuestion, Bash(backlog:*), Bash(compass:*), Bash(condukt:*), Bash(fugu-router:*), Bash(hypothesis:*), Bash(git:*), Read
---

# /flow — 統合 source→executor driver

`/flow` は **課題の供給 → 解決手段の実行** を1本のループで回す。

```
SOURCE（課題の供給）              EXECUTOR（解決手段の実行）
  compass    … 次の右サイズの一手   ─┐
  backlog    … 確定済みキュー        ├─▶  condukt（fugu-router がモデル選択）─▶ verify
  hypothesis … 計測待ちの PDO 仮説   │
  prompt     … ユーザー直の課題文   ─┘
```

> `hypothesis` は PDO discovery の出力（検証したい仮説）を実行へ繋ぐ source。**2 相**で扱う:
> ① **open** な仮説 → **RAT ゲート**（Step 3-1 の 4）を先に通す: 未テストの高リスク×弱証拠 assumption
>    （leap of faith）があれば、full build ではなく**その assumption だけを de-risk する最小実験**に落とす。
>    leap of faith が無ければ「その仮説を検証する実験」として condukt に流す（build）。完了すると condukt が
>    gate PASS 時に `awaiting-measurement`（出荷済み・未計測）へ遷移させる。
> ② **awaiting-measurement** な仮説 → **measure step**（Step 3-1 の 2）で観測値を回収し、
>    **計測した証拠を添えて** validate/reject して閉じる（出荷だけでは validate しない＝build ≠ validate）。

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

### Step 0.5 — 自律ゲート（human gate 縮退の共通スイッチ）

ループ中に人間へ問い合わせる（`AskUserQuestion`）箇所は、**自律モードでは決定論的な既定へ縮退**する。
判定は毎回この**共通スイッチ**で行う（新しい機構は作らない。condukt / scout と**同一**のゲート）:

```bash
condukt state autonomy-check   # exit 0 + {"autonomous":true} → 自律 / exit 1 + {"autonomous":false} → 非自律
```

- **exit 0（autonomous）** → 下表の human gate はすべて**決定論的な既定へ縮退**し、`AskUserQuestion` を出さない。
- **exit 1（非自律・既定）** → **従来どおり** `AskUserQuestion` を出す（後方互換。挙動を一切変えない）。
- `condukt state autonomy-check` が存在しない版（`exit 127` / "unknown subcommand"）→ **非自律とみなす**
  （＝ exit 1 と同じ＝従来どおり `AskUserQuestion`。安全側フォールバック）。

| human gate（従来 Ask） | 非自律（exit 1・従来） | 自律（exit 0・決定論的既定） |
|---|---|---|
| **ロック競合**（Step 2・生きている保有者） | `AskUserQuestion`（待機 / 強制奪取 / 中止） | **stand down**（報告して clean exit。`--force` 自動奪取はしない） |
| **resume 選択**（複数の再開候補） | `AskUserQuestion` で選ばせる | **決定論的既定**（3-1 の優先度 pick 規則の先頭。Ask しない） |
| **pivot-check**（Step 4・`pivot` 判定） | `AskUserQuestion`（再オリエンテーション / 継続） | **`persevere` を自動採用**（Ask せず継続＝次の gap を取り直す） |
| **連続失敗 3 件**（早期脱出） | `AskUserQuestion`（続行 / 中止） | **clean stop**（Ask せずループを止め Step 4 へ） |

**安全不変条件（自律でも残す唯一の停止）**: 自律モードで残る human stop は **(a) worker が blocked**
（condukt がエスカレーション）と **(b) deploy/push の GATED 承認** の 2 つだけ。それ以外の human gate は
上表の決定論的既定へ縮退する。**budgetguard の予算超過による早期脱出（Step 4）はどのモードでも維持**する。

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

- 別セッションがアクティブにロック保持中（**生きている保有者**）→ `condukt state autonomy-check` で分岐:
  - **exit 1（非自律・従来）** → `AskUserQuestion`（待機 / 強制奪取 `--force` / 中止）。
    `--force` は **生きている保有者からも奪取**する（`backlog lock acquire --force ...`）。
  - **exit 0（自律）** → **stand down**: `--force` の自動奪取はせず、「別セッションが実行中のため見送り」と
    報告して**clean exit**（ロック未取得のまま正常終了。生きた保有者は決して奪わない）。
- stale（保有 pid が死亡）なら `acquire` が**自動で reap** するため `--force` は不要（自律・非自律とも同じ既存挙動）。
- 取得失敗時は理由を報告して終了。

### Step 3 — 実行ループ（繰り返し）

「source が尽きる / 予算超過 / ユーザー中断」まで以下を繰り返す。

#### 3-1. 次のタスクを優先度順にピック

1. **compass の主筋**（Step 1 の `to_condukt`）が未消化なら → それを最優先で選ぶ。
2. **measure step（計測ループを閉じる / build ≠ validate）** — 新規 build より**先に**、出荷済み・未計測の仮説を回収する:
   ```bash
   hypothesis list --status awaiting-measurement   # condukt が merge 時に遷移させた「出荷済み・未計測」
   ```
   - 各 awaiting-measurement 仮説について、**計測信号が今観測可能か**を判定する:
     - **観測可能** → これは **condukt build ではなく measure タスク**。実験で観測した成果を集め、
       そのまま 3-3 の sink で `hypothesis validate/reject --evidence` して**仮説を閉じる**
       （この 1 件はここで完了。condukt は起動しない）。3-2 を飛ばして 3-3（measure 由来）へ。
     - **まだ観測不能**（データ蓄積待ち等）→ awaiting-measurement のまま残し、
       「計測待ち（まだ観測不能）」として報告し次の候補へ進む（ここで無限ループしない）。
   - `hypothesis` バイナリが無い / 0 件なら skip。
3. measure 対象（今観測可能なもの）が無ければ **backlog**（確定キュー）。
   backlog に **複数の ready 課題がある場合は、順列（1件ずつ）ではなく 1 回の condukt run に束ねて並列処理**する。
   **並列/直列の判定は flow がするのではなく、condukt の決定論スケジューラ（`schedule.rs`）に委譲**する
   ＝ flow は独立候補を「束ねて渡すだけ」で、ファイル競合・`Serial`/`Gated` クラス・shared-glob・依存層は
   condukt が判定し、非衝突タスクだけを並列バッチに、衝突・危険なものは自動で直列に落とす
   （**「並列が危険/高コストなら直列」はこの層で保証**される＝conservative: 迷えば直列）。

   a. **バッチを取り出す**（優先度順の上位 N 件。並びは priority→weight 降順→created_at）:
      ```bash
      backlog list --status pending --project "$PWD" --json   # 並び順どおりの JSON 配列
      ```
      先頭から最大 **N 件**（既定 N=condukt の `max_parallel`。無指定なら **4**）を候補バッチとする。
      各件の `id` / `title` / `notes` を控える（sink で **id ごとに** `done`/`fail` するため必須）。
      backlog が 1 件だけなら従来どおり単一課題として扱う（N=1）。
   b. **コスト/危険ゲート（直列フォールバック）** — 次のどれかに該当する候補は**バッチから外して 1 件ずつ直列**に回す（安全側）:
      - budgetguard が予算逼迫を示す → バッチ幅を絞る（極端なら N=1＝従来の直列に縮退）。
      - notes から明らかに **相互依存**／同一領域・同一ファイルを触る／deploy・push（Gated 相当）を含むと読める。
      判断に迷うものは**バッチに入れてよい**（condukt が衝突を検出して自動で直列化するため二重の安全網になる）。
   c. **backlog に積む側**（このループが `backlog add` する場合）— compass opportunity 由来なら **その weight を供給**する
      こと（weight が compass→backlog→flow と流れ、影響度の高い機会が先頭に来て同じ並列バッチに乗りやすくなる）:
      ```bash
      W=$(compass gap | jq -r '.opportunities[0].weight // empty')   # active outcome の最重要 opportunity の weight
      backlog add --title "<課題>" --project "$PWD" --priority p1 --weight "${W:-0}"
      ```
      weight 無指定は既定 0.0＝従来の (priority, created_at) 順（後方互換）。weight は順序を変えるだけで priority は上書きしない。
4. backlog も空なら **hypothesis（新規 discovery: open 仮説）**:
   ```bash
   hypothesis list --status open    # confidence 降順（同点 created_at 昇順）でソート済み。空なら次へ
   ```
   **`list --status open` は confidence 降順で並ぶ**ので、**先頭（最高 confidence ＝ 最も検証価値が高い仮説）から順にピック**する
   （挿入順ではなくスコア順で discovery を駆動する。各行頭の `(conf X.XX)` が検証優先度）。
   open な仮説があれば、**full build に直行する前に RAT ゲート（riskiest-assumption test）を通す**:
   ```bash
   RAT=$(hypothesis rat <hid>)      # 未テストの最重要×弱証拠 assumption（leap of faith）を 1 行返す
   ```
   - `RAT` が**非空**（高リスク・未テストの leap of faith がある）→ 課題文は **full build ではなく、
     その assumption だけを検証する最小 de-risk 実験**にする（"<assumption text> が成り立つかを最小コストで測る実験"）。
     `RAT` 行頭の index を控え、3-3 の sink で `hypothesis tested <hid> <index>` を呼んで計測ループを閉じる。
   - `RAT` が**空**（高リスクの未テスト assumption が無い＝既に de-risk 済み）→ 従来どおり
     その**仮説を検証する実験**（full build）を課題文にする。
   いずれも仮説 ID を控える。`hypothesis` バイナリが無い / 0 件 / `rat` 未対応なら従来どおり full build に流す。
5. compass 主筋・measure（観測可能なもの）・backlog・open 仮説のいずれも**実行可能なものが無い**
   → **ループを抜けて Step 4 へ**（awaiting-measurement にまだ観測不能な仮説が残っていても、
   それは「計測待ち」として残課題に計上しループは終える）。
6. ピックしたタスクを**課題文**に組み立てる:
   - **単一課題**（compass 主筋 / measure / hypothesis / 単発 backlog）→ タイトル＋ notes（仕様・制約・参照ファイル）で従来どおり 1 課題文。
   - **backlog バッチ（3 の a/b で複数残った場合）→ 1 つの課題文に、各 backlog item を「独立した top-level タスク」として列挙**する。
     各項目に **id・タイトル・notes・（分かるなら）触るファイル/領域**を明記し、「**これらは互いに独立。非衝突なものは並列に、
     衝突・共有リソースを触るものは直列に scheduleしてよい**」と condukt に明示する（分解時に item 境界を保てるよう、
     item 単位で done_criteria を切ってもらう）。**item id ↔ condukt タスク**の対応を控える（sink で id ごとに書き戻すため）。
     並列上限は condukt の `max_parallel`（既定 4）が実効的に効くので flow 側で待ち合わせ制御はしない。
7. **選択を shared discovery store に記録**（未選択は `discovered` で次サイクルへ。バッチなら選んだ各 item を記録）:
   ```bash
   compass discovery select --session-id "<SESSION_ID>" --title "<選んだタスクのタイトル>"
   ```
   - 失敗時は fail-soft（compass 欠如 / 呼び出し失敗時も続行）。

#### 3-2. condukt で実行（fugu-router がモデル選択）

課題文を `/condukt` に渡す。condukt が分解 JSON を出したら、`fugu-router` が各タスクの `suggested_model` を実績から上書きする（併用時）:

```
/condukt <課題文>
```

- `/condukt` は **`Task` ツールで非同期起動**（オーケストレーション継続のため）。
- compass 由来の一手なら、`north_star / current_gap / measuring_stick` を文脈として課題文に添える。
- **backlog バッチは 1 回の `/condukt` 呼び出し**（複数 condukt run を並走させない＝グローバル backlog ロック /
  worktree / merge 競合を増やさない）。並列化は condukt 内部（Phase 5 の worktree 並列 + schedule.rs のバッチ）が担う。
- **verify も自動で並列化される**: condukt の Phase 6 は worker 完了ごとに即 verifier を起動し待ち合わせしない
  （pipeline 検証）。バッチで複数 item を渡せば、その検証も item 横断で並列に走る＝別途 flow 側で verify を並列化する必要はない。

#### 3-3. 検証 → sink（結果の書き戻し）

condukt の完了ゲートを通ったら結果を source に書き戻す:

**バッチ（複数 backlog item を 1 run に束ねた場合）は item ごとに個別 sink する**: condukt の完了ゲートは
タスク単位なので、6 で控えた **item id ↔ condukt タスク**の対応を使い、**通ったタスクの item は `done`、
blocked/失敗の item は `fail`** と書き分ける（**部分成功をそのまま反映**＝一部が失敗しても通った分は done にする。
バッチ全体を一括で成功/失敗扱いにしない）。以下は 1 item あたりの sink:

- **成功**:
  - backlog 由来 → `backlog done <id>`（バッチなら通った各 item の id について実行）
  - compass 由来 → 完了した move を **measuring_stick で判定**し、その verdict を記録する（＝計測ループを閉じる）:
    ```bash
    compass outcome --verdict <forward|unchanged|backward> --evidence "<観測した成果>"
    ```
    verdict は move の diff・テスト結果・gap への接近度から **driver(LLM) が判定**する（前進=forward / 不変=unchanged / 後退=backward）。
    `--evidence` は計測値（テスト数・ベンチ・観測した挙動）を必須とする＝出荷だけでは記録しない（build ≠ validate）。
    記録後 `compass gap` を取り直すと `last_outcome` が次サイクルに反映される（人手の別コマンド不要＝sink の一部として自動記録）。
  - hypothesis 由来（**新規 experiment の build が完了**）→ condukt は gate PASS 時に linked_hypotheses を
    **`awaiting-measurement`（出荷済み・未計測）へ遷移済み**。**出荷しただけでは validate しない**ので、
    flow はこの場で validate/reject せず、仮説を awaiting-measurement に残す。閉じるのは**次サイクルの
    measure step（3-1 の 2）**が観測値を添えて行う（build ≠ validate）。「計測待ち N 件」を残課題として報告する。
  - measure step 由来（**3-1 の 2 で観測値を回収した awaiting-measurement 仮説**）→ 観測した成果を添えて閉じる:
    ```bash
    hypothesis validate <id> --run <RID> --evidence "<観測した成果>"   # 反証なら reject <id> --reason "<反証内容>"
    ```
    これで awaiting-measurement → validated / rejected に遷移し、計測ループが閉じる
    （`validate`/`reject` は証拠必須なので、観測値の無い「出荷だけ」では status を変えられない）。
  - fugu-router 併用時 → 検証結果（どのモデルが通ったか・コスト）を `record` で書き戻して方策を更新。
- **失敗**（blocked / needs-serial 等）:
  - backlog 由来 → `backlog fail <id> --reason "<概要>"`、スキップして次へ（バッチなら**失敗した item だけ** fail、他は上記どおり done）。
  - ユーザーに失敗を通知するが、ループは続行。

#### 3-4. ループ継続判定

3-1 に戻る。早期脱出条件（下記）に当たれば Step 4 へ。

### Step 4 — ロック解放とサマリ

source が尽きた / ユーザー中断 / 予算超過のいずれかで:

```bash
backlog lock release
```

**早期脱出時もロック解放は必須**。最後に「処理件数・成功・失敗・残キュー・次に取り直した gap」を報告する。

#### pivot-check（ループ終端の方向判断）

ロック解放の直後、ループを正常終了した場合（中断・エラー以外）は以下を実行する:

```bash
compass pivot-check   # {"recommendation":"persevere"|"pivot","streak":N,"threshold":N,"reason":"…"}
```

- **`persevere`** → そのまま継続。「次の gap を取り直す」と報告する。
- **`pivot`** → `condukt state autonomy-check` で分岐:
  - **exit 1（非自律・従来）** → `reason` に集計理由（streak 長・対象 verdict 列）が入っている。それを引用してユーザーに提示し、
    **north_star を彫り直す（再オリエンテーション）か否か**を問う（`AskUserQuestion`）。
    ユーザーが「再オリエンテーション」を選んだら `/compass` を案内して終了。「継続」なら通常どおり報告して終了。
  - **exit 0（自律）** → **`persevere` を自動採用**（pivot でも決して Ask しない）: `reason` を報告に引用しつつ、
    ループは止めず「次の gap を取り直す」で継続する（north_star の彫り直しは人間の判断として保留＝勝手に `/compass` しない）。
  pivot 判定は `compass outcome` を積み重ねることで精度が上がるため、outcomes が 0 件なら pivot-check はスキップしてよい。

## 早期脱出

| 状況 | 対応 |
|---|---|
| ユーザーが中断を指示 | 直ちに Step 4（ロック解放）へ |
| 連続失敗が 3 件以上 | `condukt state autonomy-check`: **exit 1（従来）** → `AskUserQuestion`「続行 / 中止」／ **exit 0（自律）** → Ask せず **clean stop**（ループを止め Step 4 へ） |
| budgetguard が予算超過を返す | ループ終了（Step 4）。残キューはそのまま次セッションへ |
| compass ゲートが「再スコープが必要」を示す | ループを止め、`/compass` をユーザーに促す |
| `backlog next` が予期しないエラー | 報告して Step 4 へ |

## ハードルール

- **source/executor の役割を混ぜない**: 課題の選定は compass/backlog、実行は condukt。`/flow` 自身は判定とループだけ。
- **driver は1本**: `/flow` 実行中は `/backlog` を併走させない（backlog ロックで物理的に直列化されるが、ユーザーにも明示する）。
- **並列は「バッチを 1 condukt run に束ねる」で実現**（複数 condukt run を並走させない）: backlog に複数 ready 課題が
  あれば順列ではなく 1 回の condukt run に束ねて渡し、**並列/直列の実判定は condukt の `schedule.rs`（ファイル競合・
  Serial/Gated・shared-glob・依存層）に委譲**する。flow 自身は独立候補を束ねるだけで、危険/高コストなら condukt が
  自動で直列化する（conservative＝迷えば直列）。予算逼迫や明白な相互依存が読めるときは flow 側でバッチ幅を絞る（極端は N=1）。
- **盲目実行しない**: compass ゲートが鮮明でない限り、自動でキューを流し始めない。
- **ロック解放を絶対に飛ばさない**（早期脱出・エラー時も）。
- **自律モードでは human gate を縮退する**: `condukt state autonomy-check` exit 0 のとき、
  ロック競合 / resume 選択 / pivot / 連続失敗 3 件は `AskUserQuestion` を出さず**決定論的既定へ落とす**
  （ロック競合＝stand down、pivot＝persevere、連続失敗＝clean stop）。自律で残る停止は
  **(a) worker blocked** と **(b) deploy/push の GATED 承認**、および **budgetguard 早期脱出**のみ。
  exit 1（既定・非自律）は**従来どおり全 Ask を維持**（後方互換）。存在しない版（exit 127）は非自律とみなす。
