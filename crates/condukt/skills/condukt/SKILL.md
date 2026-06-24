---
name: condukt
description: 課題を解釈→タスク分割→合意→並列/直列スケジュール(決定論)→worktree並列実装→検証→完了ゲートまで回す合意駆動オーケストレーター。複数ステップ・複数ファイルにまたがる大きめの課題に使う。分割の衝突解析・worktree・状態管理・ゲートは condukt バイナリが決定論的に担い、LLM は解釈・実装・検証に集中する。
argument-hint: [課題文]
allowed-tools: Task, AskUserQuestion, Bash(condukt:*), Bash(fugu-router:*), Bash(git:*), Read, Write, Edit, Grep, Glob
---

# /condukt — 決定論エンジン駆動オーケストレーター

`/condukt <課題>` で、解釈→分割→合意→並列実装→検証→統合を一サイクル回す。

**役割分担**: 判断 (解釈・実装・検証) は LLM、決定論 (衝突解析・スケジュール・worktree・
状態・完了ゲート) は `condukt` バイナリ。バイナリがあるかは `condukt --version` で確認でき、
無ければユーザーに plugin 導入 (README) を案内する。

## 不変条件 (外さない)

1. **合意は main loop のみ** — `AskUserQuestion` はこの skill (main) でしか使えない。合意未了の
   タスクを実装に渡さない。
2. **GATED は子に実行も承認もさせない** — deploy 等 `class:"gated"` のタスクは `condukt schedule`
   が `gated` に分離する。実装フェーズの対象外。承認はユーザーから main で得る。
3. **共有ファイルは直列** — `condukt schedule` が `shared_globs` 設定と file 衝突解析で `serial` に
   落とす。serial タスクは worktree に出さず main で順に実装する。
4. **並列実装の子は専用 worktree、1 dir = 1 branch** — worktree は `condukt worktree create` が
   作る (repo 外・branch 重複拒否を強制)。各子は自分の turn 内で commit。
5. **完了は `condukt state gate` が判定** — 「全タスク verified かつ worktree 残置・未コミット無し」を
   満たすまで完了宣言しない。

## 手順

### Phase 0 — 受領
引数から課題文を取る (無ければ直前の会話の依頼)。`--dry-run` なら Phase 3 の schedule 提示で止める。

**open run チェック**: `--resume` フラグが無い場合でも、まず停止中 run が無いか確認する:
```
condukt state list
```
結果に応じて分岐する:

| open run 数 | $ARGUMENTS | 対応 |
|---|---|---|
| 0 件 | あり | 通常フロー（Phase 0.5 へ） |
| 0 件 | 空 | 直前の会話から課題を取る |
| **1 件** | **空** | **AskUserQuestion なしで自動的に Phase 0-alt（resume）へ移行** |
| 1 件 | あり | 新規課題として扱う（既存 run は放置） |
| 2 件以上 | 空 | `AskUserQuestion` でどれを再開するか確認 |
| 2 件以上 | あり | 新規課題として扱う |

引数が `--resume <RID>` または `resume <RID>` の形式でも **Phase 0-alt** へ進む。

**STUCK タスクの検知と回復**: `condukt state list` の結果に `running` 状態のタスクが含まれる場合、
前セッションの worker が途中で終了した可能性がある (stuck worker)。以下で回復する:
```
condukt state abandon --run $RID --all-stuck   # stuck タスクを pending に戻す
# コマンドが無い場合は個別に戻す:
condukt state set --run $RID --task <t.id> --status pending
```
pending に戻したタスクは Phase 0-alt → Phase 5 で通常通り再投入する。`--all-stuck` は TTL 超過
(デフォルト: 最終更新から 30 分超) の `running` タスクのみを対象とする。現在実行中の worker が
ある場合は誤って停止しないよう、実行中 Task の有無を確認してから実行する。

### Phase 0-alt — Resume (中断 run の再開)

`--resume <RID>` が指定された場合（または Phase 0 でユーザーが再開を選んだ場合）、Phases 0–4 を
スキップして以下を実行する:

