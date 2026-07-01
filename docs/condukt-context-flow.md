# condukt のコンテキスト読み込みフロー

`/condukt` を指定したとき、各フェーズでどのようにコンテキストが読まれるかを段階的に説明する。

---

## 登場人物の整理

| 呼び名 | 正体 | 特徴 |
|---|---|---|
| **main loop** | `/condukt` を受けたあなたとの会話セッション | 会話の全履歴を持つ |
| **interpreter** | Phase 1 で起動する sub-agent | 完全に新規起動 |
| **worker** | Phase 5 で実装を担当する sub-agent | 完全に新規起動 |
| **verifier** | Phase 6 で検証を担当する sub-agent | 完全に新規起動 |

---

## Step 1 — `/condukt` 入力直後

ユーザーが `/condukt` と打つと：

1. Claude Code がスキルファイル（`skills/condukt/SKILL.md`）を読み込む
2. その内容が **main loop のシステムプロンプトに注入** される
3. **main loop が持つコンテキスト** = この会話の全履歴 + スキル指示

```
[ この会話全体 ] + [ SKILL.md の指示 ]
        ↓
    main loop
```

main loop は「あなたとの全会話」を見ている。だから「直前に話していた課題」を引き継げる。

---

## Step 2 — Phase 1: interpreter を起動する瞬間

main loop は `Agent()` ツールで interpreter を起動する。このとき **重要な事実がある**：

> **sub-agent はこの会話を一切見ていない。**

interpreter が知ることができるのは、main loop が **プロンプトとして文字列で渡したものだけ** である。

スキル指示の Phase 1 に書いてあるとおり：

```
KNOWLEDGE=$(condukt knowledge)  ← ファイルから読む
PLAYBOOKS=$(fugu-router procedures search ...)  ← ファイルから読む

→ これらを文字列にしてプロンプトに埋め込む
→ Agent(subagent_type: "condukt-interpreter", prompt: "..." + KNOWLEDGE + PLAYBOOKS)
```

つまり main loop が**手動で情報を切り出して文字列として渡す**のが唯一の通信手段である。

```
  この会話
     ↓  (main loop が必要な部分を抜粋してテキスト化)
  interpreter のプロンプト
     ↓
  interpreter が受け取るのはこのテキストだけ
```

---

## Step 3 — Phase 5: worker を起動する瞬間

worker も同じ。main loop は以下を**すべてテキストとして手組みして**渡す：

```
作業ディレクトリ: /home/.../worktrees/toolguard-truncate
touched_files: ["crates/ctxrot/src/hooks/toolguard.rs", ...]
done_criteria: "..."
interface_context: (grep で抽出した関数シグネチャ)
knowledge_context: (condukt knowledge の内容)
```

worker は「この会話でどんな議論があったか」「前回のセッションで何をしたか」**何も知らない**。渡されたプロンプトが唯一の情報源である。

それゆえスキルの Phase 5 には「`interface_context` は main が grep で事前収集して渡す」とある。worker に grep させると worker のコンテキストが汚れるので、main loop が事前に刈り取って文字列で渡す設計になっている。

---

## Step 4 — worktree と condukt state の役割

sub-agent が会話を共有できないなら、どうやって「worker が書いたコードを verifier が読む」のか？

答えは **ファイルシステムを共有メモリとして使う** である：

```
worker
  → worktree (/home/.../worktrees/toolguard-truncate) にコードを書く
  → git commit する
  → "done" とだけ返す

verifier のプロンプト
  ← main loop が "worktree=/home/..." をテキストで渡す
  → verifier がそのパスのファイルを Read する
```

`condukt state`（JSONL ファイル）も同じ：

```
state init → ~/.condukt/state/harness-xxx/run-xxx.json に書く
state set → そのファイルを更新する
→ どのセッション・どのサブエージェントからも読める
```

---

## 全体像

```
ユーザー入力
     │
     ▼
[ main loop ]──────────────────────────────────────────
│ この会話の全履歴 + SKILL.md の指示 を持つ             │
│                                                       │
│  ① condukt knowledge (ファイル読み)                    │
│  ② compass gap (バイナリ呼び出し)                      │
│  ③ git log (シェル実行)                               │
│   ↓ テキスト化して interpreter のプロンプトに埋める   │
│                                                       │
│  Agent(interpreter) ──→ [ interpreter ]              │
│                          会話を見ない                  │
│                          プロンプトのみ見る            │
│                          Decomposition JSON を返す     │
│                                ↓                      │
│  condukt validate / schedule (バイナリ)               │
│  AskUserQuestion (合意)                               │
│  condukt state init (ファイルに書く)                  │
│                                                       │
│  Agent(worker) ────→ [ worker ]                      │
│   プロンプトに:         会話を見ない                   │
│   - worktree path        worktree にファイルを書く     │
│   - done_criteria        git commit する              │
│   - interface_context    "done" を返す                │
│   - knowledge            ↓                           │
│                   condukt state set → ファイル更新    │
│                                                       │
│  Agent(verifier) ──→ [ verifier ]                    │
│   プロンプトに:         会話を見ない                   │
│   - worktree path        worktree のファイルを Read   │
│   - done_criteria        pass/fail を返す             │
│                                ↓                      │
│  condukt state set → ファイル更新                     │
│  condukt state gate → 完了確認                       │
│  git merge → main に取り込む                         │
└───────────────────────────────────────────────────────
```

---

## なぜこの設計か

main loop のコンテキストが長くなると応答が遅くなり、コストも上がる。worker・verifier を独立起動することで「重い作業の出力（ビルドログ・diff 全文）が main loop のコンテキストに積み上がらない」という効果がある。その代わり、必要な情報は main loop が**意識的に切り出してテキストで渡す**設計になっている。

この考え方は [`docs/context-optimization.md`](./context-optimization.md) の「3 軸（Size/Cost/Correctness）」と共鳴している：sub-agent 分離は **Cost** 軸（main loop のプレフィルを安く保つ）と **Size** 軸（重い出力を main context に積まない）の両方に寄与する。
