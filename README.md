# harness

Cargo ワークスペース・モノレポ。`yukineko` の Claude Code ハーネス一家を単一ソースで管理する。

- 共通基盤: `crates/harness-core`（ビルド時依存。各プラグインのバイナリに静的に焼き込まれる）
- 各プラグイン: `crates/<plugin>/` — Rust クレートかつ Claude Code プラグイン（`.claude-plugin/plugin.json` + `hooks/` + 同梱 `bin/`）

配布は別リポ `yukineko/claude-plugins` の marketplace が `git-subdir` で各 `crates/<plugin>` を指す（Phase F でカットオーバ）。

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
