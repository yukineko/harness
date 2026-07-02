## north_star
今の autonomy を「並列・無人でも壊れない土台」にする。scout・condukt・flow の autonomy 縮退は達成済み——次はその上で回るコードが、状態競合・LLM生成コードの実行・schema drift に対して安全であることを保証する。Devin 等は到達目標ではなく指標(参照点)であり、土台が堅くなった上で相性の良い良機能(サンドボックス実行・ランタイムFB・GitHub連携 等)を取捨選択して取り込む。

## definition_of_done
- condukt state の read-modify-write (pause_run・resume_run・StateAction Set) が file-lock で直列化され、2プロセス並列 RMW の lost-update 再現テストが green
- specguard forge の LLM生成 test_cmd が shell 実行前に blastguard の detect で検証され、破壊的コマンド payload を reject する単体テストが green
- condukt verify ゲートの skip_eligible 前提を検証する expect が graceful error に置換され、不変違反の入力で panic せず理由返却するテストが green
- orphan worktree cleanup が dirty state でも force 除去でき、次サイクルに孤児を残さないテストが green
- cargo test workspace 全 pass

## measuring_stick
擁護可能性 × ゴールへの接近距離 ÷ コスト

## current_gap
condukt state の並列 RMW が file-lock 無しで lost-update しうる(TOCTOU)。autonomy+worktree 並列を無人で回す前提が崩れる最大の穴。ここを塞ぐのが土台堅牢化の keystone。

## next_action
keystone: condukt state の RMW を file-lock で直列化する。backlog の既存 lock パターン(create_new atomicity)を踏襲し pause_run・resume_run・StateAction Set を保護、2プロセス並列 RMW の lost-update 再現テストを追加。size=m。後続(parked)で test_cmd 検証・expect fail-soft・orphan cleanup。

## parked
- LLM生成test_cmd の blastguard 検証(コマンド注入トラストバウンダリ)
- verify ゲート expect の fail-soft 化
- orphan worktree の force-cleanup
- (土台の上・指標を見つつ取捨選択) サンドボックス実行・ランタイムFB・GitHub連携・コードRAG・SWE-bench・cross-task学習

