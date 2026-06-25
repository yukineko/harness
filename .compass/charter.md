## north_star
Claude Code 用 developer productivity プラグイン集。condukt・compass・session-insights・specguard 等の連携により、AI-assisted 開発の品質・可観測性・安全性・自律性を継続的に向上させる。

## definition_of_done
- cargo test --workspace が全件 pass する
- 全プラグインが plugin リストに表示され、plugin install コマンドから導入可能
- session-insights が SessionEnd フックでスケルトンノートを自動生成し、record スキルで散文を記入するフローが完成する

## measuring_stick
私が今も擁護できるゴールに、測れるだけ近づくか（build より validate 寄り — 既存機能を壊さず、新機能は観測可能な改善として確認できること）。

## current_gap
session-record の SessionEnd 自動生成フローが未完了（harness-core への pricing/usage 昇格 → session-insights SessionEnd フック強化 → 動作確認）。cargo test --workspace による全件 pass の確認も未実施。

## next_action
harness-core に pricing/usage モジュールを昇格する（gauge の transcript.rs・pricing.rs を harness-core へ移動し、gauge はそれを参照するようにリファクタ）

## parked

