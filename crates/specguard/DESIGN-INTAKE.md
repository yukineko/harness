# DESIGN: specforge intake — ソース横断の要件組成と "つめる" 入口ゲート

**過去 prompt・リポジトリ文書・Obsidian 記録から要件素材を組成し、厳格化できない点を
graded な閾値で検出して、推論で埋めず人間に同期で詰める入口レイヤの設計。**

このドキュメントは [DESIGN.md](DESIGN.md) の **§9-1（要望の入力経路 — 未確定）を閉じる拡張設計**。
DESIGN.md が「ゲートの *論理* は押さえたが *窓口* は要設計」（§10 入口）と残した部分を埋める。
新しい段は ①intake の手前と §5.3 Pre-flight rigor gate の内側に入り、**既存の ②normalize 以降は
無改造**。実装場所は specforge と同居（`crates/specguard/src/forge/`）、specguard は無改造で呼ぶ。

---

## 1. 動機と立ち位置

現 `specforge draft` の入力は `--req <file> + --canon <docs>` の**明示指定**。これは
「要望が既に1枚の文書に整っている」前提で、実運用の入口を満たさない。実際の要望は:

- **過去の prompt / 対話**（このリポジトリでの依頼の履歴）に散在し、
- **リポジトリ文書（canon）** に部分的に書かれ、
- **Obsidian の決定記録（AEGIS sessions / decisions）** に「なぜそう決めたか」が残っている。

intake レイヤは、これら3ソースを**決定的に収集 → provenance/authority を付与 → 厳格化に足りるかを
判定 → 足りなければ人間に同期で詰める**。出力は現 `draft` がそのまま食える「要望素材束 +
canon ポインタ」。つまり **`draft` の前段**であり、`draft` 以降の契約（§4 marker / §5 昇格ゲート）は
一切変えない。

```
[このレイヤ = 新規]                         [DESIGN.md = 既存・無改造]
 gather ─▶ pre-flight rigor (graded) ─▶ interrogate ループ ─▶ ② normalize ─▶ ③ ratify ─▶ …
 (3ソース)   G1–G4 を未達点に分解        AskUserQuestion で詰める   (要望→IR)    (昇格)
```

### 設計原則（DESIGN.md §設計原則を継承・崩さない）

intake は DESIGN.md の6原則をそのまま負う。特に本レイヤで効くのは:

- **原則5（矛盾は権威で自動解決しない）** — 後述 §3 の authority 順は **人間に詰めるときの
  tiebreak 助言**としてのみ使い、機械が「上位が勝つ」と潰さない。**矛盾は常に Ask**。
- **原則6（生成を目的化しない）** — 素材が足りなければ束をでっち上げず、不足を人間に返す。
- **判定は LLM・ハーネスは決定的** — 収集・スコアリング・provenance 付与・ゲート集約は決定的、
  「この素材が要件として接地するか」の判断のみ LLM。

---

## 2. 全体図（intake の内部）

```
                         ┌──────────────── 入口ゲート（同期）────────────────┐
 要望トピック/ID         │                                                   │
   │                     ▼                                                   │
   │   ┌─────────┐  provenance+authority   ┌──────────────┐  未達を質問化     │
   └──▶│ ① gather │──── 付き素材束 ────────▶│ ② pre-flight │──── open Q ──┐    │
       │ 3ソース  │                         │ rigor(graded)│              │    │
       └─────────┘                         └──────┬───────┘              ▼    │
            │                                     │ 全 G 通過    ┌──────────────┐│
            │ ソース不足                          │             │ ③ interrogate ││
            ▼                                     │             │  "つめる"ループ││
      needs_user(doc リクエスト・雛形)            │             └──────┬───────┘│
                                                  │                    │        │
                                                  ▼                    │解決     │
                                          [DESIGN.md ②normalize]◀──────┘        │
                                                  │           max_rounds 超過    │
                                                  │           or 人間 defer       │
                                                  └────────────▶ sentinel(離席)──┘
```

