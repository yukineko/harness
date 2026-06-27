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
計測ループの決定論コア達成: compass outcome が verdict(前進・不変・後退) を measured evidence 必須で記録し、gap.rs が last_outcome を surface（commit 4370feb, backlog 66d0968a done）。残り＝統合半分: flow/compass の sink が完了 move に対し compass outcome を自動呼び出しし、verdict を move の diff から LLM 判定する層（次スライス or 次 carve）。

## next_action

## parked

