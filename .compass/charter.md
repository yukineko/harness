## north_star
ideate→implement→verify (scout→backlog→flow→condukt) を人間0介入で完走できる autonomy モードを設ける。停止は (a) 情報不足=worker blocked と (b) 外向き不可逆=deploy/push 承認 の2種のみ。それ以外の AskUserQuestion(機械的承認＋意図的HOTL不変)は autonomy 有効時に決定論的既定へ縮退させる。

## definition_of_done
- 共通の autonomy スイッチ (各バイナリが読む autonomous:bool config か env) が存在し、無効(既定)時は現行の全 AskUserQuestion が従来どおり出る(後方互換)
- autonomy 有効時: scout Phase4 選別 Ask を省き top-N を auto-queue する経路がある
- autonomy 有効時: condukt Phase3 分解合意 Ask を省き schedule 結果をそのまま採用する経路がある
- autonomy 有効時: flow の pivot-check=auto-persevere / lock競合・resume選択・連続失敗 を決定論的に解決し AskUserQuestion を出さない
- autonomy 有効時に残る停止は worker blocked と deploy/push GATED 承認の2種のみ (テストまたは skill 監査で確認)
- e2e: 1つの scout 施策が人間0介入で backlog→condukt実装→verify done まで到達する実証手順が green
- cargo test --workspace 全 pass

## measuring_stick
擁護可能性 × ゴールへの接近距離 ÷ コスト

## current_gap
全13 human-gate が今も無条件 AskUserQuestion を出す。autonomy を選択的に縮退させる共通スイッチ機構が未実装で、各バイナリ(scout/condukt/flow)は「今 autonomous か」を決定論的に読む手段を持たない。最大の関門は condukt Phase3 分解合意——ここを縮退できれば scout選別・flow pivot も同じ縮退パターンを再利用できる。keystone = autonomy スイッチ + condukt Phase3 auto-agree の最小スライスで縮退パターンを1本 validate すること。

## next_action
keystone: condukt に autonomy スイッチを追加する。(1) condukt config に autonomous:bool (default false) + env override、(2) 既存 condukt state check-criteria に倣い condukt state autonomy-check サブコマンドで skill が決定論的に読めるようにする、(3) autonomous 有効時のみ condukt SKILL.md Phase3 の分解合意 AskUserQuestion をスキップし schedule 結果をそのまま採用、無効時は従来どおり、(4) 分岐を検証するテスト。size=m。scout auto-queue と flow pivot は同パターンで後続。

## parked
- scout Phase4 auto-queue (autonomy 有効時に top-N を選別Ask無しで backlog add)
- flow autonomy: pivot auto-persevere / lock競合・resume・連続失敗 の決定論解決
- e2e 実証手順: scout施策1件を人間0介入で done まで通す検証