- **入口は同期**（DESIGN.md §10 が「§5.3 Pre-flight = 入口・同期的に止める」と既に許容）。
  中間（normalize 以降）は従来どおり機械自走 + 例外時 async pull。HOTL 不変条件は保たれる。
- interrogate が解けない残りは **sentinel に落として離席可**（hybrid）。同期一辺倒にしない。

---

## 3. ソースモデル — provenance と authority

gather は各素材片（fragment）に **出所と権威**を付ける。authority は **DESIGN.md §5.1 の権威階層を
具体ソースに割り当てたもの**で、用途は §5.1 のまま「**矛盾解決の自動化には使わない／人間に詰める
ときの既定 tiebreak 助言**」。

| authority | ソース | 既定パス | 役割 | DESIGN.md §5.1 対応 |
|---|---|---|---|---|
| **高** | Obsidian 決定 | `<vault>/AEGIS/decisions/`, `…/sessions/` | 確定した決定・理由 | User 正典に準ずる |
| **中** | repo 文書（canon） | `--canon` glob / `docs/**` | コードに最も近い仕様 | 生成ドキュメント上位 |
| **低** | 過去 prompt | `~/.claude/projects/<enc-cwd>/*.jsonl` | 意図のヒント（弱い） | draft / 弱い生成物 |

> `<enc-cwd>` は cwd を Claude Code がエンコードしたディレクトリ名（例: 本リポジトリなら
> `-mnt-c-Users-hiroyuki-nakayama-src-harness`）。transcript は会話ログなので **裏取りなしには
> 要件にしない**＝最弱。

**収集は決定的（語彙スコアリング、埋め込み無し）**。playbook / specguard の既存スコアリング思想を
流用する: トピック語 × triggers/tags/title/body の overlap（CJK は文字単位）。各 fragment は

```
fragment = { text, source_path, authority: high|mid|low, score, anchor }
```

として束ねる。**authority は「強さ」、score は「関連度」** で直交。提示順は (authority, score) 降順。

### 3.1 矛盾と authority（原則5 の厳守）

gather 段では矛盾を**解決しない**。pre-flight（§4）が矛盾を検出したら interrogate（§5）で人間に出す。
そのとき authority 順を **既定の選択肢（tiebreak 助言）として添える**が、機械は確定しない:

```
矛盾検出 ─▶ 「Obsidian決定X(高) は A、repo文書Y(中) は B。既定は X 優先だが、どちらを正とする？」
            └─ 人間が裁く（X / Y / 第三の答え / 文書を直す）── 自動では潰さない（§5.1）
```

これにより「つめる」が鋭くなる: **既定があるので人間は即答でき、覆したいときだけ覆す**。

---

## 4. 閾値 = graded rigor gate（machine floor + D2 監査）

DESIGN.md §5.3 の rigor gate（G1–G4）を **二値の pass/fail から「未達点の集合」へ graded 化**する。
「推論で十分でない閾値」を **再現可能な機械判定**に置く（採用: 機械 floor + D2 監査。LLM 自己申告
confidence は権威にしない）。

| ゲート | 判定 | 担当 | 未達 → 何を詰めるか |
|---|---|---|---|
| **G1 接地** | 各 acceptance 候補が canon に逐語引用で裏付く | LLM（D1 流用） | 「この条件の根拠 doc が無い」→ doc/値を要求 |
| **G2 沈黙ゼロ** | 決定点で canon が沈黙していない | LLM（**specguard D2**） | 「ここが未定義」→ 決定を要求 |
| **G3 矛盾ゼロ** | 要望・canon・各ソース間に矛盾なし | LLM（**specguard D2**） | 「X と Y が矛盾」→ §3.1 で裁定要求 |
| **G4 反証可能** | 各 criterion が観測可能な pass/fail | LLM（specforge 固有） | 「観測方法が不明」→ 測定基準を要求 |

- **floor（決定的）**: 各 requirement が acceptance を**1つ以上持つか**、canon 引用が**空でないか**、
  orphan（どの canon にも結びつかない）でないか — をハーネスが機械チェック（`ir.rs` の rigor 契約の
  延長）。LLM の `rigor:pass` 過大主張はここで棄却（§5.3 安全弁を継承）。
