# canon: clamp_score

`src/clamp.py` の module-level 関数 `clamp_score(n)`。スコアを閉区間 0..=100 に収める。

- **R-LOW**: `n < 0` → ちょうど `0`。
- **R-HIGH**: `n > 100` → ちょうど `100`。
- **R-MID**: `0 <= n <= 100` → `n` のまま。
- **R-INT**: 整数を受け取り整数を返す (float 変換しない)。
