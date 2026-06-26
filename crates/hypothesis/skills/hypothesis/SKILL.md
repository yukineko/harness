# hypothesis スキル

PDO（プロダクト発見）の仮説ライフサイクルを管理します。仮説の追加・検証・棄却・一覧表示を行い、compass のゴールと連動させることで、発見活動を構造化します。

## コマンド一覧

### 新規仮説追加

```
hypothesis add "仮説テキスト" [--goal "ゴールキーワード"]
```

- `"仮説テキスト"` — 追加する仮説の内容を文字列で指定する
- `--goal "ゴールキーワード"` — compass の `charter.md` に記載された `north_star` または DoD キーワードを指定して仮説を紐づける（省略可）

例:
```
hypothesis add "ユーザーはオンボーディングで離脱している" --goal "retention"
```

### 検証済みにマーク

```
hypothesis validate <id> [--evidence "根拠"]
```

- `<id>` — 仮説 ID（`hypothesis list` で確認）
- `--evidence "根拠"` — 検証の根拠となるデータや観察結果を記録する（省略可）

例:
```
hypothesis validate HYP-003 --evidence "ユーザーインタビュー5件中4件で確認"
```

### 棄却にマーク

```
hypothesis reject <id> [--reason "理由"]
```

- `<id>` — 仮説 ID
- `--reason "理由"` — 棄却理由を記録する（省略可）

例:
```
hypothesis reject HYP-007 --reason "A/B テストで有意差なし"
```

### 一覧表示

```
hypothesis list [--status open|validated|rejected]
```

- `--status open` — 未検証の仮説のみ表示
- `--status validated` — 検証済みの仮説のみ表示
- `--status rejected` — 棄却済みの仮説のみ表示
- フィルタなしで全仮説を表示

例:
```
hypothesis list --status open
```

## compass との連動

`--goal` オプションに `charter.md` の `north_star` フィールドや DoD（Definition of Done）に記載されたキーワードを指定することで、仮説を compass のゴールに紐づけられます。

セッション開始時に `hypothesis session-start` が自動実行され、現在オープンな仮説の数とゴール別の内訳が表示されます。compass が示す次のアクションと照合しながら、検証すべき仮説を優先付けしてください。

### 典型的なワークフロー

1. compass で現在のゴール（north_star）を確認する
2. `hypothesis add` でゴールに紐づいた仮説を登録する
3. 発見活動（インタビュー・実験・データ分析）を実施する
4. 根拠が得られたら `hypothesis validate` または `hypothesis reject` でステータスを更新する
5. `hypothesis list --status open` で未検証の仮説を確認し、次の発見活動を計画する
