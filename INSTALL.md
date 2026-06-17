# specguard 導入ガイド

仕様↔実装 整合監査ハーネス specguard を、ビルドから対象リポジトリへの組み込み・
定期実行まで一通り導入する手順。概要・設計は [README.md](README.md) を参照。

---

## 1. 前提条件

| 必要なもの | 用途 | 備考 |
|---|---|---|
| **Rust toolchain (`cargo`)** | specguard 本体のビルド | https://rustup.rs から導入 |
| **`git`** | 変更スコープの解決 (`git diff` / `ls-tree`) | 監査対象が git リポジトリであること |
| **`claude` CLI**（Claude Code） | 監査エージェント（既定） | `specguard run` のときだけ必要。認証済みであること |

`init` / `scope` / `prompt` / `ack` は `claude` CLI 無しでも動く（エージェントを起動しない）。
実際に監査する `run` のときだけ `claude` が必要。別のエージェントに差し替えることも可能
（後述の `[agent]`）。

---

## 2. 本体のインストール

### install.sh を使う（推奨・WSL2 / Linux / macOS）

```sh
./install.sh
```

- `cargo build --release` でビルドし、`~/.local/bin/specguard` に配置する。
- 配置先を変えるなら環境変数で:

  ```sh
  SPECGUARD_BIN_DIR=/usr/local/bin ./install.sh
  ```

- `~/.local/bin` が PATH に無ければスクリプトが追加方法を案内する。例:

  ```sh
  echo 'export PATH="$HOME/.local/bin:$PATH"' >> ~/.bashrc && source ~/.bashrc
  ```

確認:

```sh
specguard --version
specguard --help
```

### 手動ビルド

```sh
cargo build --release
# 生成物: target/release/specguard
cp target/release/specguard ~/.local/bin/    # 任意の PATH 上へ
```

### Windows（ネイティブ）

`install.sh` は bash 用。ネイティブ Windows では cargo で直接ビルドして配置する:

```powershell
cargo build --release
# target\release\specguard.exe を PATH の通った場所へコピー
```

---

## 3. 監査対象リポジトリへ組み込む

監査したいリポジトリのルートで `init` を実行する:

```sh
cd /path/to/your/repo
specguard init
```

生成物:

| パス | 内容 |
|---|---|
| `specguard.toml` | スターター設定（`specguard.example.toml` のコピー） |
| `.claude/settings.json` | SessionStart hook（`.specguard-pending` を検知してセッション開始時に提示） |

`init` は **冪等**:

- 既存の `specguard.toml` は `--force` を付けない限り上書きしない。
- 既存の `.claude/settings.json` の他の設定は壊さず、SessionStart hook だけを追記する。
  再実行しても hook は重複しない。

設定を作り直したいとき:

```sh
specguard init --force      # specguard.toml を example で上書き
```

---

## 4. 設定を編集する（`specguard.toml`）

`init` 直後の設定はサンプルなので、自分のリポジトリに合わせて編集する。要点:

```toml
[project]
name = "MyProject"
root = "."                  # 監査するリポジトリルート（この設定ファイルからの相対）

# [agent] を省略すると、ハーネスで read-only を強制した既定 (claude --print) を使う。
# 別エージェントに差し替えるときだけ書く。

[scope]
baseline_ref  = ""          # 空なら .last-ref → fallback_ref の順で解決
fallback_ref  = "HEAD~20"   # 初回 / 解決不能時のベースライン

[output]
report_dir = "reports/spec-audit"
sentinel   = ".specguard-pending"

# 領域: glob にマッチする変更があれば in-scope。canon に「どこを読むか」を指す。
[[area]]
name  = "backend"
globs = ["src/server/**", "api/**"]
canon = ["docs/architecture/api.md"]

# 不変条件: 変更の有無に関わらず毎回チェックする絶対ルール。
[[invariant]]
name        = "secrets path"
description = "secrets are only read from the approved config path"
canon       = ["docs/architecture/config.md"]
```

- `[[area]]` か `[[invariant]]` が最低 1 つ必要（無いと「監査対象なし」エラー）。
- `canon` は**ファイルパス／`file:section` ポインタ**だけを書く。仕様の中身はコピーしない
  （コピーするとドリフトの種になる）。エージェントが実体を読みに行く。

設定が正しいかは、エージェントを呼ばずに確認できる:

```sh
specguard scope     # 解決された baseline / in-scope 領域 / skip 領域を表示
specguard prompt    # 各 shard に渡るプロンプトを表示
```

---

## 5. 最初の監査

