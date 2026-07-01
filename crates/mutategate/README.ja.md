# mutategate

このワークスペース向けの **ミューテーションテスト kill-rate ゲート**。内部ツールであり、
配布される Claude Code プラグインではない（`plugin.json` を持たない）。

## なぜ

ゴールデン/リグレッションテストは、コードが *以前と同じ振る舞いを続けている* ことは証明する。
だが、もし欠陥が混入したときにテストがそれを **捕捉できるか** については何も語らない。
ミューテーションテストはその隙間を埋める。小さな欠陥（「mutant」）を注入し、既存テストが
失敗するか（mutant が **caught/killed**）、それでもパスしてしまうか（mutant が
**missed/survived**）を確かめる。生存可能な mutant のうち kill された割合が **kill-rate**
（mutation score）である。スコアが低ければ、どれだけグリーンに見えてもテストスイートは弱い。

背景: Meta の Automated Compliance Hardening (ACH) と PRIMG
(arXiv:2505.05584)。

## 何であり、何でないか

- ミューテーションエンジンを **実装しない**。標準の Rust ツール
  [`cargo-mutants`](https://mutants.rs) の上に立つ。
- **ゲートそのものである**: `cargo-mutants` の `outcomes.json` をパース → kill-rate を計算 →
  閾値を下回ったら非ゼロで exit する。この parse→score→exit のロジックは純粋で、固定の
  サンプル JSON に対してユニットテスト済み（`cargo test -p mutategate`）なので、pass/fail の
  判定は決定論的であり、（遅い）エンジンを起動せずに走る。

ここで用いる kill-rate の定義:

```
viable   = caught + missed + timeout      (unviable な mutant は除外 — シグナルなし)
killed   = caught + timeout               (timeout はテストが露出させた誤動作)
kill_rate = killed / viable               (未定義 -> ゲートは失敗)
```

## 使い方

```sh
# 既存の outcomes.json に対する決定論的ゲート:
cargo run -p mutategate -- --outcomes mutants.out/outcomes.json --min-kill-rate 0.80

# エンドツーエンド（パイロットクレートでエンジンを走らせてからゲート）:
scripts/mutation-gate.sh
PILOT=difflog MIN_KILL_RATE=0.70 scripts/mutation-gate.sh
```

Exit コード: `0` pass、`1` kill-rate が閾値未満（または生存可能な mutant が無い）、`2`
usage/IO/parse エラー。

## スコープ（意図的に狭い）

ワークスペース全体に対して `cargo-mutants` を走らせるのはゲートにするには遅すぎるため、
パイロットは **1 クレート** に絞る:

- **パイロット: `harness-core`** — 共有のビルド時ロジック。`hash`/`pricing`/`spans` は
  純粋でミューテーションに向く。`PILOT=<crate>` で上書きできる。
- `MUTANTS_EXTRA="--file src/hash.rs"` でさらに絞れば高速な実走ができる。

**閾値: 0.80。** これは確立されたミューテーションツール（例: PIT）や Meta ACH の系譜が示す
実務上の堅牢性のバーを反映している。これを下回るスイートは、検出可能な欠陥を明らかに
取りこぼしている。パイロットではゲートがフレークでなくシグナルであるよう保守的に保つ。

## 今後の拡張

- クレートは 1 つずつ、それぞれが既に閾値をクリアしてから追加する。そうすれば新しい
  クレートがゲートを黙って引き下げることはない。
- スイートが硬くなるにつれ `MIN_KILL_RATE` を引き上げる。生存した mutant は
  `target/mutants-<pilot>/`（`missed.txt`）で確認する。
- CI: `.github/workflows/mutation.yml` が手動ディスパッチ・週次スケジュール・ゲート機構に
  触れる PR でパイロットを走らせる — パイロット限定、ジョブ上限 30 分。
