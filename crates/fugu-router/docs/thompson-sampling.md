# Thompson Sampling — fugu-router のルーティング原理

fugu-router が「どのモデルティアに振るか」を決めるときに使っている
**Thompson sampling**（多腕バンディットの一手法）を、初心者向けに解説する。
本家 Sakana fugu が *学習* するコーディネータを、fugu-router は
**retrieval（k-NN）+ online bandit（Thompson sampling）** で近似している
（`README.md` 参照）。

---

## 1. 解く問題：多腕バンディット (Multi-Armed Bandit)

複数の選択肢（腕）があり、それぞれ未知の報酬分布を持つ。各ステップで1つ選んで
報酬を観測する。**総報酬を最大化（= リグレットを最小化）** したい。

```
リグレット(T) = T·μ*  −  Σ 実際に得た報酬      (μ* = 最良腕の期待報酬)
```

核心は **探索 (exploration) と活用 (exploitation) のトレードオフ**：

| | 説明 | リスク |
|---|---|---|
| 活用 | 今まで一番良かった腕を引く | 本当の最良腕を見逃す |
| 探索 | まだ試していない腕を引く | 劣った腕に報酬を浪費する |

---

## 2. Thompson Sampling の考え方

**「各選択肢の当たり率を “確信度つき” で予想し、その予想からくじを引いて
一番高い腕を選ぶ」**。

ポイントは当たり率を1つの数字でなく **分布（山）** で持つこと：

```
試行が少ない腕 → 山が広い（よくわからない＝自信なし）→ たまに高い値が出る → 探索される
試行が多い腕   → 山が狭い（確信あり）              → 安定した値が出る   → 活用される
```

毎ラウンドの手順：

1. **① 各腕の山からくじを1枚引く**（分布からサンプリング）
2. **② 引いた値が最大の腕を選ぶ**
3. **③ 当たり/外れで、その腕の山を更新**

自信のない腕は山が広い → ときどき高いくじが出て **試され**、情報が増えて山が狭くなる。
本当に良い腕は高い位置で狭くなり **安定して選ばれる**。
→ 探索と活用のバランスが**自動で**取れる。

### 山の正体：Beta 分布

成功/失敗を数えるだけで山が作れる（ベルヌーイ報酬の共役事前分布）：

```
山 = Beta(1 + 当たった回数, 1 + 外れた回数)
  0勝0敗 → Beta(1,1) = 真っ平ら（0〜100%まったく不明）
  7勝3敗 → Beta(8,4) = 70%付近に寄った山。試行が増えるほど尖る
```

この山から乱数を1つ引くのが「くじを引く」操作。

---

## 3. 他手法との比較

| 手法 | 探索のしかた | ひとこと |
|---|---|---|
| ε-greedy | 確率 ε でランダム | 単純。ダメな腕も平等に試して無駄 |
| UCB | 「自信のなさ」をボーナス加点（楽観） | 決定論的、理論保証 `O(log T)` |
| **Thompson** | **山からくじを引く（確率的）** | 不確実な腕を確率的に試す。実務で強い |

> UCB は「自信ない奴に下駄を履かせる」、Thompson は「自信ない奴ほど結果がブレる
> くじを引かせる」。どちらも不確実なものをほどよく試すための工夫。

---

## 4. 実証コード（標準ライブラリのみ）

`random.betavariate(a, b)` で Beta 分布から直接サンプリングできる。