```sh
specguard run
```

- in-scope 領域ごと + 不変条件を、それぞれ別プロセス（fresh context）で並列監査し統合する。
- 結果:
  - `reports/spec-audit/<date>.md` … レポート本体
  - `reports/spec-audit/.last-ref` … 次回の change-triggered baseline（クリーンな回のみ前進）
  - `.specguard-pending` … `needs_user=yes` の指摘があるときだけ生成（sentinel）

指摘に対応したら sentinel を消す:

```sh
specguard ack
```

> **重要**: 未処理の sentinel がある間は baseline を前進させない（未修正ドリフトを取りこぼさ
> ないため）。対応して `ack` するまで同じ範囲を再監査し続ける。

### よく使うフラグ

```sh
specguard --config path/to/specguard.toml run    # 設定ファイルを指定 (-c)
specguard --baseline HEAD~5 run                   # baseline を上書き (-b)
SPECGUARD_BASELINE_REF=origin/main specguard run  # 同上（環境変数）
specguard --date 2026-06-17 run                   # レポート日付を固定（テスト用）
```

---

## 5.5 決定ログ（ADR）と D3 監査

仕様変更の *理由* を、その時点の canon commit に pin して残せる:

```sh
specguard decide "Single signing path"
# -> decisions/<date>-single-signing-path.md を生成 (canon_commit に pin)
```

生成された記録の frontmatter を編集する:

- `canon:` … この決定が支配する canon ポインタ（`file` / `file:section`）
- `drivers:` … **反証可能な理由**（例: "HMAC 鍵ローテーションが単一署名経路を要求"）
- `review_when:` … driver が崩れる＝再検討すべき条件

以降の `specguard run` では **D3 監査**が走り、各決定について
(a) 指す canon が今も一致するか（鮮度）、(b) driver/review_when が今も成立するか
（陳腐化＝理由より長生きした規則）を照合する。決定ログは *証拠* であって権威ではなく、
canon が常に正。`[decisions] dir` は in-repo ディレクトリでも Obsidian vault path でも
指せる（`""` で無効化）。

## 6. 定期実行（Human-on-the-loop）

`specguard run` をスケジューラから定期起動し、`.specguard-pending` を SessionStart hook
（`specguard init` が設定済み）で拾って「修正に着手?」を促す運用に組み込める。

cron（Linux / WSL2）の例:

```cron
0 9 * * * cd /path/to/your/repo && /home/you/.local/bin/specguard run >> /tmp/specguard.log 2>&1
```

Windows タスクスケジューラなら `specguard.exe run` を作業ディレクトリ＝リポジトリルートで
登録する。

---

## 7. 終了コード（スケジューラ／フック連携用）

| code | 意味 |
|---|---|
| 0 | 正常終了 |
| 2 | 設定 / 使用法エラー |
| 3 | いずれかの shard の出力に marker が無い（レポートは保存。baseline 前進・sentinel はしない） |
| 4 | いずれかの shard のエージェントが非ゼロ終了（真のコードは stderr） |

---

## 8. アンインストール

```sh
rm ~/.local/bin/specguard                 # バイナリ
# 対象リポジトリ側（任意）:
rm specguard.toml .specguard-pending
rm -r reports/spec-audit
# .claude/settings.json の SessionStart hook は手動で削除
```

---

## 9. トラブルシューティング

| 症状 | 原因 / 対処 |
|---|---|
| `specguard: command not found` | `~/.local/bin` が PATH に無い。§2 の PATH 設定を実施 |
| `spawning agent 'claude'` 失敗 | `claude` CLI 未インストール／未認証。`run` 以外なら不要 |
| exit 2「nothing to audit」 | `[[area]]`/`[[invariant]]` が未定義。`specguard.toml` を編集 |
| `baseline ... failed` → all-tracked | baseline も fallback も解決できず全 tracked file を監査（若い repo の初回など）。意図通りなら無視可、嫌なら `[scope].fallback_ref` を調整 |
| sentinel が消えない | 対応後に `specguard ack` を実行（クリーンランだけでは消えない） |
| エージェントが書き込もうとして失敗 | 既定はハーネスで read-only を強制（allowlist + auto-deny）。仕様どおり。**初回は実 `claude` で一度 `run` し、書き込み/任意 Bash が実際に弾かれるか確認推奨**（CLI バージョンで権限フラグ挙動に差が出ることがある） |

---

## 開発者向け

```sh
cargo test                  # unit + integration（擬似エージェント。実 LLM 不要）
cargo clippy --all-targets
```