```
condukt state resume-context --run <RID>
```

返される JSON の内容で分岐する:

| 条件 | 次のアクション |
|---|---|
| `verified_count == total_count` | Phase 7（完了ゲート）へ |
| `needs_verification` が空でない | Phase 6（検証）から再開。`needs_verification` タスクを検証する |
| `pending_tasks` / `failed_tasks` が空でない | Phase 5（実装）から再開。`pending_tasks` を通常実装、`failed_tasks` を `failure_context` 付きで実装 |

`failed_tasks` の `failure_context` は以前の verifier 理由が state に無い場合は省略し、
`done_criteria` と `touched_files` のみを渡す。再開後は通常の Phase 5→6→7 フローに合流する。

### Phase 0.5 — リサーチ (researcher agent, 条件付き)
以下のいずれかを満たす場合に `condukt-researcher` を起動する:
- 課題が外部ライブラリ/API に依存しており、仕様が手元に無い
- 既知の落とし穴 (breaking change・互換性問題) が想定される
- 新しいアーキテクチャパターンを導入する場合

以下の場合は省略して Phase 1 に進む:
- 課題がコードベース内完結で外部依存が明らか
- 簡単なリファクタリングや設定変更

researcher を起動した場合、その出力 JSON を変数に受け取り、Phase 1 の interpreter プロンプトに
含める:
```
RESEARCH_BRIEF=$(Task condukt-researcher "...")   # researcher の返す JSON
```
Phase 1 の interpreter 起動時に `research_brief: $RESEARCH_BRIEF` をプロンプトに含めることで、
interpreter が外部仕様・落とし穴・推奨パターンを踏まえた Decomposition を生成できる。

### Phase 1 — 解釈 (interpreter agent)

**knowledge 注入 (soft 依存)**: interpreter を起動する前に知識ファイルを取得し、あれば interpreter
プロンプトに含める:
```
KNOWLEDGE=$(condukt knowledge 2>/dev/null || true)
# KNOWLEDGE が空でなければ interpreter プロンプトに knowledge_context: $KNOWLEDGE として渡す
```

**playbook 検索 (soft 依存)**: fugu-router が利用可能なら、類似過去タスクの手順を取得して
interpreter プロンプトに含める (Devin Playbooks 相当):
```
if command -v fugu-router >/dev/null 2>&1; then
  PLAYBOOKS=$(fugu-router playbook search --query "<課題文の要約>" --k 3 2>/dev/null || true)
  # PLAYBOOKS が "[]" 以外なら interpreter プロンプトに playbook_context: $PLAYBOOKS として渡す
fi
```

`Task` で `condukt-interpreter` 相当 (subagent_type を持たない環境では `Explore` を model:opus で)
を起動し、課題を **Decomposition JSON** にさせる。スキーマは `agents/condukt-interpreter.md` 準拠:
```json
{ "goal": "...", "tasks": [
  { "id": "t1", "title": "...", "touched_files": ["path/or/glob", ...],
    "deps": ["他タスクid"], "class": "parallel|serial|gated",
    "suggested_model": "sonnet|opus|haiku", "done_criteria": "検証で確認する合格条件",
    "confidence": "high|medium|low" }
]}
```
`open_questions` 相当が出たら、この時点で `AskUserQuestion` を 1 回使って解消する。

### Phase 2 — 検証 + ルーティング + スケジュール (決定論)
Decomposition JSON を一時ファイルに書き:
```
condukt validate --file <json>        # 不正なら理由を提示しユーザーに差し戻し

# (任意) fugu-router があれば、学習済み方策で各タスクの suggested_model を上書きする。
# 無ければ interpreter の suggested_model のまま続行 (soft 依存・壊さない)。
if command -v fugu-router >/dev/null 2>&1; then
  fugu-router route --file <json> --report <route.json> > <json.routed>
else
  cp <json> <json.routed>
fi

condukt schedule --file <json.routed>  # → {batches, serial, gated, warnings}
```
- `fugu-router route` は「似た過去タスクで検証を通った最安ティア」を選び `suggested_model` を決定論的に確定する (fugu のコーディネータ相当を実績検索で近似)。
- `<route.json>` にはタスク id ごとの `verifier_model`(独立検証モデル)・`basis`・`rationale` が入る。Phase 6 の検証モデル選択に使う。
- `warnings` (shared_glob により serial 降格 等) はユーザーに見せる。以降 `<json.routed>` を正とする。

