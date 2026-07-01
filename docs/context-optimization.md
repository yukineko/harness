# Context Optimization — 施策まとめ

長いセッションでのコンテキスト劣化を防ぐために構築した施策の全体像。
検出・退避・制御を担う `ctxrot` と、サイズ・コスト・正確性の変換レイヤー `context-governor`、その 2 本柱を記録する。

---

## 3 つの軸と 2 つのシステム

コンテキスト最適化が触れるのは **3 つの独立した軸** だけ。混同するとすべてが無効になる。

| 軸 | 何を変えるか | 効く手段 |
|---|---|---|
| **Size** | ウィンドウ占有を実際に縮める | ツール結果のトリム・参照ボディの外部化・グルーミング |
| **Cost** | プレフィルを安く保つ | プロンプトキャッシュを温かく保つ = 安定したプレフィックスを維持する |
| **Correctness** | 規範がサマリー化で消えるのを防ぐ | lossy 圧縮を外部委任し、何が残るかだけを管理する |

> **設計原則:** context-governor は独自の lossy サマライザーを一切持たない。圧縮はビルトインのコンパクションに委任し、「何が残るか / 何を退避するか / あとで何を想起するか」の規律だけを追加する。

---

## System 1: ctxrot — 検出・レスキュー・制御

1 バイナリ・6 サブコマンド構成。各フックは stdin から JSON ペイロードを読み、エラーでも必ず exit 0（ターンを絶対に壊さない）。

### フック一覧

| サブコマンド | Hook Event | 役割 |
|---|---|---|
| `guard` | `UserPromptSubmit` | バンド検出（50/75/90%）・大ファイル参照への誘導。バンド≥2 では preemptive rescue note を書く。バンドごとに1回だけ注入（自身がrotにならないよう）。 |
| `rescue` | `PreCompact` | コンパクション直前に transcript をストリームして durable rescue note（決定事項/残課題/触ったファイル/リンク）を書く。決定論的、LLM 不使用。非同期 `claude -p` distill も起動（`distill_on_compact`）。 |
| `restore` | `SessionStart` | 前セッションの rescue note から決定事項・残課題を inject。セッションタグで自分のノートを優先し、並列セッションのキャリーオーバーを混入させない。 |
| `preguard` | `PreToolUse` | ロード前の予防ゲート。deny glob（絶対拒否）/ allow glob（サイズゲート bypass）/ サイズゲート（≥1MB の unbounded Read を deny）。通常のソース読み込みに影響しない最小設計。 |
| `toolguard` | `PostToolUse` | 50KB–1MB 帯の大ペイロード着弾後に「次回は sub-agent 経由で」と誘導。ブロックはしない。 |
| `statusline` | `statusLine` | 常時コンテキスト使用率メーター（`ctxrot 52% ▮▮▯▯ band1 ~104k/200k`）。バンドで色変化（緑→黄→赤）。 |

### 追加機能

| 機能 | 説明 |
|---|---|
| `re-anchor` | ウィンドウ末尾付近で既知の決定事項を再注入（lost-in-the-middle 対策）。`reanchor_min_band=2` かつ 8 プロンプトに1回だけ発火。recall eval で効果測定可能。 |
| auto-distill (danger band) | 200k 危険帯到達時に自動で `/distill` 相当を発火し退避を促す。nudge だけでなく実行も。 |
| auto-compact (Stop hook) | `Stop` フック経由で、セッション終了時に高使用率なら `/compact` を促すナッジを送る。 |
| `/distill` skill | オンデマンドの LLM 品質蒸留。使用率を読んで低ければスキップ、高ければ distill + `/compact` 必須化。 |
| `/ctx` skill | load / pin / unload / list — コンテキストに何を入れるかの明示的制御。状態は `ctxrot ctx` ストアに保持。 |

---

## System 2: context-governor — サイズ・コスト・正確性

単一フックディスパッチバイナリ。各アイテムは **3 レーン** に分類され、レーンが唯一の取り扱い規則になる。

### 3 レーン分類

| レーン | 意味 |
|---|---|
| `Pinned` | 最終コンテキストに常駐。規範・契約・不変条件。 |
| `Verbatim` | lossy 圧縮禁止。PreCompact でバッキングストアにスナップショット。 |
| `Evictable` | グルーム・退避・あとで想起可能。ToolResultGroomer が受け入れるのはこれだけ。 |

> **型安全不変条件:** `ToolResultGroomer`（唯一の圧縮ハンドラー）は `Evictable` トークンしか受け付けない。`Pinned`/`Verbatim` を渡すとコンパイルエラーになる。「verbatim は圧縮されない」が型システムで表現不能になっている。

### フック一覧

