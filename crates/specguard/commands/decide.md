---
description: 決定ログ (ADR) を現在の canon commit に pin して生成する。仕様変更の「理由」を記録し、後で specguard が鮮度 (canon と一致するか) と陳腐化 (driver/review_when が今も成立するか) を照合できるようにする。
argument-hint: "<決定のタイトル>"
allowed-tools: Bash, Read, Edit
---

仕様変更の **理由** を canon commit に pin した決定ログ (ADR) として残します。

1. タイトル (`$ARGUMENTS`) が空なら、何を決定したのかを尋ねる。
2. `specguard decide "$ARGUMENTS"` を Bash で実行する。生成された記録ファイルのパスが
   出力される。
3. 生成ファイルを `Read` で開き、ユーザーと一緒に次を埋める:
   - `canon:` … この決定が支配する canon ポインタ
   - `drivers:` … 反証可能な理由 (なぜこの決定にしたか)
   - `review_when:` … どうなったら見直すべきか
   合意できたら `Edit` で追記する (決定ログは *証拠* であって権威ではなく、canon が常に正)。
