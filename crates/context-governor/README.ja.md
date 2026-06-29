# context-governor

Claude Code 組み込みの compaction（自動要約圧縮）の**周りに被せる薄い制御層**。単一の hook-dispatch バイナリとして配線される。

## 目的

context-governor は、コンテキストマネージャをゼロから作り直すものではない。harness 既存の compaction はそのまま使い、その周辺に **4 つの機能**だけを足す。

- **pin（ピン留め）** — 規範（contract・不変条件・命名規則・受け入れ基準など）を常駐させ、compaction を越えて生き残らせる。
- **lossless-recall（無損失リコール）** — verbatim（逐語保持が必須）な情報を backing store に退避し、要約で静かに消えないようにする。
- **retrieval（リトリーバル）** — 巨大だが状況依存な参照本体（網羅テーブル・全エンドポイント一覧・付録など）をウィンドウ外へ押し出し、必要なターンだけ注入する。
- **tool-hygiene（ツール結果の手入れ）** — エージェントループで最も膨らむ要因であるツール結果を、ターンごとに刈り込む。

設計の核心は、この層が触れる **3 つの軸を混同しない**ことにある（v1/v2 の設計失敗はこの混同だった）。

- **size（ウィンドウ占有）** — 実際にウィンドウを小さくする。効くのは「常駐する規範テキストの最小化」「参照本体の retrieval への押し出し」「ターンごとのツール結果の手入れ」の 3 つだけ。キャッシュ配置・ピン留め・自動 compact 閾値の引き下げは size を減らさない。
- **cost（再計算・レイテンシ）** — prefill を安くする＝prompt cache を効かせる。安定した接頭辞が勝ち、毎ターン接頭辞を書き換えると失う。
- **correctness（規範の保全）** — 規範や逐語必須の情報が、要約のなかで黙って消えるのを止める。

重要なのは、**この層は独自の lossy な要約器を一切持たない**こと。圧縮は組み込み compaction に委譲する。context-governor が足すのは「何を常駐させ／何を退避し／何を後で呼び戻すか」という規律だけである。

## どうして必要か

長いセッションでは、Claude Code の compaction が走ってコンテキストを圧縮する。compaction 自体は必要だが、素のままだと次の失敗モードを踏む。

- **規範が消える** — contract や不変条件のような「毎ターン効いていてほしい」norm が、要約で paraphrase されたり脱落したりする。規範違反は norm が ambient（常駐）でない限り気づけないため、これは静かに correctness を壊す。
- **逐語情報が壊れる** — 逐語一致が必要な情報（ID・コマンド・仕様の原文）が要約で改変される。
- **ツール結果でウィンドウが膨らむ** — エージェントループでは肥大したツール結果が支配的な成長項になり、放置すると size と signal-to-noise の両方が悪化し、lost-in-the-middle を招く。
- **軸の取り違え** — 「ピン留めすればウィンドウが小さくなる」「閾値を下げれば省サイズになる」といった、size と cost/correctness を混同した手を打ってしまう。実際にはピン留めは常駐コスト（一定の税）を**増やす**。

context-governor は、これらを**型として**強制することで防ぐ。各アイテムは `Pinned`（常に最終コンテキストに存在）・`Verbatim`（決して lossy 圧縮しない）・`Evictable`（手入れ・退避・retrieval 可）の 3 レーンのいずれかに属し、レーンがそのアイテムの扱われ方の唯一の真実になる。たとえば「逐語アイテムを圧縮してはならない」という不変条件は、圧縮を行う唯一のハンドラ（`ToolResultGroomer`）が `Evictable` トークンしか受け取れないことで**表現不能**にしてある——`Pinned`/`Verbatim` を groomer に渡すコードはコンパイルが通らない。

## どう使うか

context-governor は**単一のフックディスパッチバイナリ**である。stdin でフックペイロードを受け取り、`hook_event_name` で分岐して対応するハンドラを実行し、エンベロープ（JSON）を stdout に書く。スラッシュコマンドやサブコマンドはなく、Claude Code のフックに配線して使う。

| フックイベント | ハンドラ | 役割（触れる軸） |
|---|---|---|
| `PostToolUse` | `ToolResultGroomer` | ★主たる size レバー。肥大したツール結果を刈り込む／要約置換する。`Evictable` のみを扱うため、構造上 `Pinned`/`Verbatim` を渡せず、出力は入力より小さい。 |
| `UserPromptSubmit` | `ContextInjector` | retrieval／参照本体の注入。プロンプトの隣に `additionalContext` を添える（プロンプトの置換ではなく、モデルが読む前の reduce）。 |
| `SessionStart` | `StateRehydrator` | 復元。normative core / verbatim を store から再注入し、ピンが compaction を越えて生き残るようにする（resume の reseed も）。 |
| `PreCompact` | `CompactionGuard` | バックストップ。compaction 前にトランスクリプトをスナップショットし、verbatim スパンを backing store へ記録してから進行可否を決める。既定は `Proceed`（圧縮は組み込みに委譲、ここでは自前要約しない）。 |
| `Stop` / `SubagentStop` | `Checkpointer` | 完了した仕事を閾値ゲート付きで backing store へ外部化する。**副作用のみ**で、出力は破棄され決してブロックしない。 |

実行ルールは 2 つ。

- **ターンを壊さない** — ディスパッチ全体が `harness_core::hook::run_hook` の内側で走り、panic を握りつぶして exit 0 する。空・不正なペイロードは無音の no-op（`{}`）。
- **ブロックできるのは PreCompact だけ** — `Block` 決定のみ exit 2（Claude Code のブロック信号）。`Proceed` を含む他のすべての経路はエンベロープを書いて exit 0 する。

組み込み compaction の上に被せる薄い層なので、追加の API キーは不要で **subscription で完結**する。動作に必要なのはフック配線とこのバイナリだけ（hooks + binary）。

> 注: 現在の実装はフェーズ 1。lane/spec の型・フック I/O エンベロープ・ハンドラのトレイト集合・不変条件という「契約」を凍結した段階で、既定ハンドラの中身はフェーズ 2 以降で埋まる（groomer を最優先に）。バイナリのディスパッチ配線と契約の形は確定している。
