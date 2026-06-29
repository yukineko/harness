## north_star
I6 で観測可能になった size 圧(ledger の resident_tokens=ウィンドウ占有)を、size レバー(groom・inject・snapshot)の制御入力へ昇格させる。観測するだけの台帳から、観測値が次のレバー決定を駆動する閉ループ(observe→act)へ。confidence が分割を、weight が順序を、outcome trend が pivot を駆動したのと対称に、観測した size 圧が『どれだけ刈り込むか』を駆動する。最初の最も対称な一手は groom 予算を固定キャップから window-pressure-aware にすること(観測→制御の最小スライス)。

## definition_of_done
- DefaultGroomer の groom 予算が固定値ではなく、観測された window pressure(resident 占有 または 明示の pressure 入力)の決定論関数になる。pressure が高いほど予算が小さくなる単調関数で、pressure 入力が無いかゼロのときは現行の既定 budget に一致する(後方互換)。
- pressure から budget への写像が unit test を持つ: 高 pressure で budget が縮む、ゼロや欠損で既定値、単調性(pressure 増で budget 非増)。
- 観測ソースが配線される: groomer が pressure を観測値(ledger か state、もしくは HookInput)から読み、その値で予算を決める経路が存在する(pure な budget 関数 と それを呼ぶ to_output)。
- cargo test --workspace 全 pass、clippy -D warnings clean、cargo fmt --check clean。frozen な契約モジュール(types・handlers・io)は byte 不変に保つ。

## measuring_stick
私が今も擁護できるゴールに、測れるだけ近づくか(build より validate 寄り — 既存機能を壊さず、新機能は観測可能な改善として確認できること)。

## current_gap
groomer の予算は固定キャップ(groom_budget() が CONTEXT_GOVERNOR_GROOM_BUDGET env か既定 2048 を読むだけ)で、観測された window pressure に盲目。I6 台帳は resident_tokens を記録するようになったが、それを読み戻して挙動を変える経路が無い=observe without act。最大かつ右サイズの gap は DoD#1-#3: pure な単調関数 budget_for(pressure, default)(高 pressure で縮む・ゼロ/欠損で既定)+ unit test、to_output がその関数で予算を決める配線。parked は他2レバーの observe→act スライス(反復 injection の dedup、巨大ツール出力の truncate)。

## next_action
crates/context-governor/src/defaults/groomer.rs に pure な pressure→budget 単調関数 budget_for(pressure, default) を追加(高 pressure で縮む・ゼロや欠損で既定値・単調)し unit test を付け、to_output が観測 pressure からその関数で予算を決める経路を配線する。frozen モジュール(types・handlers・io)は不変、workspace test/clippy/fmt green。

## parked
- context-governor: 反復する per-turn reference injection を seen-state で dedup する(injector の observe→act スライス, backlog 442f03c0)
- context-governor/ctxrot: 巨大ツール出力を nudge だけでなく truncate する(+反復 nudge を cap)(groomer/guard の observe→act スライス, backlog 8ebfa49c)

