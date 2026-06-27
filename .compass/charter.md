## north_star
完了した一手の成果を compass measuring_stick で judge し、その判定を記録して次の gap に反映する。build より validate に寄せ、計測ループ（awaiting-measurement から validated か rejected まで）を閉じる。

## definition_of_done
- move 完了後の成果を measuring_stick で判定する step を compass が持つ（ crates/compass/src/route.rs で handoff に書くだけだった measuring_stick を、新しい outcome 記録経路が読み戻して 前進・不変・後退 を判定する）
- 判定結果が永続化され次サイクルの gap に反映される（ crates/compass/src/gap.rs が記録済み outcome を読み、 compass gap の出力に直近 move の判定を含める）
- outcome 記録は measured evidence を必須とする（証拠なしの記録は失敗する＝ build より validate）
- cargo test --workspace が全件 pass（既存の不変条件を壊さない）

## measuring_stick
私が今も擁護できるゴールに、測れるだけ近づくか（build より validate 寄り — 既存機能を壊さず、新機能は観測可能な改善として確認できること）。

## current_gap
計測ループの『判定』半分が欠落。route.rs:320 は measuring_stick を handoff に書くだけで、move 完了後にそれを読み戻して『前進・不変・後退』を判定し記録する経路が無い（route.rs:320-323 = written-only, never consumed）。最大差分: outcome を measured evidence 付きで記録し gap.rs がそれを次サイクルに surface する仕組みを新設する。

## next_action

## parked

