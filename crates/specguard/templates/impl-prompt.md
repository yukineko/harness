# specforge — 実装タスク: {{PROJECT_NAME}} / {{SPEC_ID}} / {{REQ_ID}}

生成日: {{DATE}}

---

## 対象 requirement

**ID**: `{{REQ_ID}}`

**statement**:
{{STATEMENT}}

**acceptance criteria** (⑥監査の照合点):
{{ACCEPTANCE}}

**canon poインタ** (実装根拠の接地先):
{{CANON}}

---

## 実装ガイドライン

1. **acceptance criteria をすべて満たす**実装を行うこと。
   各 criterion が observable で falsifiable であることを確認する (G4)。
2. **canon を読む**こと。canon に書かれていない振る舞いを追加しない。
3. **他 area を壊さない**こと。変更範囲は statement の scope に絞る。
4. **テストを書く**: 各 acceptance criterion に対する自動テストを最低1つ書く。
   テスト結果 (pass/fail コマンド) を evidence に含める。
5. 完了したら、以下の機械マーカーで出力を締める:

```
{{IMPL_MARKER}}
task_id: {{REQ_ID}}
status: done
test_cmd: <テスト実行コマンド>
test_result: pass
evidence_note: <1〜2行の証拠サマリ>
```

---

## 厳守事項

- marker なしで終了した場合、このタスクは **失敗**扱いになる。
- `status: done` は acceptance criteria がすべて通る場合のみ使う。
  部分的な達成は `status: partial` で、残課題を `evidence_note` に書く。
- `status: failed` はブロッカーがあり達成不能な場合に使う。
  理由と必要な情報を `evidence_note` に書く。
