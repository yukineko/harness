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
PDO-loop charter DoD 達成: DoD1(hypothesis=flow source)・DoD2(awaiting-measurement on merge + 証拠必須 validate/reject)・DoD3 core(experiment class が merge 経路から除外)・DoD4(cargo test --workspace 664 pass)。ループは閉じた: discovery(hypothesis)→flow→condukt→merge→awaiting-measurement→(計測)→validate/reject。残りは強化のみ: 66d0968a(成果を measuring_stick で判定・記録, l)・2b18a458(experiment findings/discard, p2)・p2 architecture(OST/input metrics/dual-track/scoring/cadence)。次サイクルは PR マージ後に /compass で次ゴール定義 or 強化項目を継続。

## next_action

## parked

