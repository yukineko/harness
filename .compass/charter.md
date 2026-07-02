## north_star
phase-4=失敗回復の真の再計画。phase-3 までで内ループ(計画→並列実行→ゲート→検証)と外界・runtime 還流の閉ループは完成。現在の失敗回復 cascade は model ティア昇格のみ(haiku→sonnet→opus で同一 decomposition を強いモデルで再試行)で、『タスクの切り方・アプローチ自体が誤り』の失敗を回復できない。phase-4 は cascade に真の再計画を加える: 失敗シグナルが decomposition の誤りを示すとき、model を上げるのではなく interpreter にタスクを再解釈・再分解(別アプローチ・別スコープ)させる経路を閉じる。replan するか model 昇格かの判定は Rust 決定論(失敗シグナルの分類)、再解釈自体は LLM(interpreter)。subscription-native・LLM↔決定論分離・never-break-a-turn を崩さず追加する。sandbox・code RAG・cross-task学習・外部ベンチは yardstick として参照するのみ(parked 維持)。

## definition_of_done
- 失敗した task の failure シグナル(verifier reason・failed_tests・escalation 履歴と到達ティア)から『model 昇格で足りる実装バグ型』か『decomposition の再解釈が要る型』かを決定論的に分類する純関数が condukt に存在する。再現テスト: opus まで昇格しても直らない型 か done_criteria とタスクスコープの不一致を示す reason を入力すると replan を、単純な実装バグ型は escalate_model を返すことを assert して green
- replan 判定時に、元タスクの failure_context を添えて interpreter に再分解を要求する handoff(プロンプト構成)を生成する経路があり SKILL Phase 6 のカスケードに配線されている。再現テスト: replan 経路が元 decomposition ではなく新規 decomposition を要求する構成(failure_context と別アプローチで再分解せよ指示)を出力することを assert して green
- replan か escalate かの判定は Rust 決定論側・再解釈(新 decomposition 生成)は LLM(interpreter) が担う分離をコードで確認でき、replan にも上限(無限ループ防止・例 最大1回)があり超過時は fail-soft でユーザーエスカレーションに縮退する再現テストが green。condukt の fmt と clippy が clean で既存テスト非回帰
- cargo test workspace 全 pass 維持

## measuring_stick
擁護可能性 × ゴールへの接近距離 ÷ コスト

## current_gap
現在の失敗回復 cascade(SKILL Phase6 + consensus.rs)は model ティア昇格のみ: verifier fail → failure_context 添付 → suggested_model を1ティア上げ(haiku→sonnet→opus)同一 decomposition を再試行。opus 到達 or 初回 opus で fail なら即ユーザーエスカレーション。crates 全体に『失敗が decomposition の誤りか実装バグかを分類する』ロジックも『再分解を要求する replan 経路』も存在しない(consensus.rs の escalate は常に opus 昇格を返すだけ)。最大の梃子は DoD#1=『failure シグナル(verifier reason・failed_tests・到達ティア)から escalate_model か replan かを決定論的に分類する純関数』——distill_failure/FailureDigest と同じ Rust 決定論側に自然に乗り、外部依存なし・size s〜m の ONE。replan handoff の生成配線は DoD#2、determinism 分離と上限/fail-soft 縮退は DoD#3。sandbox・code RAG・cross-task学習・外部ベンチは parked 維持。

## next_action

## parked
- #7 cross-task 学習: fugu-router episode store をタスク層へ拡張 (backlog 8086b5d0)
- #4 code RAG(埋め込み無し版): playbook 検索を構造スコアリング・deepwiki接地・シンボル索引で強化 (backlog 32739700)
- #1 サンドボックス実行 (backlog 3cd5ed15): Docker・VM は思想と乖離・blastguard+worktree が回答——無人ホスト損傷が現実化したら再訪
- #10 外部ベンチハーネス (backlog de758e5d): フレームワーク機能でなく計測プロジェクト——公開検証フェーズで
- foundation-hygiene: rebuild-plugins.sh が hooks と config も同期 (backlog 4a499fb9)

