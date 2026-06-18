# DESIGN: specguard 検証ゲート — findings の反証 + 網羅性批評 (監査側ハーネス)

**監査が出した findings を額面で信じず、独立 agent で *反証* (偽陽性除去) し、
*網羅性批評* (偽陰性発掘) して初めて人間に上げる、検証の閉ループの設計。**

`specguard` 本体 (`src/main.rs` の `run`、`templates/audit-prompt.md`) の思想と契約を
そのまま継承し、不足している **findings 自体の検証ループ** を定義する。生成側
(specforge) の設計は別途 [DESIGN.md](DESIGN.md)。

> **実装状況 (2026-06-18):** V1 反証ゲート + V2 網羅性批評 + **§7 検証 prompt の ratify
> ゲート統合**を実装済み (`src/verify.rs`、`templates/refute-prompt.md`、
> `templates/completeness-prompt.md`、`[verify]` config、`src/ratify.rs`、
> `tests/integration.rs`)。検証は `Vec<(label, Parsed)>` の純変換として `merge_report`/
> sentinel ロジックを無改造で挟み込み、`agent::run_shards`/`parse::parse` を再利用する。
> 検証テンプレは批准 lock に **有効なゲートのときだけ** pin される (§7)。**既定 OFF**
> (明示 opt-in)。**未実装(次スライス): §3.5 多票 (votes)、§4 loop-until-dry tier。**
> §11 に確定した設計判断を記録。

---

## 1. 動機 — いま欠けている閉ループ

現状のパイプライン (`src/main.rs::run`) は:

```
scope ──▶ shards (area/invariant/decision を fresh context で並列監査) ──▶ marker 解析
      ──▶ merge_report ──▶ needs_user なら sentinel + baseline 据え置き
```

各 shard の findings は **そのまま額面どおり** 統合され、`needs_user=yes` があれば
sentinel が立つ。`parse::Parsed` は report 本文と `needs_user` フラグしか持たず、
**findings を一度も独立に再検証していない**。これは agent 系で最も効くハーネス要素
(「生成→検証→修正の閉ループ」「多重検証」) が監査側に欠けている状態。

監査ツールの核心的な失敗モードは2つ、互いに逆向き:

- **偽陽性 (false positive)**: 実は `A: 監査の誤読` なのに `矛盾`/`不明` として上がる。
  例外条項の見落とし・誤った逐語引用・canon の読み違い。**1件ごとに人間の `ack`
  サイクルを1回消費し、繰り返すと sentinel が狼少年化して信頼を失う。**
- **偽陰性 (false negative)**: canon に定義された検証可能なルールを、サンプリング監査
  (`audit-prompt.md` 規律4: 実装はサンプリングで見る) が **照合し損ねて見落とす**。
  上がってこないので誰も気づけない — 監査ツールとして最も危険。

audit-prompt の verdict 語彙は既に `A: 監査の誤読 → finding 取り下げ` を持つが、これは
**同一 agent の自己申告** であり、独立した第二の目は無い。検証ゲートはこの2つの目を足す。

---

## 2. 二ゲート概要

```
shards ──▶ findings 収集 (既存) ──┐
                                  │
        ┌── V1 反証ゲート ─────────┴──────────────┐   偽陽性を削る
        │ needs_user=yes の各 finding を独立 skeptic │
        │ に渡し「逐語引用で REFUTE してみろ」。     │
        │ 覆せたものだけ取り下げ (A 降格)。迷えば残す │
        └────────────────────┬─────────────────────┘
                             │ 生き残った findings
        ┌── V2 網羅性批評 ────┴──────────────────────┐   偽陰性を埋める
        │ shard ごとに「canon の検証可能ルールで、    │
        │ 実装と照合され *なかった* ものは何か」を問う │
        │ → 新規 finding 候補を追加 (再批評はしない)  │
        └────────────────────┬─────────────────────┘
                             ▼
          merge_report (audit 原文 + 検証 verdict を併記) ──▶ sentinel
```

V1 は **偽陽性**、V2 は **偽陰性** に効く。両方足して初めて監査の両側面を覆う
(§8 でこの2つがバイアス軸で相補的であることを示す)。

---

## 3. V1 反証ゲート (adversarial verification)

