---
description: 対応済みの specguard sentinel をクリアする。needs_user の指摘に人間が対応した後に実行し、SessionStart hook が同じ件で促し続けるのを止める。
allowed-tools: Bash
---

ユーザーが `needs_user=yes` の仕様ドリフト指摘に **対応し終えた** ことを前提に、
`specguard ack` を Bash で実行して sentinel をクリアしてください。出力をそのまま
報告する。まだ対応していない場合は実行せず、先に対応するよう促してください
(ack すると次回の監査でその drift が diff から外れ、検出漏れになりうるため)。
