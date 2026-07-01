# daily

> Claude Code 向けの**1 日 1 回タスクランナー**。Rust 製の `SessionStart` フックが、**暦日あたり最大 1 回**だけタスクを実行し、見つかった結果をセッションに還元する。現在のタスクは依存関係の**セキュリティ監査**（`cargo deny check`）。

## 目的

`daily` は、Claude Code の `SessionStart` イベントに配線された決定論的な Rust バイナリである。その責務は、「定期的に回す価値はあるが、セッション開始のたびに回すのは無駄」なチェックを、**暦日（ローカル時刻）あたりちょうど 1 回**だけ走らせ、結果を非ブロッキングに会話へ注入することにある。

現在実装されているタスクは `security` の 1 つだけで、内容は次のコマンドである。

```sh
cargo deny check advisories bans sources licenses
```

監査結果に応じた挙動は以下のとおり。

| cargo-deny の結果 | `daily` の振る舞い |
|---|---|
| 成功（クリーン） | 何もせず沈黙する |
| 失敗（指摘あり） | `🔒 daily security audit: …` として最初の数行（`error` / `warning` / `RUSTSEC`）と `cargo deny check` を実行するヒントを注入する |
| cargo-deny 未インストール / 実行エラー | 沈黙する（ターンを壊さない） |

`cargo-deny` は `$CARGO_HOME/bin/cargo-deny` から解決し、無ければ `PATH` にフォールバックする。監査はセッションの `cwd` で実行され、`deny.toml` の無いリポジトリでも問題ない（cargo-deny 自身のデフォルトが使われる）。

実行結果は `additionalContext` としてエージェントに渡される**非ブロッキング**な情報であり、ターンを中断させることはない。また **subscription-native**（サブスクリプション完結）であり、API キーは不要、マシン外に何も送らない。フックは LLM を呼ばない決定論的バイナリである。

## どうして必要か

セキュリティ監査のような重めのチェックは、毎セッション走らせるとセッション開始の体感コストが積み上がり、結局オフにされがちになる。一方で完全に手動にすると、走らせること自体を忘れる。

`daily` はこの「毎回は重い／手動だと忘れる」というジレンマを、**1 日 1 回ゲート**で解く。各暦日の最初のセッションだけがコストを払い、その日の以降のセッションは静かにスキップする。これにより、誰も意識しなくても依存関係の脆弱性・ライセンス・banned crate などの監査が日次で回り続ける。

「今日もう走ったか？」という判定は、共有クレートの `harness-core::daily::DailyGuard` にある決定論ロジックが担う。

- 状態ファイル `~/.daily/state/<task>-daily.txt` に最後に実行した `YYYY-MM-DD` を保持する。
- `should_run()` は保存された日付が今日と異なるときだけ真になり、`mark_done()` が今日の日付を刻む。
- ゲートは時計の時刻ではなく**暦日（ローカル時刻）**を基準とするため、その日に何回セッションを開いても実行はちょうど 1 回に保たれる。

## どう使うか

### フック配線

`daily` の入口は単一のフックである。

| フック | イベント | 内容 |
|---|---|---|
| `daily session-start` | `SessionStart`（startup / resume / clear） | 有効かつ当日未実行なら、セキュリティ監査を実行し、`mark_done()` で日付を刻み、指摘があれば注入する。常に exit 0。 |

### サブコマンド

| サブコマンド | 目的 |
|---|---|
| `daily session-start` | SessionStart フック本体: 保留中の日次タスクを実行する |
| `daily install` | （未実装）フックを `~/.claude/settings.json` に追加する。現状は手動配線が必要 |

### プラグインとしての導入（推奨）

```text
# Claude Code 内で:
/plugin marketplace add yukineko/claude-harnesses
/plugin install daily@yukineko
```

フックは `${CLAUDE_PLUGIN_ROOT}/bin/daily session-start` を呼ぶ。`bin/daily` はプラットフォーム別バイナリ（`bin/daily-<os>-<arch>`）を選ぶ POSIX ランチャーで、対応バイナリの無いホストでは静かに exit 0 する。セキュリティタスクを機能させるには `cargo-deny` を別途インストールする必要がある（`cargo install cargo-deny`）。

### 設定

`~/.daily/config.toml`（任意）:

```toml
enabled = false   # すべての日次タスクを無効化する
```

config が無ければ**有効**として扱われる。現状の判定は単純な `enabled = false` の部分文字列マッチで、これを設定するとランナー全体を停止できる。