- **D2（LLM）**: 沈黙/矛盾/重複は specguard を**無改造で呼ぶ**。gather した素材束 + canon に対し
  Pre-flight として回す（DESIGN.md §5.3 の「Pre-flight = 新規の適用点」をソース束へ拡張）。

各未達は **1つの open question** に正規化される:

```
open_q = { gate: G1|G2|G3|G4, requirement_ref, gap: "...", sources: [fragment…], default: <authority最上位の値|null> }
```

**閾値 = open question が 0 件**。1件でも残れば interrogate へ（生成コストを払う前に止める）。

---

## 5. interrogate ループ — "つめる"

open question が残る間、**同期で人間に詰める**。DESIGN.md §10 が入口の同期介入を許すので、
これは HOTL 不変条件（中間は自走・境界は同期）に収まる。

```
while open_qs not empty and round < max_rounds:
    q = open_qs.next()                     # (authority, gate severity) 順
    ans = AskUserQuestion(q.gap, options=[q.default(あれば先頭/"推奨"), …, 自由入力])
    record_decision(q, ans)                # §6: 任意で Obsidian decisions へ書き戻し
    re-run pre-flight (§4)                  # 回答を反映して G1–G4 を再判定 → open_qs 更新
                                           #   → 新たな沈黙/矛盾が見えたら "さらに深い質問" が出る
defer or round==max_rounds → sentinel(§DESIGN.md 5.2) で残りを async pull（離席）
all cleared → ② normalize へ（素材束は厳格化の根拠が揃った状態）
```

**"どんどんつめる" の実体**は「回答 → 再 pre-flight → 新しい未達が浮上 → さらに問う」の反復。
1問ずつ詰めるたびに rigor を取り直すので、浅い回答は次のラウンドで深い質問を誘発する。

設計上の決め事:

- **質問は機械が生成しない、未達は機械が出す** — gap の検出（§4）は決定的/LLM 監査、問い方の
  文面は LLM が素材を引用して作る（断定せず「どちらが正か／何を測るか」を問う）。
- **既定（tiebreak）を先頭の "推奨" 選択肢に** — §3.1。即答可能にしつつ覆せる。
- **原則5 を破らない** — 矛盾系（G3）は必ず人間が裁く。機械は default を提示するだけ。
- **hybrid フォールバック** — `max_interrogation_rounds` 到達 or 人間 defer で sentinel に落とす。
  同期に縛り付けない（離席運用を殺さない）。`max_rounds = 0` で「同期せず全部 sentinel」= 現行
  非同期挙動に退化（後方互換）。

---

## 6. 決定の書き戻し（任意・HOTL ループの閉じ）

interrogate で人間が裁いた矛盾/不足は **次回の authority=高 ソースになるべき**（同じ問いを二度
詰めない）。回答を Obsidian `decisions/` に追記するオプションを持つ（既定 off、`record_decisions=true`
で on）。これは DESIGN.md ⑦ の `specguard decide`（理由を canon に pin）の intake 版で、
**人間の裁定がループに資産として残る**。書式は最小:

```markdown
# <date> 決定: <gap の一行>
- 文脈: <requirement_ref> / 衝突ソース <X(高)> vs <Y(中)>
- 裁定: <人間の回答>
- 理由: <あれば>
```

---

## 7. config 追加（specforge.toml）

```toml
[sources]
obsidian_vault = "~/aegis-obsidian"          # <vault>/AEGIS/{decisions,sessions} を走査
canon          = ["docs/**/*.md"]            # repo 文書（authority=中）
transcripts    = "~/.claude/projects"         # 過去 prompt（authority=低）。enc-cwd は自動解決
authority      = ["obsidian", "canon", "prompt"]  # 高→低（tiebreak 助言の順。自動解決はしない）

[gather]
top_k          = 24          # 束に入れる fragment 上限
min_score      = 1           # これ未満の関連度は捨てる

[rigor]
require_acceptance     = true   # floor: 各 requirement に acceptance 必須
require_canon_citation = true   # floor: canon 引用が空でない
run_d2_audit           = true   # G2/G3 を specguard D2 で監査
max_interrogation_rounds = 4    # "つめる"同期ラウンド上限。0 = 全部 sentinel(非同期/後方互換)
record_decisions       = false  # interrogate の裁定を Obsidian decisions に書き戻すか
```

