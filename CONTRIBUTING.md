# Contributing

このモノレポは複数の Claude Code プラグインを 1 つの workspace で管理し、各プラグインを
`git-subdir`（`yukineko/claude-harnesses.git` の `crates/<plugin>`、`ref=main`）で配布する。配布の仕組みは
[README の「配布」節](README.md) を参照。

## ビルドとローカル反映（ソース変更を有効化する）

各プラグインの「実体」は、リポジトリのソースでも `crates/<plugin>/bin/<name>-<os>-<arch>`（配布用にコミットされた
バイナリ）でもなく、**インストール済みキャッシュ** `~/.claude/plugins/cache/<vendor>/<plugin>/<ver>/bin/<name>-<os>-<arch>`
にある platform 別バイナリである。`bin/<name>` ランチャが `uname` で該当バイナリを exec する。

**帰結**: crate のソースを直しても、リポジトリを `cargo build` しただけでは動いているハーネスの挙動は変わらない。
キャッシュ内のバイナリを差し替えて初めて有効化される（実行時 config は即反映されるが、ロジックは再ビルド＋差し替えが必須）。

ローカルで有効化するには:

```sh
scripts/rebuild-plugins.sh            # cargo clean → release build → ホスト platform のキャッシュを冪等に差し替え
scripts/rebuild-plugins.sh --no-clean # 増分ビルド（cargo clean をスキップ）
scripts/rebuild-plugins.sh --dry-run  # 差分だけ表示（ビルド・コピーなし）
scripts/rebuild-plugins.sh --stage-repo   # コミット対象 crates/*/bin も更新
CLAUDE_PLUGIN_CACHE=/path scripts/rebuild-plugins.sh   # キャッシュルート上書き
```

**配布用バイナリの注意**: `rebuild-plugins.sh` はホスト platform（例: Linux なら `linux-x86_64`）しか再ビルドできない。
darwin バイナリは Mac / CI で生成する前提のため、linux 版だけを `crates/*/bin` にコミットすると platform 間が
不整合になる。通常はローカルの再ビルド blob をコミットせず、配布用は Mac / CI で全 platform 揃えて更新する。
単一 crate を任意 target で staging するには `scripts/build-plugin-bin.sh <crate> [rust-target] [bin-name]` を使う。

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

## カバレッジ運用

CI の [`coverage.yml`](.github/workflows/coverage.yml) が `cargo llvm-cov` でワークスペース全体の
line coverage を計測し、下限（`COVERAGE_MIN_LINES`、初期値 65%＝計測時点の約 72% から数ポイント下）を下回ると非ゼロ終了する。この下限は
**目標値ではなく、上げていくための「床」**であり、coverage が余裕を持って上回るようになったら PR で
引き上げる（赤いビルドを通すために下げない）。テストを伴わない実装は既存の `tdd` ゲートと同様、
この床を通じても抑止される。数値は各 run の Job Summary と lcov アーティファクトで確認でき、
外部サービス・トークンは不要。ツール導入も他のワークフロー（`msrv` / `semver` / `security-audit`）と
同じく `cargo install --locked` で行い、CI のサプライチェーンを first-party に保つ。
