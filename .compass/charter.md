## north_star
Claude Code 用 developer productivity プラグイン集。condukt・compass・session-insights・specguard 等の連携により、AI-assisted 開発の品質・可観測性・安全性・自律性を継続的に向上させる。

## definition_of_done
- cargo test --workspace が全件 pass する
- 全プラグインが plugin リストに表示され、plugin install コマンドから導入可能
- session-insights が SessionEnd フックでスケルトンノートを自動生成し、record スキルで散文を記入するフローが完成する

## measuring_stick
私が今も擁護できるゴールに、測れるだけ近づくか（build より validate 寄り — 既存機能を壊さず、新機能は観測可能な改善として確認できること）。

## current_gap
全 DoD 達成済み。cargo test --workspace 全件 pass・全プラグイン導入可能・session-insights SessionEnd 自動生成フロー完成（record = true 確認）。次の拡張課題の洗い出しが未実施。

## next_action
compass で次の拡張ゴールを定義する（/compass で north_star を更新し新たな gap を導出）

## parked