| Hook Event | ハンドラー | 役割 (軸) |
|---|---|---|
| `PostToolUse` | ToolResultGroomer | **★ 最主要サイズレバー。** ウィンドウ圧力を見てグルーム予算を動的調整。Evictable のみ head/tail トリム。出力は常に入力より小さい（property test で保証）。 |
| `UserPromptSubmit` | ContextInjector | プロンプトに対して仕様セクションをスコアリングし、マッチしたセクションを `additionalContext` として付加。ledger の `was_injected` で重複排除。 |
| `SessionStart` | StateRehydrator | コンパクション後に規範コアを復元。バッキングストアの `SNAPSHOT_KEY` を `additionalContext` へ再注入。SpecClassifier レーンを考慮した選択的注入。 |
| `PreCompact` | CompactionGuard | コンパクション前にトランスクリプトをスナップショット、verbatim スパンをバッキングストアへ記録。デフォルトは常に Proceed（ブロックしない）。 |
| `Stop` / `SubagentStop` | Checkpointer | 完了した作業をしきい値ゲート付きでバッキングストアへ外部化。副作用のみ、出力は捨てられる。ターンを絶対ブロックしない。 |

### Action Ledger — 計測基盤

サイズ軸は「主張」でなく「計測」が必要。groom / inject / snapshot の 3 アクションは、実行のたびに `ledger.jsonl` へ1行ずつ追記する。`saved_tokens`（実際に回収したウィンドウ占有）と `resident_tokens`（アクション後の占有）を記録。

```sh
context-governor rollup   # ledger 集計: total_saved_tokens, per-action breakdown
```

---

## 共存モデル — フィールド非交差

2 つのシステムは 4 つのイベントを共有するが、書き込み先が異なるため競合しない。

| Event | context-governor の書き込み先 | ctxrot の書き込み先 |
|---|---|---|
| `PostToolUse` | `updatedToolOutput`（in-place 圧縮） | `additionalContext`（次ターンへの誘導テキスト） |
| `UserPromptSubmit` | `additionalContext`（参照ボディ注入） | stdout テキスト（バンド助言） |
| `SessionStart` | `additionalContext`（規範コア復元） | `additionalContext`（決定事項・残課題 carryover） |
| `PreCompact` | スナップショット → バッキングストア（常に Proceed） | rescue note 書き込み（常に exit 0） |

---

## セッション通しの流れ

```
セッション開始
  restore      (SessionStart)  決定事項・残課題を carryover inject
  rehydrator   (SessionStart)  規範コアをバッキングストアから復元

… 作業中 …

  groomer      (PostToolUse)   ツール結果を毎ターン圧力に応じてトリム
  toolguard    (PostToolUse)   大ペイロード通過後に sub-agent 誘導
  injector     (UserPromptSubmit) 関連仕様セクションをプロンプトに付加
  guard        (UserPromptSubmit) バンド監視・バンド≥2 で preemptive rescue note
  preguard     (PreToolUse)    1MB 超 unbounded Read を事前 deny

使用率 ≥ 75%  auto-distill 発火 or /distill 実行

/compact
  rescue       (PreCompact)    durable rescue note 書き込み
  guard        (PreCompact)    transcript スナップショット
  (async)      claude -p       LLM 品質 distill note（distill_on_compact）

セッション終了
  checkpointer (Stop)          完了作業をバッキングストアへ外部化
  stop hook    (Stop)          高使用率なら次セッション開始前に /compact 促進
```

---

## 計測ファイル

| ファイル | 内容 | CLI |
|---|---|---|
| `state_dir/ledger.jsonl` | context-governor の action ledger (groom/inject/snapshot) | `context-governor rollup` |
| `state_dir/metrics.jsonl` | ctxrot のトークン軌跡・バンド交差・rescue note サイズ・gate deny | `ctxrot metrics`, `ctxrot metrics compare A B` |

`session-insights report --context` が context-governor の ledger ヘルスをレポートに統合する。

---

## 主要マイルストーン

| コミット | 内容 |
|---|---|
| `b48d3d7` | context-governor Phase 1 — 型・トレイト・フックディスパッチ契約の凍結 |
| `90fd171` | DefaultGroomer — Phase 2 の最主要サイズレバー実装 |
| `489994e` | DefaultClassifier / DefaultInjector — Phase 2b |
| `39c3d88` | DefaultRehydrator — SessionStart 復元 |
| `f47735e` | DefaultGuard — PreCompact スナップショットバックストップ |
| `ceafc00` | DefaultCheckpointer — しきい値ゲート付き Stop スナップショット |
| `bd439ae` | durable JSONL ledger — サイズ計測基盤の構築 |
| `aa112aa` | rollup サブコマンド — ledger 集計 CLI |
| `5c30c01` | window-pressure-aware groom budget — I6 observe→act |
| `986df03` | injector dedup — ledger seen-state で同一参照の重複注入を防ぐ |
| `49c5412` | SpecClassifier lanes in rehydrator — Pinned/Verbatim/Evictable を復元時に考慮 |
| `d755842` | auto-distill at 200k danger band — nudge だけでなく自動発火 |
| `d18325d` | ContextWindow を HookInput に追加 — harness-core 共通フィールド |
| `4d570df` | `auto_compact_enabled` / `auto_compact_at_percentage` config 追加 |
| `b655fea` | Stop hook — セッション終了時の自動コンパクト誘導 |
| `725a118` | session-insights report --context — ledger ヘルスをレポートに統合 |
| `42fa3a2` | context-governor をマーケットプレイスプラグインとして公開 |
