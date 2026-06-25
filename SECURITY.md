# Security

## 脆弱性の報告

セキュリティの問題を発見した場合は、**GitHub Issues** で報告してください。

- 報告先: <https://github.com/yukineko/harness/issues/new?labels=security>
- テンプレート: タイトルに `[SECURITY]` を付け、再現手順・影響範囲・発見バージョンを記載
- **対応 SLA**: 報告受領から **7 営業日以内** に初回応答します。重大度 Critical は **48 時間以内** に応答します。

> **注意**: 公開前に調整が必要な重大な脆弱性は Issue を非公開 (Private) に設定するか、
> リポジトリの Security Advisory 機能を使用してください。

---

## セキュリティ監査の実行方法

```bash
# 依存クレートの CVE/RustSec 勧告・ライセンス・禁止クレートをチェック
cargo deny check advisories bans sources licenses

# cargo-deny のインストール (未導入の場合)
cargo install cargo-deny --locked
```

CI は毎週月曜日と Cargo.lock 変更時に自動実行します (`.github/workflows/security-audit.yml`)。

---

## harness 固有のセキュリティ表面

### 1. sh -c コマンド実行 (意図的 escape hatch)

以下のクレートはユーザー設定から受け取ったコマンド文字列を `sh -c` で実行します:

| クレート | 設定キー / 環境変数 |
|---|---|
| condukt | `test_command` (設定ファイル) |
| beacon | `cmd` (設定ファイル) |
| tdd | `runner.cmd` (設定ファイル) |
| donegate | `check.cmd` (設定ファイル) |
| ctxrot | `CTXROT_DISTILL_CMD` (環境変数) |

**信頼境界**: これらのコマンドは**ローカルの信頼された設定/環境変数**からのみ受け取ります。
hook stdin・トランスクリプト・ネットワーク入力からは受け取りません。
外部入力を `sh -c` に渡す実装は追加しないこと。

### 2. git 引数インジェクション

`difflog/src/git.rs` の `diff_stat` / `diff_name_status` / `diff_body` / `log_oneline` は
revision 引数 (`base`) を `Command::args()` に渡す際、`--` セパレータを挿入し
`validate_base()` で先頭 `-` を拒否することで引数インジェクションを防いでいます。

**不変条件**: git subprocess は `Command::new(prog).args([...])` の安全な形式を維持し、
`format!()` で構築したユーザー由来文字列をシェル経由で渡さないこと。

### 3. パストラバーサル

- `fugu-router/src/pathutil.rs`: `normalise_path()` は `canonicalize()` 後に `strip_prefix`
  して repo-relative パスを生成。`/etc/passwd` 等 repo 外パスは変換せず返します。
- `harness-core/src/store.rs`: `write_note_named()` はセパレータをサニタイズし
  store root 外への書き込みを `PermissionDenied` で拒否します。

**不変条件**: 永続化するパスに絶対 home dir プレフィックス (`/home/...`) を残さないこと
(PII 最小化)。

### 4. 逆シリアライズ (serde_json / toml)

hook stdin・トランスクリプト・状態ファイルの JSON/TOML デシリアライズが主な
未検証入力表面です。安全な Rust のため memory-unsafety はありませんが、
攻撃者が影響できるファイルサイズ・ネスト深さに上限を設けることを推奨します。

---

## 既知の勧告を ignore する手順

`deny.toml` の `[advisories]` セクションに理由コメント付きで追記します:

```toml
[advisories]
ignore = [
  { id = "RUSTSEC-YYYY-NNNN", reason = "影響しない理由を記載" },
]
```

詳細は [RustSec Advisory Database](https://rustsec.org/advisories/) を参照。
