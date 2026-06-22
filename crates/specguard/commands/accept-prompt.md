---
description: 監査プロンプト (メタ正典) を批准して pin する。内容を確認・合意したうえで理由を添えて実行すると、契約チェック (必須 placeholder) を通し fingerprint・canon commit・理由を lock に記録する。require_ratification 有効時に run の前提となる。
argument-hint: "<批准する理由>"
allowed-tools: Bash, Read
---

これは **批准 (consent) の儀式** です。監査プロンプト = 「何を drift とみなすか」を
決めるメタ正典であり、批准は人間の責任で行います。安易に実行しないでください。

1. まず批准対象のプロンプトテンプレートをユーザーが確認できるよう、必要なら
   `templates/audit-prompt.md` 等を `Read` で提示する (specguard.toml の `[prompt].template`
   指定があればそちら、無ければ埋め込み既定。verify ゲート有効時は refute/completeness も)。
2. ユーザーが内容に合意し、理由 (`$ARGUMENTS`) を示したら
   `specguard accept-prompt -m "$ARGUMENTS"` を Bash で実行する。理由が空なら実行せず、
   理由を尋ねる。
3. 契約違反 (必須 placeholder 不足) で拒否されたら、stderr の不足項目をユーザーに伝える。
   成功したら pin された内容 (canon commit / pin したポリシー / 理由) を報告する。
