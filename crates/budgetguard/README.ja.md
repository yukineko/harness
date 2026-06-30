# budgetguard

> 🌐 [English](README.md) ・ **日本語**

**Claude Code 向けリアルタイム・コスト予算ゲート (Rust 製)**

## 目的

budgetguard は、Claude Code セッションの **見積もりコスト (USD)** を監視し、設定した上限を
超えたら Stop を **ブロックする** ハーネスである。gauge が「観測する」のに対し、budgetguard は
「制御する」。

`budgetguard gate` は Claude Code の **Stop** フックに配線される。各 Stop で次を行う:

1. セッションのトランスクリプト (JSONL) を読み、モデルごとのトークン使用量を集計する。
2. 組み込みの料金テーブルで USD コストを見積もる (gauge と同じ料金表)。
3. ローカルの **日次台帳** (`~/.budgetguard/state/ledger.json`) を、このセッションの最新コストで
   更新する。
4. セッション合計とその日の累積合計を、設定した上限と照合する。
5. 判定を出す:
   - **上限内** → exit 0、Stop はそのまま進む。
   - **warn 閾値超過** → `{"additionalContext":"…"}` を出す (助言のみ、ブロックしない)。
   - **block 閾値超過** → `{"decision":"block","reason":"…"}` を出し、ターンが終わる前に
     エージェントへ保存・コミットを促す。

超過通知はエージェントに差し戻されるので、エージェントは作業を保存して優雅に切り上げられる。

これは検証ゲート群のコスト制御版の兄弟である:

| ゲート | タイミング | 問い |
|---|---|---|
| `donegate` | Stop 時 | ビルドが通り、テストが通るか? |
| `reviewgate` | Stop 時 | コード品質は許容できるか? |
| **`budgetguard`** | **Stop 時** | **コストは予算内か?** |

API キーは不要。トランスクリプトは既にディスク上にあり、budgetguard はそれを決定論的に読む。
何もマシンの外には出ない。

## どうして必要か

LLM エージェントを回しっぱなしにすると、コストは静かに積み上がる。gauge のような
観測ツールは「いくら使ったか」を後から教えてくれるが、それは事後報告であり、走っている
ターンを止めはしない。暴走したループや想定外に高価なセッションは、誰かがダッシュボードを
見るまで膨らみ続ける。

budgetguard はこの空隙を埋める。コストを Stop ゲートに変えることで、セッション単位・日次の
**ハードな上限**を強制し、超過分をエージェント自身に差し戻して安全に着地させる。観測だけでなく
**制御**が要る場面のためのハーネスである。

安全側に倒れる設計になっており、誤って作業をブロックしないよう次を保証する:

- `[[session]]` も `[[daily]]` も上限を設定していなければ、すべての Stop を許可する。
- ハーネス側のエラー (不正な設定、読めないトランスクリプト、自身のバグ) は exit 0 で素通りする。
- `BUDGETGUARD_DISABLE=1` ならフックは no-op になる。

## どう使うか

### 導入 (プラグイン)

マーケットプレイス経由 (カタログは本リポジトリ `yukineko/claude-harnesses` のルートにある):

```
/plugin marketplace add yukineko/claude-harnesses
/plugin install budgetguard@yukineko
```

**サブスクリプション完結** — Stop フック 1 本と同梱の Rust バイナリだけで動き、`ANTHROPIC_API_KEY`
も追加インストールも要らない。

### 手動導入 (ソースから)

```sh
cargo install --path .
budgetguard init          # 雛形の ./budgetguard.toml を書き出す
budgetguard install       # Stop フックを ~/.claude/settings.json にマージする
```

### 設定

プロジェクトルートに `budgetguard.toml` を置くか、グローバル既定として
`~/.budgetguard/config.toml` を置く。全オプションは `budgetguard.example.toml` を参照。

```toml
[session]
warn_usd  = 0.50
block_usd = 2.00

[daily]
warn_usd  = 5.00
block_usd = 20.00
```

### サブコマンド

```sh
budgetguard gate      # Stop フック (stdin の JSON を読み、判定を出す)
budgetguard status    # 解決済みの設定 + 今日の支出
budgetguard init      # 雛形の budgetguard.toml を書き出す
budgetguard install   # フックを ~/.claude/settings.json にマージする
budgetguard uninstall # フックを取り除く
```

### 料金

`harness-core` の組み込み料金テーブルを使う (gauge と同じ):

| ファミリ | 入力 $/1M | 出力 $/1M |
|---|---|---|
| Fable / Mythos | 10 | 50 |
| Opus | 5 | 25 |
| Sonnet | 3 | 15 |
| Haiku | 1 | 5 |

`budgetguard.toml` の `[[price]]` スタンザで任意のモデルの料金を上書きできる。

## ライセンス

MIT
