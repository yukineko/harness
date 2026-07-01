# Contributing

このモノレポは複数の Claude Code プラグインを 1 つの workspace で管理し、各プラグインを
`git-subdir`（`yukineko/claude-harnesses.git` の `crates/<plugin>`、`ref=main`）で配布する。配布の仕組みは
[README の「配布」節](README.md) を参照。

## バージョン運用ポリシー

**プラグインは独立して semver でバージョニングする。workspace 全体で統一した単一バージョンは
持たない。** root `Cargo.toml` の `[workspace.package]` は `rust-version` / `edition` / `license`
だけを共有し、`version` は **意図的に置かない**。

理由: 各プラグインは `git-subdir` で個別に配布・インストールされる独立した成果物であり、
利用者はプラグイン単位でインストール・更新する。統一バージョンにすると、無関係なプラグインの
コミットでも全プラグインが「新バージョン扱い」になり、利用者に無意味な更新を強いる
（git-subdir では `version` 省略時に「コミット SHA = バージョン」となるのと同じ問題）。

### バージョンの置き場所（2 箇所を常に一致させる）

バイナリを持つプラグインは、バージョンを **2 箇所** に持つ。両者は常に同じ値でなければならない:

| ファイル | 用途 |
|---|---|
| `crates/<plugin>/.claude-plugin/plugin.json` の `version` | marketplace / `/plugin install` が参照する配布バージョン |
| `crates/<plugin>/Cargo.toml` の `version` | cargo ビルド・依存解決が参照するクレートバージョン |

例外:
- **skill のみのプラグイン**（例: `scout`）は Rust バイナリを持たないため `plugin.json` のみ。
- **内部共有ライブラリ**（例: `harness-core`）は単独配布されないため `plugin.json` を持たず、
  `Cargo.toml` の `version` は cargo 内部用。プラグインとして bump 対象にはしない。

### bump のルール

- **変更したプラグインだけ** を bump する。他プラグインのバージョンは触らない。
- semver に従う: 破壊的変更 → major、後方互換の機能追加 → minor、バグ修正 → patch。
- `plugin.json` と `Cargo.toml` の `version` を **同じ値で同時に** 上げる。
- **`harness-core` を変更した場合**: それによって挙動が変わるプラグインは、当該プラグインも
  bump する（共有ライブラリの変更は、それを呼ぶプラグインの「変更」として伝播する）。
  `harness-core` 自身は配布物ではないので、利用者向けの bump 対象ではない。

### チェックリスト（プラグインを変更した PR）

- [ ] `crates/<plugin>/.claude-plugin/plugin.json` の `version` を semver で bump した
- [ ] `crates/<plugin>/Cargo.toml` の `version` を同じ値に合わせた
- [ ] 変更していない他プラグインのバージョンは触っていない
- [ ] `cargo test --workspace` / `cargo clippy --workspace -- -D warnings` / `cargo fmt --check` が通る