```python
"""Thompson Sampling 実証デモ。3本腕のベルヌーイ・バンディット。"""
import random

TRUE_P = {"A": 0.30, "B": 0.55, "C": 0.70}   # 真の当たり率(本人には未知)。Cがベスト
BEST_P = max(TRUE_P.values())
ARMS = list(TRUE_P)
N_ROUNDS = 2000


def pull(arm):
    """腕を1回引く。当たれば1、外れれば0(ベルヌーイ試行)。"""
    return 1 if random.random() < TRUE_P[arm] else 0


def thompson(seed):
    random.seed(seed)
    a = {arm: 1 for arm in ARMS}   # 1 + passes
    b = {arm: 1 for arm in ARMS}   # 1 + fails
    picks = {arm: 0 for arm in ARMS}
    reward = 0
    for _ in range(N_ROUNDS):
        samples = {arm: random.betavariate(a[arm], b[arm]) for arm in ARMS}  # ①くじを引く
        choice = max(samples, key=samples.get)                              # ②最大の腕を選ぶ
        r = pull(choice)                                                    # ③結果で更新
        reward += r
        picks[choice] += 1
        if r:
            a[choice] += 1
        else:
            b[choice] += 1
    return picks, reward, a, b


def epsilon_greedy(seed, eps=0.1):
    random.seed(seed)
    passes = {arm: 0 for arm in ARMS}
    counts = {arm: 0 for arm in ARMS}
    picks = {arm: 0 for arm in ARMS}
    reward = 0
    for _ in range(N_ROUNDS):
        if random.random() < eps:                       # 探索: ランダム
            choice = random.choice(ARMS)
        else:                                           # 活用: 今の最高平均
            rates = {arm: (passes[arm] / counts[arm]) if counts[arm] else 0.0 for arm in ARMS}
            choice = max(rates, key=rates.get)
        r = pull(choice)
        reward += r
        picks[choice] += 1
        counts[choice] += 1
        passes[choice] += r
    return picks, reward


if __name__ == "__main__":
    tp, tr, a, b = thompson(seed=0)
    print("Thompson:", tp, "reward=", tr,
          "regret=", round(BEST_P * N_ROUNDS - tr))
    ep, er = epsilon_greedy(seed=0)
    print("eps-greedy:", ep, "reward=", er,
          "regret=", round(BEST_P * N_ROUNDS - er))
```

### 実行結果

```
真の当たり率: A=0.30  B=0.55  C=0.70(ベスト)   2000ラウンド

=== Thompson Sampling ===
  腕A(真値0.30):    20回   推定当たり率=0.36
  腕B(真値0.55):    27回   推定当たり率=0.41
  腕C(真値0.70):  1953回   推定当たり率=0.70   ← 98%をベスト腕に集中
  総報酬 = 1382  /  リグレット = 18

=== ε-greedy (eps=0.1) ===
  腕A(真値0.30):   110回
  腕B(真値0.55):   103回
  腕C(真値0.70):  1787回
  総報酬 = 1350  /  リグレット = 50         ← 強制ランダムで取りこぼし大

参考: 常にベストC → 1400 (リグレット0) / 常にランダム → 1033
```

**読み取れること:**

1. **ベスト腕Cに収束** — 誰も真の確率を教えていないのに、結果だけから 98% をCに集中。
2. **ダメな腕を早く見切る** — A/B は計47回で探索を打ち切り。ε-greedy は最良判明後も
   強制ランダムで A/B を 213回引き続け、リグレットが膨らむ。
3. **よく引いた腕ほど推定が正確** — Cの推定 0.70 は真値と一致、引いてない A/B は
   荒いまま放置。探索を必要な所だけに使う挙動が数字に出ている。

---

## 5. fugu-router での具体化

選択肢 = Claude ティア `haiku < sonnet < opus`、当たり = 「検証を通った」。
実装は `src/policy.rs::decide_bandit`（`config.rs` の `explore=true` で有効、デフォルト）。

上のデモとの違いは2点だけ：

| デモ | fugu-router |
|---|---|
| `random.betavariate(a, b)` で Beta から引く | **正規分布で近似** `rng.normal(mean, sd)`（軽い） |
| 「くじが最大の腕」を選ぶ | **「閾値 `pass_threshold` を超えた中で一番*安い*ティア」** を選ぶ（コスト最適化） |

```
Beta(1+passes, 1+fails) の平均・分散 → 正規近似でサンプリング
  → cheapest-first で閾値を超えた最安ティアを採用
  → どれも超えなければ事後平均が最良のティアを活用
  → 履歴ゼロなら keyword prior にフォールバック (prior_model)
```

試行の少ない安いティアは事後分布が広い → たまに高くサンプルされ **probe（探索）** され、
実績が貯まると **活用** に収束。これにより「安く済むなら安く、必要なら高いモデル」を
オンライン学習する。

### 全体のパイプライン

```
タスク(title+files) ──▶ ① k-NN検索 (rag.rs, Jaccard) ──▶ 似た過去エピソード
                                                          (model, pass?, cost)
                       ──▶ ② Thompson sampling (policy.rs) ──▶ suggested_model
```

`record` で結果を記録するたびに各ティアの Beta 事後分布が更新され、次回の
ルーティングが賢くなる。

---

## 参考

- 実装: `crates/fugu-router/src/policy.rs`（`decide` = しきい値版 / `decide_bandit` = Thompson版）
- 類似検索: `crates/fugu-router/src/rag.rs`, `semantic.rs`
- 本家: [Sakana AI fugu](https://sakana.ai/fugu-release/)
- 設計の位置づけ: `docs/AGENTIC-CODING-GUIDE.md`
