# 段階C PoC — 仮説3: worktree 並列実装 + 監査付き逐次 merge (DESIGN.md §6)

`poc-loop/`(単一 task で生成→監査ループが閉じることを実証)の続き。こちらは **複数 task を
git worktree で並列実装し、各 task を ⑥ 監査が通って初めて merge する** ことを実証する。

```
②③ 各エリアを draft+ratify  →  ⑤ 各 task を別 worktree で並列 impl
   →  ⑥ worktree 単位で specguard 監査  →  通った task だけ逐次 merge
```

2 つの **互いに素なエリア**(`src/clamp.py` / `src/slug.py`、1 エリア=1 ファイル)を使うので、
並列 task が同じファイルを触らず merge 衝突が原理的に起きない(DESIGN.md §6: area 境界で task を
切る)。

## 構成

| ファイル | 役割 |
|---|---|
| `canon/clamp.md`, `canon/slug.md` | 2 エリアの正典 |
| `req-clamp.md`, `req-slug.md` | 2 エリアの要望 |
| `specforge.toml` | ②③ 用 |
| `specguard.toml` | ⑥ 用(**2 area**、各 1 ファイル) |
| `impl-prompt.md` | 「自分の担当ファイルだけ実装」する task エージェント |
| `run-parallel.sh` | 並列ドライバ(worktree 並列 + 監査 + merge) |

## 実行

```sh
./run-parallel.sh --dry   # 配線スモークテスト(LLM 不要)
./run-parallel.sh         # 本番(`claude` を task 数だけ並列に呼ぶ)
```

## 結果

✅ **仮説3 実証:** clamp と slug を **2 つの worktree で並列実装** → 各々 ⑥ 監査クリア →
**衝突なく逐次 merge** → merged tree の 2 エリア監査も「修正候補なし」で収束。

```
merged (parallel, audited, conflict-free): clamp slug
held (failed audit, 差し戻し対象):          none
files in merged tree: src/clamp.py src/slug.py
✅ all tasks merged conflict-free and the merged tree audits clean.
```

### 副産物 — §5.2 escalation が実エージェントで発火

初回 run で **slug の normalize が rigor fail(G2 沈黙)で escalate** した。初版 `canon/slug.md`
は「URL に使える slug」と書きながら **許容文字集合・句読点/非 ASCII の扱いを定義していなかった**。
normalize エージェントは draft をでっち上げず、不足(S-CHARSET / S-STRIP / S-UNICODE / 退化入力)を
**逐語で指摘し、人間が埋める雛形まで提示**して停止した。canon を厳格化(S-CHARSET ほかを追記)して
再 run すると draft 化に成功した。**「生成を目的化しない」原則(DESIGN.md §5.2)が実エージェントで
機能することの実証**でもある。

## これで段階C の3仮説が揃った

| 仮説 | 実証 |
|---|---|
| 1 normalize が反証可能 acceptance に落とす | ✅ `poc-loop/`(+ 本 PoC の clamp/slug) |
| 2 ⑥D1 が task 単位で drift 判定 | ✅ `poc-loop/`(収束)+ `--drift`(検出+差し戻し) |
| 3 worktree 並列が衝突なく merge | ✅ 本 PoC |

ループが閉じ、3 仮説が揃ったので、安定部分(まず ④prompt-build)を **段階B で Rust 化**できる。

## 限界

- ③ratify は自動承認(PoC)。本来は人間の合意ゲート。
- 証拠は ⑥ の逐語監査のみ(テスト実行・§6.1 は未配線)。
- task 数は固定(`TASKS=(clamp slug)`)。spec の requirement から動的に task を起こすのは
  段階B(④prompt-build)の仕事。
