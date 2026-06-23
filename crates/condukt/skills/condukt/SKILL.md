---
name: condukt
description: 課題を解釈→タスク分割→合意→並列/直列スケジュール(決定論)→worktree並列実装→検証→完了ゲートまで回す合意駆動オーケストレーター。複数ステップ・複数ファイルにまたがる大きめの課題に使う。分割の衝突解析・worktree・状態管理・ゲートは condukt バイナリが決定論的に担い、LLM は解釈・実装・検証に集中する。
argument-hint: [課題文]
allowed-tools: Task, AskUserQuestion, Bash(condukt:*), Bash(fugu-router:*), Bash(git:*), Read, Write, Edit
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
引数から課題文を取る (無ければ直前の会話の依頼)。`--dry-run` なら Phase 4 の schedule 提示で止める。

### Phase 1 — 解釈 (interpreter agent)
`Task` で `condukt-interpreter` 相当 (subagent_type を持たない環境では `Explore` を model:opus で)
を起動し、課題を **Decomposition JSON** にさせる。スキーマは `agents/condukt-interpreter.md` 準拠:
```json
{ "goal": "...", "tasks": [
  { "id": "t1", "title": "...", "touched_files": ["path/or/glob", ...],
    "deps": ["他タスクid"], "class": "parallel|serial|gated",
    "suggested_model": "sonnet|opus|haiku", "done_criteria": "検証で確認する合格条件" }
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

### Phase 4 — run 初期化
```
RID=$(condukt state init --file <json>)   # tasks=pending で run を作成、run id を返す
```

### Phase 5 — 並列実装 (batches を順に)
`schedule.batches` を**先頭から順に** 処理する (バッチ間は依存順、バッチ内は並列):
- バッチ内の各タスク `t` について:
  1. `WP=$(condukt worktree create --topic <t.id> --branch condukt/<t.id>)`
  2. `condukt state set --run $RID --task <t.id> --status running --worktree "$WP" --branch condukt/<t.id>`
  3. `Task` で `condukt-worker` 相当を起動 (model=`t.suggested_model`)。プロンプトに
     **作業ディレクトリ=$WP・触れてよいファイル=t.touched_files・done_criteria・「worktree 内で
     commit、merge はするな」** を渡す (子はこの会話の文脈を見られない)。
  4. 完了したら `condukt state set --run $RID --task <t.id> --status done`
- バッチ内は 1 メッセージで複数 `Task` を同時発行して並列化する。
- `serial` タスクは worktree に出さず main で順に実装し commit。

### Phase 6 — 検証 (verifier agent) + 実績の記録
done の各タスクを `condukt-verifier` 相当で done_criteria 照合する。検証する子の **model は
`<route.json>` の `verifier_model`**(worker と別ティアの独立検証。無ければ既定 sonnet)を使う。
pass なら `condukt state set --run $RID --task <id> --status verified`、fail なら `--status failed`
にし理由を控える。

検証後、**結果を fugu-router に記録**して次回のルーティングを賢くする (soft 依存):
```
fugu-router record --title "<task.title>" --files "<task.touched_files をカンマ区切り>" \
  --class <task.class> --model <worker に使ったモデル> \
  --status verified|failed --cost <gauge から取れれば>
```

### Phase 7 — 完了ゲート + 統合
```
condukt state gate --run $RID      # exit 0 まで完了宣言しない
```
- FAIL の理由 (未 verified / worktree 残置 / 未コミット) を解消する。`failed` タスクは理由を添えて
  Phase 5 に再投入 (失敗タスクだけ。2 サイクルで pass しなければユーザーに判断を仰ぐ)。
- 各 verified タスクの worktree を **自分の turn 内で** 閉じる:
  `condukt worktree merge --branch condukt/<id>` → `condukt worktree remove --path "$WP" --branch condukt/<id>`。
  最後に `condukt worktree cleanup` で orphan が無いことを確認。
- gate PASS で統合完了を報告 (タスク表 / 変更ファイル / 検証結果 / GATED の残提案)。

### Phase 8 — クローズ
`commit`/`push` はユーザー指示時のみ。GATED タスク (deploy 等) はユーザー承認を得てから別途実行。

## 失敗モード
- バイナリ不在 → README の導入手順を案内 (plugin install)。
- 子が共有ファイルに触りたがる → 分類ミス。serial 降格して main で実装。
- worktree 残置 → Phase 7 で必ず閉じる。`condukt state gate` が残置を検出する。
