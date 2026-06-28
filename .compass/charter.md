## north_star
出荷物を記録するだけだった opportunity 層を、次の一手の順序を変える load-bearing な層にする。各 opportunity が持つ重み(outcome-impact または confidence を表す数値)が、compass が gap と handoff で並べる opportunity の順序を決定論的に駆動する。inert な記録層から、prioritization を実際に動かす層へ。

## definition_of_done
- opportunity に重み(weight: outcome-impact または confidence を表す数値)が永続化され、add 時に設定でき list と gap 出力に現れる。未設定はデフォルト重みになる(後方互換)。
- compass gap と route handoff が opportunity を重み降順で並べて出力する(同点は既存の決定論 tiebreak)。flat な記録順でなく観測可能なスコア順になる。
- 重みを変えると順序が変わることが unit test で確認できる(高 weight の opportunity が低 weight より前に並ぶことを assert)。
- cargo test --workspace が全件 pass(既存の不変条件を壊さない)。

## measuring_stick
私が今も擁護できるゴールに、測れるだけ近づくか(build より validate 寄り — 既存機能を壊さず、新機能は観測可能な改善として確認できること)。

## current_gap
opportunity 層は store/gap/handoff まで在るが、各 opportunity に重みが無く挿入順のまま=inert(何の順序も変えない)。最大かつ右サイズの gap は DoD#1: Opportunity に weight(outcome-impact/confidence を表す数値, 未設定はデフォルト)を持たせ、add で設定でき list と gap 出力に現れるようにする。これが DoD#2(重み降順ソート)と DoD#3(順序が変わる test)を解錠する基盤。次の一手 = opportunity.rs に weight フィールド+record 引数、main.rs opportunity add に --weight、list/gap 出力に weight を載せる最小スライス。

## next_action

## parked