### 3.1 何を検証するか — 対象の絞り込み

反証するのは **`needs_user=yes` の finding だけ**。理由: sentinel を立て人間サイクルを
消費するのはこの集合に限られる。`整合`/取り下げ済み/`needs_user=no` の行を再検証しても
コストに見合わない。これで反証コストが「実際に人間を呼ぶ finding 数」に比例する。

### 3.2 harness は表をパースしない — agent に再導出させる

`parse::Parsed` は findings を行単位で構造化していない。検証ゲートでも **harness は
markdown 表をパースしない** (脆い上に「judgment は LLM・harness は構造だけ」の分担を破る)。
代わりに skeptic agent に:

1. 元 shard の `needs_user=yes` findings 本文 (audit が出した行) を渡す。
2. canon ポインタ (area.canon / invariant.canon) を渡す — **中身はコピーせずポインタだけ**
   (`audit-prompt.md` と同じ規律3)。
3. 「各 finding を **生の canon を開いて独立に再導出** し、逐語引用で **覆せるなら取り下げ、
   覆せないなら存置** せよ。verdict と逐語証拠を出せ」と指示。

skeptic の出力を **既存の `parse::parse` でそのまま再パース** する (同じ marker 契約)。
これにより `agent::run_shards` (fresh context・`MAX_PARALLEL` 有界並列・read-only allowlist)
と `parse.rs` を **無改造で再利用** できる。新しい解析器を書かない。

### 3.3 安全側バイアス — 本物を黙って消さない (不変条件)

specguard 最大の安全弁は「逐語引用できないものは `不明` に降格」= **過剰主張より過小主張に
倒れる**。反証ゲートはこれを **対称に** 適用する:

- skeptic は **逐語引用で finding を覆せたときだけ取り下げ** る。引用で反証できなければ
  finding は **存置** (default = not refuted)。「なんとなく違う気がする」では消さない。
- これにより反証は **偽陽性は削るが、本物の drift を黙って消すことはしない**。
  迷い (`B/C 判別不能` や引用不能) は人間に届く側に倒す。

### 3.4 透明性 — 取り下げも証拠つきで残す

検証は finding を **silently rewrite しない**。merge した report には:

- audit が出した原 finding、
- それに対する verify verdict (`存置` / `取り下げ` + 逐語証拠)

を **併記** する。sentinel を立てる (`needs_user`) のは **存置された finding のみ**。だが
人間は「何が・なぜ取り下げられたか」を report で必ず見られる → **誤って本物を落としても
人間が override できる**。これは specguard が findings を出すだけで canon を書き換えない
(HOTL) のと同型 — 検証も *証拠を整える* だけで、最終判断は人間。

### 3.5 投票数 (multi-skeptic) — tier で可変 〔未実装・次スライス〕

v1 は **skeptic 1 票** 固定。将来 `[verify].votes = N` で **N 票の独立 skeptic** に増やし、
過半数が取り下げたときのみ取り下げる (workflow の adversarial-verify パターン同型)。
迷ったら存置 (§3.3) なので、票を増やすほど **取り下げに厳しく = 偽陽性除去は弱まるが
偽陰性リスクは下がる**。`thorough` 監査では票を増やし、視点を変える (correctness / 例外条項 /
逐語一致) ことも将来オプション。

---

## 4. V2 網羅性批評 (completeness critic)

反証 (V1) は findings を **減らす** 方向なので、偽陰性 (見落とし) を増やしうる。これを
打ち消すのが網羅性批評 — findings を **増やす** 方向の独立 agent。

- shard ごと (または canon ごと) に fresh context で1 agent を起動。
- 「この canon に定義された **検証可能なルール** のうち、今回の監査で実装と
  **照合され *なかった*** ものを列挙せよ。各々に逐語引用を付けよ」と問う。
- 出力は **新規 finding 候補** (`仕様未記載` ではなく「未照合ルール」= 偽陰性候補)。
  `needs_user` は audit-prompt の判定ルールに従う。
- **再批評はしない** (1 パスのみ)。無限ループ防止。本格的な loop-until-dry
  (K 回連続で新規ゼロまで) は将来 `thorough` tier の選択肢として §11 に残す。

