# harness

Cargo ワークスペース・モノレポ。`yukineko` の Claude Code ハーネス一家を単一ソースで管理する。

- 共通基盤: `crates/harness-core`（ビルド時依存。各プラグインのバイナリに静的に焼き込まれる）
- 各プラグイン: `crates/<plugin>/` — Rust クレートかつ Claude Code プラグイン（`.claude-plugin/plugin.json` + `hooks/` + 同梱 `bin/`）

配布はこの repo 自身で完結する。リポ root の `.claude-plugin/marketplace.json` が marketplace カタログで、各プラグインを `git-subdir`（`yukineko/harness.git` の `crates/<plugin>`、`ref=main`）で指す。利用側は `/plugin marketplace add yukineko/harness` → `/plugin install <plugin>@yukineko`。別リポへの切り出しは行わない。

## 開発

```sh
cargo build --workspace
cargo test  --workspace
```

各プラグインの検証:

```sh
claude plugin validate crates/<plugin>
```

## バージョニング

git-subdir 配布では `version` 省略時に「コミット SHA = バージョン」となり、モノレポでは無関係なコミットで全プラグインが新バージョン扱いになりうる。
**各 `crates/<plugin>/.claude-plugin/plugin.json` に明示 `version` を置き、そのプラグインが変わった時だけ bump する。**
