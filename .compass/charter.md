## north_star
phase-5=replan 経路の実運用計測(build ≠ validate)。phase-4 で失敗回復の真の再計画(classify_failure→decide_replan→handoff, cap+fail-soft)を*実装*したが、実タスクで一度も発火・検証されていない——純関数の lib test(29件)が入出力を assert するだけで、「実行時に replan が実際にトリガーされ、model 昇格では回復できない decomposition 誤り型の失敗を再分解で回復する」証拠が無い。phase-5 は計測ループを閉じる: (1)replan 決定を実行時に構造化記録して実運用で可観測にし(決定論側)、(2)decomposition 誤りを意図的に起こす end-to-end fixture で「escalate では直らないが replan なら直る」回復を実証する(LLM 判断は再分解のみ)。可観測化・実証は Rust 決定論側、再解釈は LLM(interpreter)。subscription-native・LLM↔決定論分離・never-break-a-turn を崩さず追加する。sandbox・code RAG・cross-task学習・外部ベンチは yardstick として参照するのみ(parked 維持)。

## definition_of_done
- decide_replan が実行時に走るたびに、その 3-way directive(escalate_model/replan/escalate_to_user)と根拠(分類 reason・到達ティア・replan_count)を構造化記録する経路が condukt にある(state もしくは tracekit・外部依存なし)。再現テスト: 実行を模した入力列を流すと各 replan 決定が記録され、後から「発火した/しなかった/縮退した」を集計・観測できることを assert して green
- 「model 昇格では回復不能だが replan なら回復する」失敗を再現する end-to-end fixture がテストに存在し、decide_replan がそれを replan と分類→再分解 handoff→再分解タスクが pass、という F→(escalate fail)→(replan)→P の遷移を実証する再現テストが green(既存 fp_oracle_e2e と同じ tests/ 層・純関数 lib test の再掲ではない)
- replan が発火しなかった(escalate_model)ケースと上限超過で fail-soft 縮退した(escalate_to_user)ケースも観測記録に区別可能な計測値として現れる。condukt の fmt と clippy が clean で既存テスト非回帰
- cargo test workspace 全 pass 維持

## measuring_stick
擁護可能性 × ゴールへの接近距離 ÷ コスト

## current_gap
phase-4 で replan の純関数群(classify_failure/decide_replan/build_replan_handoff・cap+fail-soft)と `replan handoff` CLI(main.rs:600-619)・SKILL Phase6 カスケード配線は入った。しかし (a)実行時に replan 決定を記録する経路が state.rs にも tracekit にも無く(grep 空)、「実際に発火したか・何回 replan したか・fail-soft に縮退したか」を後から計測できない。(b)「model 昇格では直らないが replan なら直る」ことを示す end-to-end 証拠が無い(tests/ には fp_oracle_e2e.rs / autonomy_invariant.rs のみで replan e2e は不在。29 replan tests は src/replan.rs の純関数 lib test で、実行時の発火も回復も通していない)。最大の梃子は DoD#1=replan 決定の構造化記録(可観測化)——tracekit span / state と同じ決定論側に自然に乗り、外部依存なし・size s〜m の ONE。end-to-end 回復実証が DoD#2、発火/不発火/縮退の区別が DoD#3。sandbox・code RAG・cross-task学習・外部ベンチは parked 維持。

## next_action
DoD#1: decide_replan の 3-way directive を実行時に構造化記録する経路(state もしくは tracekit)を condukt に追加し、後から発火/不発火/縮退を集計できる再現テストを green にする。

## parked
- #7 cross-task 学習: fugu-router episode store をタスク層へ拡張 (backlog 8086b5d0)
- #4 code RAG(埋め込み無し版): playbook 検索を構造スコアリング・deepwiki接地・シンボル索引で強化 (backlog 32739700)
- #1 サンドボックス実行 (backlog 3cd5ed15): Docker・VM は思想と乖離・blastguard+worktree が回答——無人ホスト損傷が現実化したら再訪
- #10 外部ベンチハーネス (backlog de758e5d): フレームワーク機能でなく計測プロジェクト——公開検証フェーズで
- foundation-hygiene: rebuild-plugins.sh が hooks と config も同期 (backlog 4a499fb9)
