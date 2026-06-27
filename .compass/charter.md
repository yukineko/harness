## north_star
discovery(hypothesis) → flow → condukt → measure → learn のループを閉じ、harness を「build するだけ」から「validate する」基盤へ引き上げる。仮説を一級の source とし、実行結果を計測された学びとして書き戻す。

## definition_of_done
- hypothesis が flow の3つ目の source として機能する（引数なし flow 実行が open hypothesis を課題候補として surface し、 crates/flow/skills/flow/SKILL.md に source として明記される）
- 完了した一手の成果が計測として記録される（merge 時に linked hypothesis が awaiting-measurement になり、 crates/hypothesis/src/store.rs の validate と reject が計測証拠を必須とする＝証拠なしの validate は失敗する）
- condukt が experiment タスククラスを持つ（ crates/condukt/src/model.rs の Class enum に experiment が加わり、experiment タスクは auto-merge されず findings を記録する）
- cargo test --workspace が全件 pass（既存の不変条件を壊さない）

## measuring_stick
私が今も擁護できるゴールに、測れるだけ近づくか（build より validate 寄り — 既存機能を壊さず、新機能は観測可能な改善として確認できること）。

## current_gap
DoD1(flow source)・DoD3(experiment class core) 着地。DoD2 は証拠ゲート着地済み・残りは計測の write-back。残る最大差分は DoD2 の計測ループ閉じ: (a) AwaitingMeasurement status を merge 時にセット（12c7726f, m, enum+CLI verb+condukt 呼び出し）, (b) 完了した一手の成果を measuring_stick に対して判定・記録（66d0968a, l, 設計重め）。次の右サイズ主筋は (a)＝より具体的。experiment の findings/worktree-discard(2b18a458) は後続。

## next_action

## parked

