---
name: docs-issues
description: GitHub の issue を使わないリポジトリの docs/issues/ に、issue を書く・閉じるときの形（フロントマター、節、関係）。
allowed-tools:
  - Bash(design-records new:*)
  - Bash(design-records check:*)
  - Bash(design-records index:*)
  - Read
  - Edit
  - Write
---

# docs/issues

GitHub の issue があるリポジトリでは、そこに書く。ない場合だけ `docs/issues/` に置く。番号と下書きは
`design-records new --kind issue <slug> "<表題>"` で作る。OKF のバンドル（`docs/index.md` が `okf_version` を持つ）では、
フロントマターと必須の節が入った形で出来るので、中身を書く。ほかのリポジトリでは表題の行だけが出来るので、下の形を足す。

## 形

```yaml
---
type: Issue
status: closed
resolution: done
resolved-by: 0012-slug
duplicate-of: "#0012"
blocked-by: ["#0003"]
opened: 2026-10-10
closed: 2026-10-11
---
```

例は全部のキーを並べたもの。

- `type`: `Issue`（`docs/index.md` が `okf_version` を持つリポジトリでは必須）
- `status`: `open` か `closed`
- `resolution`: closed のとき。`done`、`wontfix`、`by-design`、`external`、`cannot-reproduce`、`duplicate`、`obsolete`
  のどれか。理由を添えるときは引用符で囲む（`"wontfix: 理由"`。囲まないと、コロンと空白を含む値をほかの YAML の読み手が読めない）。根拠を書く形の `ADR-NNNN`（決めた ADR）と `コミット <hash>`
  （解決したコミット。7〜40 桁の英小文字の16進）も通る。どれも閉じた理由を辿れるので、リポジトリの既にある形に揃える
- `resolved-by`: done のとき。ADR の番号かコミットの hash。任意
- `duplicate-of`: duplicate のとき
- `blocked-by`: 片方向。逆向きは grep で引く

節（見出しの 2 段目）は、必須が「問題」と「完了の条件」（`- [ ]` か `- [x]` の項目を 1 つ以上）、推奨が「経緯」。

- 関係は、その文書だけを読んで分かる側に置く。ADR との関係は ADR の `issues` から引き、issue 側には「関係する ADR:」を書かない
- 閉じるときは `status: closed` と `resolution` を同時に書く

## 検査

`design-records check` は、フロントマターを持つ issue の形と関係を注意（warning）として示す。
フロントマターの無い issue は見ない。