V2 の出力は V1 を通すか? → **通さない**。V2 は「監査が見落とした」候補であり、それ自体が
反証対象ではない (むしろ「もっと見ろ」の指示)。コスト二重化を避け、人間に直接上げる。

---

## 5. 継承する不変条件 (絶対に崩さない)

`audit-prompt.md` / README 設計 / DESIGN.md から継承し、検証ゲートでも厳守する:

1. **read-only 強制** — skeptic / critic も既定 agent と同じ allowlist (Read/Grep/Glob +
   読み取り専用 git) で起動。書き込み・ネットワーク・任意 shell は権限で遮断。
2. **判定は LLM、集約は決定的ハーネス** — 反証/網羅性の *判断* は agent、票の集計・
   取り下げ適用・report 併記・sentinel は harness が決定的に。
3. **正典の中身はコピーしない** — 検証 prompt にも canon ポインタだけ渡す。
4. **矛盾は権威で自動解決しない (HOTL)** — 検証は **偽陽性を削り偽陰性を足す** だけ。
   生き残った `矛盾`/`不明` は従来どおり人間に Ask。検証が「どちらが正か」を裁くことは
   しない (canon-conflict-escalation の原則を保つ)。
5. **本物を黙って消さない** — §3.3 (引用で覆せたときだけ取り下げ) + §3.4 (取り下げも
   証拠つきで report に残す)。これが検証導入の最大のリスク (本物の drift を消す) への弁。
6. **冪等性は保てる範囲で** — 同一入力に対し検証 agent は非決定的でありうる。だが
   取り下げ条件を「逐語引用必須」に縛ることで揺れを抑える (specguard 本体の判定揺れと同程度)。

---

## 6. パイプラインへの結線 (実装ガイド)

`src/main.rs::run` の **marker 解析後・`merge_report` 前** に検証ステージを挿む:

```
outs = run_shards(...)            // 既存
parsed = parse(outs)              // 既存
─────────────── ここから新規 ───────────────
if cfg.verify.enabled {
    refute_prompts = render_refute(parsed where needs_user)   // §3.2
    refuted = run_shards(refute_prompts)                      // run_shards 再利用
    parsed  = apply_refutation(parsed, parse(refuted))        // §3.3/3.4 で存置/取り下げ
    if cfg.verify.completeness {
        critic_prompts = render_critic(shards)                // §4
        added = parse(run_shards(critic_prompts))
        parsed = parsed + added
    }
}
─────────────────────────────────────────────
merged = merge_report(..., parsed)   // audit 原文 + verify verdict 併記 (§3.4)
```

- **新規テンプレート**: `templates/refute-prompt.md` (§3) と `templates/completeness-prompt.md`
  (§4)。既存の `{{...}}` placeholder 規約と marker 契約 (`<<<SPEC_AUDIT>>>` 相当の
  `<<<SPEC_VERIFY>>>` を新設するか、同 marker を再利用するかは §11)。
- **exit code は増やさない** — 検証 agent の失敗は既存の `EXIT_AGENT_FAILED`、marker 欠落は
  `EXIT_NO_MARKER` に集約 (`src/main.rs` の `EXIT_*` 予約に新規衝突を作らない)。
- **`scope`/`prompt` サブコマンドの非介入** — agent を呼ばない既存デバッグ経路は不変。

### 設定 (TOML)

```toml
[verify]
enabled = false       # 反証ゲート (V1)。v1 は既定 OFF (明示 opt-in)
completeness = false  # 網羅性批評 (V2)。enabled とは独立に切り替え
# votes = 1           # 〔未実装〕skeptic 票数 (§3.5)。当面は 1 固定
```

**既定 OFF の理由**: 検証は追加の agent 呼び出しを伴うので、新規挙動を黙って全ユーザに
有効化しない (既存の監査挙動・統合テストを不変に保つ)。§8 のとおり **`enabled` 単独より
両方を有効化** するのを推奨 (config コメントと example でも明示)。段階導入: 自分の repo で
両方 on にして取り下げ率・見落とし発掘を観察してから常用に上げる。

---

## 7. メタ正典 — 検証 prompt も批准ゲートに乗せる 〔実装済み〕

