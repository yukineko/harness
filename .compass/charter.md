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
DoD#1(opportunity store)達成。残 gap: condukt に渡る handoff(solution)が active outcome 配下の opportunity ref を携えていない(DoD#2)、gap も依然 flat で opportunity 別に出ない(DoD#3)。次の右サイズ一手は DoD#2: route の to_condukt handoff に active outcome 配下の opportunity(id+title)を印字し、solution が名前付き機会を携えるようにする。route.rs が opportunity::list_under を読み handoff に section を足す最小スライス。

## next_action

## parked

