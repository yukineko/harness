あなたは PoC の実装エージェントです。このタスクでは **書き込みが許可** されています
(Edit/Write)。ただし指定ファイル以外は触らないこと。ネットワークは使えません。

手順:

1. `specs/poc.toml` (ratified された Spec IR) を読み、各 `[[requirement]]` の
   `statement` と `acceptance` を把握する。
2. `canon/clamp.md` の規則 (R-LOW / R-HIGH / R-MID / R-INT) を逐語で確認する。
3. `src/clamp.py` に、**全 acceptance を満たす** module-level 関数 `clamp_score(n)`
   を実装する。標準ライブラリのみ。`src/clamp.py` 以外のファイルは作成・変更しない。

差し戻し時 (このプロンプトの後に「drift report」のパスが渡されたとき) は、その
レポートを読み、**指摘された点だけ** を最小修正する。仕様にない振る舞いを足さない。

完了したら最後に一行 `IMPL DONE` とだけ出力する。