`refute-prompt.md` / `completeness-prompt.md` は「**何を反証とみなすか / 何を網羅とみなすか**」
を決める *検証ポリシー* = メタ正典。`audit-prompt` が `accept-prompt` で批准されるのと
同じく (`src/ratify.rs`)、検証 prompt も **ratify ゲートに含める** (`require_ratification`
が on のとき fingerprint を `.specguard-prompt.lock` に pin)。**番人 (検証) の基準を機械が
勝手に緩められない** — 「番人を誰が見張る」の無限後退を、人間の同意で止める既存の解を
検証層にも延長する。

**consent は「有効なポリシー面」に限定する (scoped consent)** — ここが設計の肝:

- lock は `audit_hash` / `decisions_hash` に加え `refute_hash` / `completeness_hash` を持つ。
- **検証 hash は、そのゲートが有効なときだけ pin・drift チェックされる** (`ratify::drifted` の
  `refute_active` / `completeness_active`)。`accept-prompt` は無効なゲートの slot を **空のまま**
  残す (`main::accept_prompt`)。
- 効果: verify OFF で批准した後に **ゲートを ON にすると、lock の空 slot が drift として検出され、
  再批准を強制** する。人間は「今や有効になった検証ポリシー」を実際にレビューしてから同意する。
  批准を一度やれば全ゲートを黙って consent 済みにする、という **consent の過剰主張を避ける**。
- **後方互換**: 検証ゲート以前に書かれた lock は `refute_hash` 不在 → `#[serde(default)]` で空 →
  verify を使わないプロジェクトは再ブロックされず、ON にした瞬間だけ再批准要求 (inert policy を
  ゲートしない。change-triggered scope と同じ「動いていないものは見ない」規律)。

契約チェック (`accept-prompt` の必須 placeholder 検査) も同様に **有効なゲートの検証テンプレだけ**
対象にする (`prompt::REFUTE_PLACEHOLDERS` / `COMPLETENESS_PLACEHOLDERS`)。検証テンプレは現状
**埋め込み (embedded) 専用**で `[verify]` に template 上書き口は無いが、将来 override 口を開けても
この ratify ゲートがそのまま保護する。

---

## 8. 安全側バイアスの方向分析 — なぜ V1 と V2 を *両方* 入れるか

specguard 本体は既に「過小主張に倒れる」バイアスを持つ (引用できねば `不明`)。

- **V1 反証ゲートも findings を *減らす* 方向** → 本体と **同じ向き** のバイアス。
  V1 単独だと「監査が控えめ × 検証も控えめ」が **掛け算で偽陰性を増幅** しうる。
- **V2 網羅性批評は findings を *増やす* 方向** → **逆向き** のバイアス。V1 の過小化を
  打ち消し、見落としを能動的に掘る。

```
過小主張 ◀──────────────── バイアス軸 ──────────────▶ 過剰主張
   │                                                  │
   ├─ specguard 本体 (引用できねば不明)               │
   ├─ V1 反証 (覆せたら取り下げ)                       │
   │        ↑ ここまで全部「減らす」= 偽陰性リスク     │
   └──────── V2 網羅性批評 (未照合ルールを足す) ───────┘
                ↑ ここだけ「増やす」= 釣り合わせる
```

ゆえに **V1 だけ入れて V2 を入れないのは設計上危険**。両方入れて初めて偽陽性・偽陰性の
両側を制御でき、バイアスが釣り合う。`votes` (§3.5) は V1 の取り下げ強度のつまみ、
`completeness` は V2 の有無 — この2つで運用者が軸上の位置を調整する。

---

## 9. コストと並列

- 反証は `needs_user=yes` finding 数 × `votes` 回の agent 呼び出し。clean な監査 (findings
  ゼロ) では **追加コストゼロ** (反証対象が無い)。コストが「人間を呼ぶ件数」に比例する
  のは望ましい性質。
- すべて `agent::run_shards` 経由なので **`MAX_PARALLEL=4` の有界並列** が効く。検証 agent
  が監査 agent を圧迫しないよう、ステージを分離 (監査完了 → 検証開始) する。
- `completeness` は shard 数ぶんの固定コスト。`thorough` tier 専用にして既定 off。

---

## 10. specforge との相乗

