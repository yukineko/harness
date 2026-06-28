## north_star
出荷物だけでなく『どの機会(opportunity)に賭けたか』を構造として持ち、measure した outcome を機会へ還元する。compass は north_star→gap→単一 solution-action だったが、その間に opportunity(顧客ニーズ/PDO の OST)層を挟み、discovery→solution→measure→learn を上流(機会)まで閉じる。

## definition_of_done
- active outcome 配下に opportunity が永続化され query 可能（compass の新コマンドが active outcome 下の opportunity を >=2 件 JSON で列挙でき、空ストアでは空配列を返す）
- condukt に渡る handoff(solution)が named opportunity ref を携える（compass route の to_condukt handoff テキストに、その solution が紐づく opportunity の識別子と見出しが印字される）
- compass gap が opportunity 別に gap を出せる（compass gap の出力 JSON が flat な単一 gap でなく opportunity ごとの gap 配列を含む）
- cargo test --workspace が全件 pass（既存の不変条件を壊さない）

## measuring_stick
私が今も擁護できるゴールに、測れるだけ近づくか（build より validate 寄り — 既存機能を壊さず、新機能は観測可能な改善として確認できること）。

## current_gap
compass は north_star→gap→単一 solution を直行し(route.rs)、その間に opportunity(顧客ニーズ)層が無い。hypothesis も flat list。最大の右サイズ差分は『active outcome 配下に opportunity を永続化する store + compass opportunity add/list コマンド』(DoD#1)。handoff への ref 印字(DoD#2)と opportunity 別 gap(DoD#3)はその上に乗る後続。まず store を validate-first で切る。

## next_action

## parked

