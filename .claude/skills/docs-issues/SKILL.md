---
name: docs-issues
description: GitHub の issue を使わないリポジトリの docs/issues/ に、issue を書く・閉じるときの形（フロントマター、節、関係）。
allowed-tools:
  - Bash(design-records new:*)
  - Bash(design-records check:*)
  - Read
  - Edit
  - Write
---

# docs/issues

GitHub の issue があるリポジトリでは、そこに書く。ない場合だけ `docs/issues/` に置く。番号と下書きは
`design-records new --kind issue <slug> "<表題>"`（表題の行だけを書く）で作り、下の形を足す。

## 形

```yaml
---
status: open               # open | closed
resolution: done           # closed のとき。done | wontfix | by-design | external | cannot-reproduce | duplicate | obsolete
resolved-by: <ADR の番号かコミットの hash>   # done のとき。任意
duplicate-of: "#0012"      # duplicate のとき
blocked-by: ["#0003"]      # 片方向。逆向きは grep で引く
opened: 2026-10-10
closed: 2026-10-11
---
```

節（見出しの 2 段目）は、必須が「問題」と「完了の条件」（チェックボックス）、推奨が「経緯」。

- 関係は、その文書だけを読んで分かる側に置く。issue 側に「関係する ADR:」は書かず、ADR の `issues` から引く
- `blocked-by` に開いている issue がある間は閉じない
- 閉じるときは `status: closed` と `resolution` を同時に書く

## 検査

`design-records check` が、フロントマターを持つ issue の `status`（open か closed）、閉じた issue の `resolution`、必須の節を
注意（warning）として示す。フロントマターの無い issue は、別の形で書くリポジトリのものとして見ない。
`resolved-by`、`duplicate-of`、`blocked-by` の整合（相手の存在、開いている相手を持つ closed）は、まだ検査しない（#6）。
