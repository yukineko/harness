# /condukt のコンテキスト読み込み — 段階解説

## 登場人物

| 呼び名 | 正体 | 特徴 |
|---|---|---|
| **main loop** | `/condukt` を受けたメインセッション | この会話の全履歴を持つ |
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

main loop は「あなたとの全会話」を見ています。だから「直前に話していた課題」を引き継げます。

---

## Step 2 — Phase 1: interpreter を起動する瞬間

main loop は `Agent()` ツールで interpreter を起動します。このとき重要な事実があります：

> **sub-agent はこの会話を一切見ていない。**

interpreter が知ることができるのは、main loop が **プロンプトとして文字列で渡したものだけ** です。

スキル指示の Phase 1 に書いてあるとおり：

```
KNOWLEDGE=$(condukt knowledge)       ← ファイルから読む
PLAYBOOKS=$(fugu-router procedures search ...)  ← ファイルから読む

→ これらを文字列にしてプロンプトに埋め込む
→ Agent(subagent_type: "condukt-interpreter", prompt: "..." + KNOWLEDGE + PLAYBOOKS)
```

つまり main loop が**手動で情報を切り出して文字列として渡す**のが唯一の通信手段です。

```
  この会話
     ↓
(main loop が必要な部分を抜粋してテキスト化)
  interpreter のプロンプト
     ↓
  interpreter が受け取るのはこのテキストだけ
```

---

## Step 3 — Phase 5: worker を起動する瞬間

worker も同じです。main loop は以下を**すべてテキストとして手組みして**渡します：

```
作業ディレクトリ: /home/.../worktrees/toolguard-truncate
touched_files: ["crates/ctxrot/src/hooks/toolguard.rs", ...]
done_criteria: "..."
interface_context: (grep で抽出した関数シグネチャ)
knowledge_context: (condukt knowledge の内容)
```

worker は「この会話でどんな議論があったか」「前回のセッションで何をしたか」何も知りません。
渡されたプロンプトが唯一の情報源です。

それゆえスキルの Phase 5 には「`interface_context` は main が grep で事前収集して渡す」とあります。
worker に grep させると worker のコンテキストが汚れるので、main loop が事前に刈り取って文字列で渡す設計です。

---

## Step 4 — worktree と condukt state の役割

sub-agent が会話を共有できないなら、どうやって「worker が書いたコードを verifier が読む」のでしょうか？

答えは **ファイルシステムを共有メモリとして使う** です：

```
worker
  → worktree (/home/.../worktrees/toolguard-truncate) にコードを書く
  → git commit する
  → "done" とだけ返す

verifier のプロンプト
  ← main loop が "worktree=/home/..." をテキストで渡す
  → verifier がそのパスのファイルを Read する
```

`condukt state`（JSONL ファイル）も同じです：

```
state init → ~/.condukt/state/harness-xxx/run-xxx.json に書く
state set  → そのファイルを更新する
           → どのセッション・どのサブエージェントからも読める
```

---

## 全体像

```
ユーザー入力
     │
     ▼
[ main loop ]──────────────────────────────────────────
│ この会話の全履歴 + SKILL.md の指示 を持つ
│
│  ① condukt knowledge (ファイル読み)
│  ② compass gap (バイナリ呼び出し)
│  ③ git log (シェル実行)
│   ↓ テキスト化して interpreter のプロンプトに埋める
│
│  Agent(interpreter) ──→ [ interpreter ]
│                          会話を見ない
│                          プロンプトのみ見る
│                          Decomposition JSON を返す
│
│  (main loop が JSON を受け取り state init)
│
│  Agent(worker) ──→ [ worker ]
│    ↑ プロンプトに含む      会話を見ない
│    ・done_criteria        worktree のファイルを見る
│    ・touched_files        実装して commit して返す
│    ・interface_context
│
│  (main loop が worktree から diff を確認)
│
│  Agent(verifier) ──→ [ verifier ]
│    ↑ プロンプトに含む      会話を見ない
│    ・worktree パス         ファイルを Read して照合
│    ・done_criteria         pass/fail を返す
│
└──────────────────────────────────────────────────────
         ↕ 共有メモリ
  [ ファイルシステム ]
  ~/.condukt/state/...    ← runs / tasks の状態
  ~/.condukt/worktrees/.. ← コード差分
  condukt knowledge       ← プロジェクト知識
```

---

## なぜ「会話を渡さない」設計なのか

1. **コンテキスト節約** — 会話全体を sub-agent に渡すとトークンが爆発する
2. **役割の明確化** — 各 sub-agent は「自分に渡されたプロンプトの仕事だけ」に集中できる
3. **並列実行** — 複数 worker が同時に動くとき、共有会話は競合するがファイルは競合しない（worktree で隔離）

---

## よくある誤解

| 誤解 | 実際 |
|---|---|
| sub-agent は会話履歴を見ている | 見ていない。プロンプトに渡されたテキストのみ |
| worker が直接 main に質問できる | できない。AskUserQuestion は main loop 専用 |
| condukt state は「データベース」 | ただの JSONL ファイル。バイナリが読み書きする |
| phase は並列に動く | worker は並列だが、phase 自体は直列（1→2→…→7） |