### Phase 3 — 合意 (main loop / AskUserQuestion)
`schedule` 結果 (並列バッチ / serial / gated) を `AskUserQuestion` で提示し合意を取る。割り直しが
出たら Decomposition を直して Phase 2 へ戻る。`--dry-run` ならここで停止。

**confidence ゲート (Devin Confidence Score 相当)**: `confidence: low` または `confidence: medium`
のタスクは、`AskUserQuestion` の計画提示で明示的に強調し、done_criteria や scope の確認を促す。
ユーザーが合意すれば通常通り進む (実装・検証のゲートは Phase 6 で行う)。

### Phase 4 — run 初期化
```
RID=$(condukt state init --file <json>)   # tasks=pending で run を作成、run id を返す
```

### Phase 4.5 — ベースライン取得
実装開始前にテストスイートの現状を記録する:
```
condukt state test --run $RID > /tmp/condukt-baseline.txt 2>&1
BASELINE_EXIT=$?
```
- exit 0（全通過）: 以降 worker / verifier は「テストが新たに壊れた」ことを fail の根拠にできる。
- exit 非 0（既存失敗あり）: `/tmp/condukt-baseline.txt` の失敗テスト一覧を `baseline_failures` として workers に渡す。verifier はこのリストに含まれる失敗を「実装前から壊れていた」として除外して合否を判定する。
- テストコマンドが未設定でエラーになる場合は無視して Phase 5 へ進む。

### Phase 5 — 並列実装 (batches を順に)
`schedule.batches` を**先頭から順に** 処理する (バッチ間は依存順、バッチ内は並列):

バッチ内の各タスク `t` について:
1. `WP=$(condukt worktree create --topic <t.id> --branch condukt/<t.id>)`
2. `condukt state set --run $RID --task <t.id> --status running --worktree "$WP" --branch condukt/<t.id>`
3. `Task` で `condukt-worker` 相当を起動 (model=`t.suggested_model`)。下表のフィールドを渡す。
4. worker の返却 status を確認する:
   - `done`: `condukt state set --run $RID --task <t.id> --status done` し、**他の worker の完了を待たずにその場で Phase 6 の verifier を起動する**（パイプライン化）。
   - `needs-serial`: 分類ミス。worktree を破棄し、タスクを serial として main で直接実装して commit する。
   - `blocked`: ユーザーにエスカレーションし、指示を仰ぐ (`AskUserQuestion` で報告する)。

バッチ内は 1 メッセージで複数 `Task` を同時発行して並列化する。worker が完了するたびに即 verifier を起動し、worker 完了の待ち合わせはしない（後続 worker が動いている間に先行タスクの検証が進む）。`serial` タスクは worktree に出さず main で順に実装し commit。

#### Worker プロンプト構成テンプレート (Phase 5 で毎回渡すフィールド一覧)