---

## 8. 段階間契約への追加（DESIGN.md §4 互換）

新段も「人間可読本文 + 末尾 machine marker」契約に従う。既存 marker / exit は不変。

| 段 | marker | トレーラ | 欠落時 |
|---|---|---|---|
| ① gather | `<<<SPEC_GATHER>>>` | `bundle_path:` / `fragment_count:` | EXIT_NO_MARKER 相当、normalize へ進まない |
| ② pre-flight | `<<<SPEC_PREFLIGHT>>>` | `open_count:` / `needs_user:` | 同上（昇格しない） |

- gather/pre-flight が `needs_user: yes` または open_count>0 を返す間、**②normalize は起動しない**
  （生成コストを払う前に入口で止める＝§5.3 の趣旨）。
- exit code は DESIGN.md の予約（0/2/3/4/5/6）に未使用値を1つ足すのみ（例 `7` = intake 素材不足）。
  既存意味は不変。

---

## 9. DESIGN.md の未解決判断（§9）に対する解決状況

| §9 項目 | 本設計での扱い |
|---|---|
| **§9-1 入力経路・正規化トリガ** | **本ドキュメントで解決**: 3ソース gather（決定的収集）+ `specforge gather/draft` トリガ。自動/手動両対応（トピック ID を渡せば gather から、明示 `--req` なら従来どおり） |
| §9-2 acceptance 検証強度 | 範囲外（実装/証拠側、DESIGN.md §6.1 のまま） |
| §9-3 spec 再昇格 | 不変（intake は draft 前段なので ratify 規律に触れない） |
| §9-4 失敗 task 閾値 | 不変（実装段の話） |
| §9-5 決定ログ自動化 | §6 で intake 版を部分的に前倒し（裁定の Obsidian 書き戻し、既定 off） |

---

## 10. 未解決の設計判断（実装前に決める）

1. **transcript からの要件抽出の信頼度** — 過去 prompt は最弱ソース。どこまでを「ヒント」止まりに
   し、どこから canon 裏取りを必須にするか（既定: transcript 単独では G1 接地に使わない）。
2. **gather の関連判定** — トピック語をどう与えるか（ID / 自由文 / 直近 N prompt の自動要約）。
   playbook 流の語彙スコアで足りるか、軽い LLM 選別を1段挟むか。
3. **interrogate の粒度** — 1ラウンド1問（深掘り重視）か、独立な未達はバッチで一括提示か。
   既定は (authority,severity) 順の逐次だが、独立 gap はまとめて聞くと往復が減る。
4. **Obsidian 書き戻しの安全性** — `record_decisions=true` 時、vault への書き込みは specforge の
   隔離方針（DESIGN.md 原則2）とどう両立するか（decisions/ への追記のみに allowlist 限定）。
5. **段階導入** — DESIGN.md §8 に倣い、まず段階 C（Workflow PoC で gather→pre-flight→interrogate が
   1本で閉じるか実証）→ 安定部分を段階 B で `src/forge/{gather,preflight,interrogate}.rs` 化。

---

## 11. まとめ

- intake は **DESIGN.md §9-1（窓口未確定）を閉じる前段レイヤ**。②normalize 以降は無改造。
- **3ソース（Obsidian>repo>prompt）を決定的に gather** し、authority/provenance を付与。
- **閾値は machine floor + specguard D2** による graded rigor gate。未達を open question に分解。
- **"つめる" = 同期 interrogate ループ**（回答→再 pre-flight→深い質問）。入口の同期介入なので
  HOTL（中間自走・境界同期）を崩さない。max_rounds/defer で sentinel に落として離席も可。
- **原則5 を厳守** — authority 順は人間に詰めるときの tiebreak 助言であって、機械は矛盾を自動解決
  しない。「推論より人間に判断を求める」という要件は、この §5.1 規律と同根。
