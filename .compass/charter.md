## north_star
土台の上で『外界との閉ループ』を張る。内ループ(計画→並列実行→ゲート→検証→観測)は土台 DoD #1-#4 で完成済——次は agent が世界とやり取りする外ループを閉じる: (a) condukt verifier が走らせた test の構造化失敗出力を worker に還流し同一 run 内で自己修正させる runtime feedback 自己デバッグ閉ループ、(b) 変更を gh CLI で PR として着地させる VCS 出口(既存 GATED 承認の背後・APIキー不要)。subscription-native / LLM↔決定論分離 / never-break-a-turn を崩さず追加する。Devin 等の sandbox 実行・agent-native IDE・重量 code RAG・外部 SWE-bench は yardstick として参照するのみで今は見送り(取捨選択の結果)。

## definition_of_done
- condukt verifier の test 実行結果が pass/fail verdict だけでなく構造化された失敗出力(失敗テスト名・アサーション差分・output tail)として worker に還流され、worker が同一 run 内で再修正を試みる閉ループが存在する。テストが失敗→還流→worker 修正→green に至る再現テストが green (還流経路が存在し pass/fail 以外の診断情報が worker プロンプトに現れることを assert)
- test 失敗還流の整形は Rust(決定論)側で行われ修正判断のみ LLM(worker)が担う分離をコードで確認でき、condukt の fmt/clippy clean・既存テスト非回帰
- 変更を gh CLI で PR 起票する終端ステップが存在し、push/PR は既存 GATED 承認の背後にある(自律モードでも人間停止を維持)。gh 既存 auth を使い外部 API キー不要。gh 不在/未 auth 時は fail-soft でローカル commit 止まりに縮退し turn を壊さない再現テストが green
- cargo test workspace 全 pass 維持

## measuring_stick
擁護可能性 × ゴールへの接近距離 ÷ コスト

## current_gap
内ループ(計画→並列実行→11ゲート→検証→観測)は土台DoD#1-#4で完成・業界先行水準。未閉塞は外ループ2軸: (1)condukt verify は test を走らせるが worker に返るのは pass/fail verdict のみで失敗ログ/差分が還流されず自己デバッグ反復ができない。(2)harness はローカル commit で行き止まりで PR 起票=実世界の着地/merge率 出口が無い(crates全体で github/PR 参照ゼロ)。最大の梃子は(1)ランタイムFB自己デバッグ閉ループ——外部依存なし・determinism分離に自然に乗る size s〜m の ONE。(2)PR出口は次keystone。cross-task学習/code RAG/sandbox/SWE-bench は取捨選択で parked。

## next_action
keystone 候補(まだ未コミット・B/C を先に回す方針): condukt verifier→worker の還流経路に、test 失敗時の構造化診断(失敗テスト名・アサーション差分・output tail)を pass/fail verdict に加えて載せ、worker が同一 run 内で自己修正できるようにする。整形は Rust、修正判断は LLM。再現テスト: 意図的に落ちるテストを持つタスクで、還流プロンプトに診断情報が現れる(pass/fail 文字列以外の failure detail を assert)ことを green。size=s〜m。#3 PR 出口は次の keystone。

## parked
- #7 cross-task 学習: fugu-router の episode store をタスク層(どの分解・手が通ったか)へ拡張 (backlog 8086b5d0・相性◎だが外ループ2軸の後)
- #4 code RAG(埋め込み無し版): playbook のキーワード検索を構造的スコアリング/deepwiki接地/LSP・ctags シンボル索引で強化 (backlog 32739700・embedding 外部依存は思想と緊張のため回避)
- #1 サンドボックス実行 (backlog 3cd5ed15): Docker/VM は思想と最も乖離・blastguard+worktree隔離が harness の回答——見送り、無人ホスト損傷が現実化したら再訪
- #10 外部 SWE-bench ハーネス (backlog de758e5d): 能力主張の検証手段だがフレームワーク機能でなく計測プロジェクト——公開/検証フェーズで
- foundation-hygiene: hooks.json↔binary の Stop/SessionEnd 不一致 (backlog 632ea942)・budgetguard 日次リセット日付キー Utc::now() 整合バグ (本セッション B で対応予定)