| フィールド | 必須/省略可 | 収集方法 | 説明 |
|---|---|---|---|
| 作業ディレクトリ | 必須 | `condukt worktree create` の出力 (`$WP`) | worktree 内だけで作業・commit させる起点 |
| `touched_files` | 必須 | Decomposition JSON の `t.touched_files` | worker が触れてよいファイルのスコープ |
| `done_criteria` | 必須 | Decomposition JSON の `t.done_criteria` | verifier が照合する合格条件 |
| `reproduction_tests` | 省略可 | Decomposition JSON の `t.reproduction_tests` | TDD ループ起点。渡すと worker が red→green サイクルを回す |
| `target_symbols` | 省略可 | Decomposition JSON の `t.target_symbols` | 編集対象の関数/クラス名。あれば `interface_context` も必須 |
| `interface_context` | `target_symbols` あれば必須 | main が Grep でスコープ外シグネチャを抽出 | worker に Grep させず main が事前収集。`grep -n "^pub fn\|^fn\|..." <file> \| head -60` や `grep -A 3 "fn <symbol>" <file>` でシグネチャ＋docstring のみ抽出して圧縮 |
| `knowledge_context` | 省略可 (soft 依存) | Phase 1 で取得した `$KNOWLEDGE` 変数 | プロジェクト固有の規約・落とし穴・推奨パターン (Devin Knowledge Base 相当) |
| `peer_tasks` | 並列タスクがあれば必須 | 同バッチの他タスクの `[{id, title, touched_files}]` | スコープ衝突防止 (Devin peer-awareness 相当)。`title + touched_files` の要約のみ。`done_criteria` や diff は含めない |
| `failure_context` | 再投入時のみ | verifier の `reason` + 失敗テスト出力 + `git diff` | `{reason, failed_tests, diff}` の形式。worker が前回失敗を把握して別アプローチを取る |

### Phase 6 — 検証 (verifier agent) + 実績の記録

**機械的 done_criteria の早期判定（verifier スキップ）**:
`done_criteria` が以下のような観察可能な事実の確認のみで構成される場合、verifier agent を省略して `Bash` で直接判定する:
- 特定の文字列が特定のファイルに存在する (`grep`)
- 特定のファイル/ディレクトリが存在する (`ls`, `test -f`)
- `cargo test` / `npm test` などのコマンドが exit 0 で終わる

判断基準: `done_criteria` に「実装」「ロジック」「設計」「コード」「振る舞い」等の語が無く、コマンド 1 ～ 3 本で完結するなら機械判定。shell チェックが pass → verified に set、fail → 通常 verifier フローへ（shell 判定は verifier の前段最適化であり、境界は厳しめに取る）。

done の各タスクを `condukt-verifier` 相当で done_criteria 照合する。検証する子の **model は
`<route.json>` の `verifier_model`**(worker と別ティアの独立検証。無ければ既定 sonnet)を使う。
verifier 起動プロンプトには以下を渡す:
- `done_criteria`: タスクの合格条件
- `worktree`: 対象 worktree パス
- `touched_files`: タスクの実装対象ファイル
- `target_symbols` (あれば): `t.target_symbols` — 検証対象の関数/クラス名。verifier がピンポイントで
  照合できる。
pass なら `condukt state set --run $RID --task <id> --status verified`、fail なら `--status failed`
にし理由を控える。

**confidence 再検証 (low-confidence pass の二重確認)**: verifier が `pass` かつ `confidence: low`
を返した場合は、model を 1 ティア上げて同じタスクを再度 verifier に投げ、2 回 pass で verified
に昇格する (Devin confidence-gated clarification の検証側相当)。2 回目も pass なら verified、fail
なら fail として通常のカスケードエスカレーションへ。

#### カスケードエスカレーション (失敗タスクのリトライ全般をここで管理)
verifier が fail したら、**同じターン内で**以下を実行して Phase 5 へ再投入する:
1. タスクを `failed` に set。
2. `failure_context` を組み立てる:
   ```json
   { "reason": "<verifier.reason>", "failed_tests": "<失敗テスト出力>", "diff": "<git diff HEAD 2>/dev/null || git show HEAD>" }
   ```
3. `suggested_model` を 1 ティア上げる (haiku→sonnet、sonnet→opus)。
4. 新しい worktree を作成し、`failure_context` と escalated model で Phase 5 worker を再起動。

リトライ上限: **ティア数 = 最大 3 回** (haiku 初回 → sonnet 1回目 → opus 2回目)。
opus で失敗した場合、または初回から opus を使っていた場合は即ユーザーエスカレーション (それ以上上げられない)。

