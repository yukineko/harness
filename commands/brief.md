---
description: 着手前の仕様ブリーフィングを read-only で行う。タスクに関係する正典ルール・不変条件を逐語引用つきで思い出し、壊しやすい不変条件と着手前に確認すべき spec gap/矛盾を洗い出して、ドリフトを未然に防ぐ (run の事後監査に対する前線)。
argument-hint: "<着手するタスクの説明>"
allowed-tools: Bash, Task
---

これから着手するタスク (`$ARGUMENTS`) について、コードを書き始める **前に**
仕様ブリーフィングを行います。決定的な描画は `specguard` バイナリに委譲し、判定は
read-only subagent に委譲します (`claude --print` のサブプロセスは起動しない)。

1. タスク (`$ARGUMENTS`) が空なら、何に着手するのかを尋ねて停止する。
2. `specguard brief "$ARGUMENTS" --prompt` を Bash で実行し、描画されたブリーフィング
   プロンプトを取得する (非ゼロ終了なら stderr を提示して停止; config 不在なら
   `specguard init` を案内)。
3. 取得したプロンプトを **一字一句そのまま** 入力として、`Task` ツールで
   `specguard-auditor` subagent (read-only) を 1 体起動する。
4. subagent が返したブリーフ (Markdown) を **そのままユーザーに提示** する。`### 着手前に
   確認すべき論点` に項目があれば、それを `AskUserQuestion` で人間に確認してから着手する
   よう促す (勝手に仮定して進めない)。

このコマンドは read-only で、コードも doc も書かない。spec gap/矛盾が見つかったら、必要に
応じて `/specguard:decide` で決定ログ化することを案内してもよい。
