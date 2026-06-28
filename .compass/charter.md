## north_star
outcome trend を load-bearing にする。単一 verdict を記録するだけの層から、蓄積した outcome 履歴（末尾の連続した unchanged または backward の streak）を決定論的に集計し、north_star レベルの pivot（方向転換）か persevere（継続）かの勧告を駆動する層へ。confidence や weight が順序を駆動したのと対称に、outcome trend が『方向転換するか継続するか』の決定を駆動し、その決定を記録して次サイクルに読み戻す。

## definition_of_done
- crates/compass/src/outcome.rs が、記録済みの outcome 履歴から末尾の連続した unchanged または backward の streak を集計する決定論関数を持つ。streak が閾値（既定 3）以上なら pivot、さもなくば persevere を、集計理由（streak 長・対象 verdict 列・参照した最後の forward の seq）つきで返す。履歴が空、または末尾が forward なら persevere（後方互換）。
- その勧告が CLI から観測できる。compass pivot-check が recommendation（persevere か pivot）と streak と reason を JSON で stdout に出し exit 0 で終わる。
- streak 集計と閾値判定の unit test を crates/compass/src/outcome.rs に追加する。末尾 forward なら persevere、閾値マイナス1の連続 unchanged なら persevere、閾値ぶんの連続 backward なら pivot、途中に forward が挟まると streak がリセットされ persevere。cargo test --workspace 全 pass、clippy -D warnings clean、cargo fmt --check clean。
- crates/flow/skills/flow/SKILL.md の Step 4（loop 終端）が compass pivot-check を consume し、pivot 勧告時は集計理由を引用して north_star を彫り直すか（再オリエンテーション）を promptし、persevere ならそのまま継続する手順が明記される。これで outcome trend が store から CLI そして flow の決定まで実際に流れ inert にならない。

## measuring_stick
私が今も擁護できるゴールに、測れるだけ近づくか(build より validate 寄り — 既存機能を壊さず、新機能は観測可能な改善として確認できること)。

## current_gap
outcome 層は単一 verdict を記録するだけ(crates/compass/src/outcome.rs の record/latest は最新1件のみ、trend/集計なし)で、連続 unchanged/backward の streak が pivot 判断を駆動しない=inert。confidence/weight が順序を駆動したのと対称な最大かつ右サイズの gap は DoD#1-#3: outcome.rs に末尾 streak 集計+閾値(既定3)で pivot/persevere を理由つきで返す決定論関数、compass pivot-check CLI で JSON 観測、streak/閾値の unit test。次の一手 = outcome.rs の集計関数+test と main.rs の pivot-check サブコマンド。DoD#4(flow SKILL Step4 の consume)は parked で次スライス。

## next_action

## parked