specforge の ⑥ (実装↔spec drift 監査) は **specguard を無改造で呼ぶ** (DESIGN.md §6.1)。
よって specguard が検証ゲートを得れば、**specforge の受け入れゲート ⑥ も自動で反証・網羅性
を継承** する — 実装パッチの merge 可否判定の偽陽性 (無駄な差し戻し) と偽陰性 (drift を
見逃して merge) が同時に減る。検証層を specguard 側に閉じ込める設計 (DESIGN.md 段階 A の
「read-only 強制は specguard に閉じ込めたまま」) と完全に整合する。

---

## 11. 設計判断 — 確定 (v1 実装) と先送り

実装時に確定した判断と、意図的に次スライスへ送ったもの:

1. **marker は再利用 (確定)** — 検証出力も `<<<SPEC_AUDIT>>>` トレーラを出し、`parse::parse`
   を無改造で再利用する。出自の混ざりは report 構造で解消 — refute は元 shard 本文に
   `### 反証 (verify)` 小節を **併記**、completeness は `completeness:<label>` という別 shard
   として追加するので、人間は出自を見分けられる (§3.4)。`<<<SPEC_VERIFY>>>` 新設は不採用
   (parse 分岐のコストに見合わない)。
2. **反証は per-shard (確定)** — `needs_user=yes` を出した shard 単位で 1 skeptic。その
   shard の canon を一度だけ読ませ、その shard の findings をまとめて再導出させる。
   finding 1件ごとにプロセスを起こす案は不採用 (プロセス爆発を避け、`MAX_PARALLEL` 有界の
   既存ランナにそのまま乗る)。
3. **取り下げ集計 = 1票 (確定、多票は先送り)** — v1 は skeptic 1 票で skeptic の
   post-verify `needs_user` をそのまま採用。多票 (§3.5) は未実装。**INCONCLUSIVE
   (agent 失敗/marker 欠落) は取り下げず存置** = fail-safe (§3.3、`fold_inconclusive`)。
4. **V2 は 1 パス (確定)** — 網羅性批評は 1 回のみ (再批評しない)。loop-until-dry
   (K 回連続で新規ゼロまで) は将来 `thorough` tier の選択肢として残す。
5. **provenance = 継承 (確定)** — 検証は監査と同じ HEAD に対して走り、`merge_report` が
   付ける canon commit pin を検証小節も継承する。別 pin は設けない。
6. **検証 agent 失敗は非致命 (確定)** — refute/completeness の agent が失敗しても run 全体は
   中断しない (検証は監査の精緻化であり、壊れた検証で監査結果を失わせない)。refute は存置、
   completeness は「見落とし候補なし」扱いにして WARN を stderr に出す。`EXIT_AGENT_FAILED`
   には集約しない (監査本体の失敗とは区別する)。

7. **検証 prompt の ratify 統合 = scoped consent (確定・実装済み)** — §7 のとおり、検証
   テンプレは **有効なゲートのときだけ** lock に pin・drift チェックする。ゲートを後から ON に
   すると空 slot が drift となり再批准を強制 (consent を黙って広げない)。後方互換は
   `#[serde(default)]` で確保。

**先送り (次スライス):** §3.5 多票、§4 の loop-until-dry tier。

---

## 12. まとめ

- 監査側に欠けている **findings の検証ループ** を、既存の `run_shards`/`parse`/`ratify` を
  **再利用** して足す。新しい解析器・新しい exit code を作らない。
- **V1 反証ゲート** = 偽陽性除去 (無駄な ack を削る)、**V2 網羅性批評** = 偽陰性発掘
  (見落としを掘る)。§8 のとおり **両方入れて初めてバイアスが釣り合う**。
- 不変条件を厳守: read-only / 判定は LLM / canon コピーしない / 矛盾は人間に Ask /
  **本物を黙って消さない** (引用で覆せたときだけ取り下げ + 取り下げも証拠つきで report に残す)。
- 検証 prompt もメタ正典として ratify ゲートに乗せ、番人の基準を機械が勝手に緩めさせない。
- specguard に閉じ込めれば specforge ⑥ も自動で継承する。
- 段階導入: V1 (`votes=1`) をまず既定 on、効果を見て V2/`votes` を上げる。
</content>
</invoke>
