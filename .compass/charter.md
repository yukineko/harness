## north_star
PDO 前提の SOURCE 層(compass と scout)に『発見タスクのマシンスコープ共有レコード』を入れ、同一マシンで並行する別 session の compass や scout が互いの発見タスクと作業をかぶらせないようにする。発見時にレコードへ追記し、新たな発見は既存レコードと内容 fingerprint で突き合わせて重複を surface から外す。flow が一手を選択した後は、そのレコードを selected へ調整し、未選択ぶんは discovered のまま次回に再浮上する。判定(何を発見・選ぶ)は LLM、レコードの永続化と突き合わせ・ロックは決定論バイナリが担い、subscription で完結する。backlog lock(flow の直列化)と直交する『発見の重複回避』層を足す。

## definition_of_done
- compass と scout がタスクを発見したとき、マシンスコープの共有レコードに発見タスクを 1 件ずつ追記する: 内容 fingerprint・発見元 session id・status(discovered) を持つ行。書き込みは fail-soft(レコード不在なら新規作成、ロック競合や IO エラーで panic せず発見自体は継続)。
- 新たな compass か scout の発見が、既存レコードと内容 fingerprint が一致するタスクを『他 session が発見済み』として surface から外す。重複回避が観測可能: 2 session ぶんの発見を順に流すと 2 回目は重複タスクを surface しない integration test がある。
- flow が一手を選択したら、対応レコードの status を selected へ遷移させ、未選択の発見レコードは discovered のまま残る。選択後の調整(status 遷移)を検証する unit test を持つ。
- 後方互換 fail-soft: レコードが無い・空・破損のときは従来どおり全発見をそのまま surface し panic しない。レコード不在時の compass と scout と flow の挙動は従来と byte 等価。
- workspace の test 全 pass、clippy が D warnings で clean、fmt の check が clean。

## measuring_stick
私が今も擁護できるゴールに、測れるだけ近づくか(build より validate 寄り — 既存機能を壊さず、新機能は観測可能な改善として確認できること)。

## current_gap
発見タスクの重複回避レコードが存在しない: compass(次の一手+parked)も scout(施策→backlog)も発見時に他 session が見える永続レコードへ書かず、内容 fingerprint で既出を突き合わせる経路も無い。よって同一マシンの並行 session が同じ発見を independently に surface し作業がかぶる。flow も選択を発見レコードへ書き戻さない。最大かつ右サイズの gap: 発見タスクを追記する fail-soft なマシンスコープ・レコード + 内容 fingerprint による既出除外 + flow 選択時の status 調整。backlog lock とは別レイヤ(lock は実行の直列化、これは発見の重複回避)。

## next_action

## parked

