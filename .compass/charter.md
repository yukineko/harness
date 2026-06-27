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
DoD1 着地: hypothesis が /flow の3つ目の source（crates/flow/skills/flow/SKILL.md + SessionStart directive）。DoD2 も証拠ゲートは着地済み。残る最大差分は2つ: (a) merge 時に linked hypothesis を awaiting-measurement にする status 追加（12c7726f, DoD2 残り）、(b) condukt の experiment タスククラス（366bf65d, DoD3）。次の右サイズ一手の主筋は (b) experiment class＝discovery を delivery と分けるループの最後のピース。

## next_action

## parked

