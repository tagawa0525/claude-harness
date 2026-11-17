# Markdown Quality Checks

Markdown ファイル（`.md`）は、次の 1 → 2 → 3 の順に実行する。

## 1. 自動修正 (markdownlint --fix)

```bash
markdownlint --fix --disable MD038 -- <files>
```

MD038（code span の内側の空白）は自動修正から外し、3 の検査で違反として出す（`--fix` は code span の先頭の
空白を落とし、シェルの `<<' X'` のような空白に意味のある書き方を変える）。設定では MD038 を無効にしない
（検査まで止まる）。ファイルの中の `<!-- markdownlint-enable MD038 -->` や `markdownlint-configure-file` が
MD038 を有効に戻していると `--fix` はまた先頭の空白を落とすが、書き手が有効にしたものなので、そのままにする。

## 2. 自動修正の補完 (fix-markdown-lint.sh)

```bash
~/.claude/skills/language-checks/scripts/fix-markdown-lint.sh <files>
```

`markdownlint --fix` が直せない違反を修正する。

| コード | 説明                           | 修正方法                   |
| ------ | ------------------------------ | -------------------------- |
| MD040  | コードフェンスに言語指定がない | 内容から言語を推定して付与 |
| MD060  | テーブルの列幅が揃っていない   | CJK 全角幅を考慮して整列   |

MD060 の幅は markdownlint に合わせ、East Asian Width が Ambiguous の文字（→ ★ ● など）を幅 1 で数える
（幅 2 で揃えると markdownlint の整形と崩し合う）。

## 3. Lint Check (markdownlint)

```bash
markdownlint <files>
```

終了コード 0 で成功。残った違反は手で修正する。

## ステージ済みファイルへの適用

コミット前にステージされた `.md` だけを対象にする場合は、次を使う（ステージした版に 1 → 2 → 3 を当ててステージし直し、
作業ツリーに触れない）:

```bash
~/.claude/bin/claude-hooks git-hook staged-checks markdown
```

git の pre-commit フックと `run-checks.sh` の Markdown のチェックも同じ検査を使う。`run-checks.sh` の `--merge` と
`--base` は、自動修正せず、作業ツリーの `.md` を `markdownlint` で検査だけする。
