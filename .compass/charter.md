## north_star
toolguard truncate 完了 → nudge-cap・dispatch-update の parked 残件を処理し、toolguard の full cycle を完成させる

## definition_of_done
- nudge-cap: config.rs に toolguard_nudge_cap: u32 (default 3) が追加され、セッション内 tooldump 回数が上限を超えたら nudge を省略する
- dispatch-update: main.rs の Toolguard ブランチが ToolguardOutput.updated を updatedToolOutput に、.nudge を additionalContext に正しくマッピングしている（実装済）
- cargo test --workspace 全 pass

## measuring_stick
擁護可能性 × ゴールへの接近距離 ÷ コスト

## current_gap
nudge-cap（config.rs + toolguard.rs のセッション内反復キャップ）が未実装。dispatch-update は ef824f4 で完了済み。

## next_action
nudge-cap: config.rs に toolguard_nudge_cap を追加し、toolguard.rs の run() でセッション内 tooldump カウントが上限を超えたら nudge = None を返す（size=s）

## parked
- dispatch-update (ef824f4 で完了済み・削除可)

