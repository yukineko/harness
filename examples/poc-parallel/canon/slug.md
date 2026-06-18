# canon: slugify

`src/slug.py` の module-level 関数 `slugify(s)`。文字列を URL slug に正規化する。

- **S-STR**: `str` を受け取り `str` を返す。
- **S-LOWER**: まず入力を小文字化する。
- **S-CHARSET**: 出力に許される文字は `[a-z0-9-]` のみ。
- **S-STRIP**: 許容集合外の文字のうち、空白でないもの (句読点・記号・非 ASCII を含む) は
  **除去する** (空文字に置換)。
- **S-SPACE**: 連続する空白 (Python の `\s`: 半角スペース・タブ・改行等。1 個でも) は
  単一の `-` に置換する。この置換は S-STRIP の除去の **後** に行う。
- **S-TRIM**: 先頭・末尾の `-` は取り除く (連続した `-` は 1 個に畳まない — 規則は
  S-SPACE と S-STRIP の合成で決まる)。
- **S-EMPTY**: 上記適用後に空になる入力 (例 `""`, `"   "`, `"!!!"`) は空文字 `""` を返す。

例 (逐語の期待値):
- `slugify("Hello, World!")` == `"hello-world"`
- `slugify("  Trim Me  ")` == `"trim-me"`
- `slugify("!!!")` == `""`