検証後、**結果を fugu-router に記録**して次回のルーティングを賢くする (soft 依存):
```
fugu-router record --title "<task.title>" --files "<task.touched_files をカンマ区切り>" \
  --class <task.class> --model <worker に使ったモデル> \
  --status verified|failed --cost <gauge から取れれば> \
  --done-criteria "<task.done_criteria>" \
  --notes "<worker サマリの要点 (任意)>"
```
`--done-criteria` を渡すと、verified タスクの手順が `~/.fugu-router/playbooks.jsonl` に蓄積され
次回 Phase 1 の playbook 検索に現れる (Devin Playbooks 相当)。failed の場合は無視される。

### Phase 7 — 完了ゲート + 統合
```
condukt state gate --run $RID      # exit 0 まで完了宣言しない
```
- gate FAIL の場合、**まず reconcile を試みる**（branch がマージ済みのタスクを自動 verified に昇格）:
  ```
  condukt state reconcile --run $RID
  condukt state gate --run $RID    # 再チェック
  ```
- reconcile 後も FAIL が残る場合に限り、理由ごとに対処する:
  - `failed` タスク → Phase 6 のカスケードエスカレーションへ戻す
  - worktree 残置 → `condukt worktree cleanup --remove` で掃除
  - 未コミット → 該当 worktree 内で commit させる
- 各 verified タスクの worktree を **自分の turn 内で** 閉じる:
  `condukt worktree merge --branch condukt/<id>` → `condukt worktree remove --path "$WP" --branch condukt/<id>`。
  最後に `condukt worktree cleanup` で orphan が無いことを確認。
- **merge pre-flight 衝突への対処**: `condukt worktree merge` が merge pre-flight で衝突を検出した
  場合は以下の手順で対処する:
  1. 衝突しているタスク (branch) を特定する。衝突 branch が複数ある場合は 1 つずつ処理する。
  2. 衝突が軽微で自動解消可能な場合: worktree 内に移動して `git merge main` → 手動でコンフリクト
     マーカーを解消 → commit してから再度 `condukt worktree merge` を実行する。
  3. 衝突が大きく再実装が必要な場合: タスクを `failed` に set し、Phase 6 カスケードエスカレーション
     を経て新しい worktree で再実装する。再実装 worker には衝突の詳細を `failure_context.reason` に
     含めて渡す。
  4. 解消後に再度 `condukt state gate --run $RID` を実行して gate PASS を確認する。
- gate PASS で統合完了を報告 (タスク表 / 変更ファイル / 検証結果 / GATED の残提案)。

### Phase 8 — クローズ
`commit`/`push` はユーザー指示時のみ。GATED タスク (deploy 等) はユーザー承認を得てから別途実行。

## 失敗モード
- バイナリ不在 → README の導入手順を案内 (plugin install)。
- 子が共有ファイルに触りたがる → 分類ミス。serial 降格して main で実装。
- worktree 残置 → Phase 7 で必ず閉じる。`condukt state gate` が残置を検出する。
- **stuck worker** → `condukt state abandon --run $RID --task <id>` で `pending` に戻し Phase 5 へ
  再投入する。`--all-stuck` で TTL 超過の running タスクをまとめて pending に戻せる。Phase 0 の
  open run チェック時に running タスクを検出したら、Task の有無を確認後に実行する。
- **merge 衝突** → Phase 7 で `condukt worktree merge` が pre-flight 衝突を検出した場合、worktree
  内で手動マージ解消後に Phase 7 リトライするか、大きな衝突は再実装として Phase 5 に戻す。詳細は
  Phase 7「merge pre-flight 衝突への対処」を参照。
- **condukt 自身を改修する場合** → 触れてよいファイルは必ず **git リポジトリ側**
  (`crates/condukt/agents/`・`crates/condukt/skills/`・`crates/condukt/src/` を含む
  worktree) を指す。**install キャッシュ (`~/.claude/plugins/cache/.../condukt/...`) を
  worker に編集させない** — キャッシュ編集は git 外でリポジトリと黙って乖離し、新規 install で
  消える。worker に渡す touched_files はリポジトリ相対パスにし、統合後に
  `crates/condukt/scripts/sync-plugin-assets.sh` でローカル install を更新する
  (`--check` で乖離検出)。
