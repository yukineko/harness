## north_star
Claude Code 用 developer productivity プラグイン集。condukt・compass・session-insights・specguard 等の連携により、AI-assisted 開発の品質・可観測性・安全性・自律性を継続的に向上させる。

## definition_of_done
- 全プラグイン（condukt/compass/session-insights/specguard/gauge 等）が cargo test --workspace で緑
- 全プラグインが marketplace.json に登録済みで `/plugin install` から導入可能
- 主要フック（SessionStart/Stop/SessionEnd/PostToolUse）が settings.json に登録され実ターンで動作確認済み
- session-record SessionEnd フローが完了し AEGIS record note が自動生成される
- compass charter が設定済みで nudge が freshness を正しく報告する

## measuring_stick
私が今も擁護できるゴールに、測れるだけ近づくか（build より validate 寄り — 既存機能を壊さず、新機能は観測可能な改善として確認できること）。

## current_gap
session-record SessionEnd フロー（AEGIS record note 自動生成）が未完了。compass charter が未設定だったため nudge が機能していなかった。autoflow /backlog スキル連携の設計ギャップが露見。

## next_action
session-record-feature の未完了事項（gauge Usage/pricing の harness-core 昇格、session-insights SessionEnd record の動作確認）に着手する。

## parked

