# 段階C PoC — 生成→監査ループが閉じるか (DESIGN.md §8)

`specforge`(②normalize + ③ratify) と `specguard`(⑥drift 監査)を **バイナリのまま** shell で
繋ぎ、未実装の ④prompt/⑤impl ギャップを **書き込み可エージェント** で埋めて、

```
要望 → ② draft(normalize+rigor) → ③ ratify → ④⑤ implement → ⑥ audit → 収束 / 差し戻し
```

が **1 本で閉じるか** を実証する PoC。Rust を増やさず、決定的ハーネス(この driver + 2 バイナリ)
と LLM 判定(normalize / implement / audit)の分担を保ったまま検証する(DESIGN.md の段階 C)。

## 構成

| ファイル | 役割 |
|---|---|
| `requirement.md` | 粒度の粗い要望(人間が書く想定) |
| `canon/clamp.md` | 要望が接地する正典(反証可能な規則 R-LOW/HIGH/MID/INT) |
| `specforge.toml` | ②③ 用 config(normalize は read-only agent) |
| `specguard.toml` | ⑥ 用 config(`src/**` ↔ `canon/clamp.md`) |
| `impl-prompt.md` | ④⑤ ギャップを埋める実装エージェントのプロンプト(書き込み可) |
| `run-poc.sh` | ループを駆動する決定的ドライバ |

## 実行

```sh
./run-poc.sh --dry   # 配線スモークテスト(config 解析・prompt 描画・scope 解決のみ。LLM 不要)
./run-poc.sh         # 本番ループ(`claude` を呼ぶ — トークン消費)
```

毎回 `mktemp -d` の使い捨て git repo に scaffold して走るので、この repo は汚さない。

## 検証する仮説(DESIGN.md §8)と結果

| # | 仮説 | 結果 |
|---|---|---|
| 1 | ②normalize が粗い要望を **反証可能な acceptance** に落とせるか | ✅ R1(clamp 範囲)+ R2(int 型)を、各々 acceptance + canon ポインタ + `falsifiable=true` で生成 |
| 2 | ⑥D1 が task 単位で drift を正しく判定するか | ✅ 正しい実装に対し「修正候補なし」→ 0 差し戻しで収束 |
| 3 | worktree 並列が merge 衝突なく回るか | ⏸ 未検証(本 PoC は単一 requirement で in-place 実装。複数 task の worktree 並列は次の拡張) |

**初回実行で実バグを 1 件発見・修正:** normalize エージェントは「TOML だけ出力」の指示に
反して判定理由のプロローグを前置きし、`AgentDraft::parse` が body 全体を TOML として
パースして失敗した。ハーネスを **「フェンス内の requirement TOML を抽出」** する方式に修正
(`ir::extract_requirement_toml`: ```` ```toml ```` フェンス → bare `[[requirement]]` 以降 →
全体、の順にフォールバック)し、normalize プロンプトにフェンス必須を明記した。**これは PoC の
主目的そのもの** — 実エージェントの揺れに対するハーネスの脆さを、安く実環境で炙り出すこと。

## 限界 / 次の拡張

- **単一 task・in-place 実装**。複数 requirement → worktree 並列(DESIGN.md §6)+ 監査付き
  逐次 merge は未実装(仮説3)。
- **③ratify を自動承認**している(PoC のため)。本来は人間の合意ゲート(HOTL)。
- **証拠は D1 逐語監査のみ**。テスト実行・UI レンダ(DESIGN.md §6.1 の artifact-typed
  evidence の実行系)は未配線。
- 差し戻しは最大 `MAX_FIX=2` 回。収束しなければ「未収束」を報告して止まる(無限ループ防止)。

これらは段階 C で「ループが閉じること」を実証した上での、段階 B(Rust 化)で固める対象。
