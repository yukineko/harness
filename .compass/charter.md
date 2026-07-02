## north_star
phase-3=ランタイムFB自己デバッグ閉ループ。内ループ(計画→並列実行→ゲート→検証→観測)と phase-2 外界閉ループ(test還流FB + PR出口)は完成済。次は「対象を実起動して得た runtime シグナル(標準出力・標準エラー・exit code・panic や例外・health)を構造化して worker に還流し、同一 run 内で自己修正させる」ランタイム自己デバッグを閉じる。phase-2 DoD#1 の test還流の自然な深化。実起動は Docker や VM を導入せず既存 blastguard コマンド検証 と worktree 隔離の枠内に限定(サンドボックス項目はparked維持)。subscription-native・LLM↔決定論分離・never-break-a-turn を崩さず追加する。Devin 等の重量 sandbox・agent-native IDE・code RAG・外部ベンチは yardstick として参照するのみ。

## definition_of_done
- condukt が対象を実起動して runtime シグナル(標準出力・標準エラー・exit code・panic や例外メッセージ・health)を決定論的に構造化する純関数が存在し、それを worker 還流プロンプトに載せる経路がある。再現テスト: 起動時に非ゼロ exit か panic を出す対象で、還流プロンプトに verdict 以外の runtime 診断(exit code・標準エラー tail・panic メッセージ)が現れることを assert して green
- runtime シグナルの整形は Rust 決定論側で行い修正判断のみ LLM(worker)が担う分離をコードで確認でき、condukt の fmt と clippy が clean で既存テスト非回帰
- 実起動は Docker や VM を導入せず既存 blastguard コマンド検証 と worktree 隔離の枠内で行う(起動コマンドが blastguard を通り危険コマンドは拒否される再現テストが green)。対象不在・起動不能・タイムアウト時は fail-soft で turn を壊さず縮退する再現テストが green
- cargo test workspace 全 pass 維持

## measuring_stick
擁護可能性 × ゴールへの接近距離 ÷ コスト

## current_gap
phase-2 で FB 還流は test 出力(distill_failure→FailureDigest)まで閉じたが、還流されるのは『テストを走らせた結果』止まり。未閉塞は『対象を実起動して得る runtime シグナル』の還流: condukt verify は test コマンドを実行するが、対象アプリ/バイナリを起動して stdout・stderr・exit code・panic/例外・health を捕捉し worker に返す経路がゼロ(crates 全体で runtime health 還流の実装なし)。最大の梃子は DoD#1=『対象を起動し runtime シグナルを決定論的に構造化する純関数(distill_failure と対をなす distill_runtime 等)＋それを worker 還流プロンプトに載せる経路』——外部依存なし・既存 distill/verify 基盤と blastguard+worktree 隔離に自然に乗る size s〜m の ONE。起動安全境界(blastguard 検証・fail-soft 縮退)は DoD#3、determinism 分離は DoD#2。sandbox(Docker/VM)・code RAG・cross-task 学習・外部ベンチは取捨選択で parked。

## next_action

## parked
- #7 cross-task 学習: fugu-router episode store をタスク層へ拡張 (backlog 8086b5d0)
- #4 code RAG(埋め込み無し版): playbook 検索を構造スコアリング・deepwiki接地・シンボル索引で強化 (backlog 32739700)
- #1 サンドボックス実行 (backlog 3cd5ed15): Docker や VM は思想と乖離・blastguard+worktree が回答——見送り、無人ホスト損傷が現実化したら再訪
- #10 外部ベンチハーネス (backlog de758e5d): フレームワーク機能でなく計測プロジェクト——公開検証フェーズで
- foundation-hygiene: rebuild-plugins.sh が hooks と config も同期 (backlog 4a499fb9)

