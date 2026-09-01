# Markdown Quality Checks

Markdown ファイル（`.md`）で実行する品質チェック。

## 1. 自動修正 (markdownlint --fix)

```bash
markdownlint --fix <files>
```

**目的**: markdownlint が機械的に直せる違反（末尾空白、見出し前後の空行、
リスト記号の統一など）を修正する

## 2. 自動修正の補完 (fix-markdown-lint.py)

```bash
python3 ~/.claude/skills/language-checks/scripts/fix-markdown-lint.py <files>
```

**目的**: `markdownlint --fix` が直せない違反を修正する

| コード | 説明                           | 修正方法                   |
| ------ | ------------------------------ | -------------------------- |
| MD040  | コードフェンスに言語指定がない | 内容から言語を推定して付与 |
| MD060  | テーブルの列幅が揃っていない   | CJK 全角幅を考慮して整列   |

MD060 の幅計算は markdownlint（string-width パッケージ）に合わせ、East Asian
Width が Ambiguous の文字（→ ★ ● など）を幅 1 として扱う。幅 2 で揃えると
2 つのツールが互いの整形を崩し合い、pre-commit ゲートが通らなくなる。

## 3. Lint Check (markdownlint)

```bash
markdownlint <files>
```

**目的**: 1・2 で直せなかった違反が残っていないか確認する

**成功条件**: 終了コード 0

**失敗時の対応**: 残った違反は手で修正する（構造的な問題であることが多い）

## ステージ済みファイルへの適用

コミット前にステージされた `.md` だけを対象にする場合:

```bash
git diff --cached --name-only -z --diff-filter=ACM -- '*.md' | xargs -0 -r markdownlint --fix --
git diff --cached --name-only -z --diff-filter=ACM -- '*.md' | \
  xargs -0 -r python3 ~/.claude/skills/language-checks/scripts/fix-markdown-lint.py
git diff --cached --name-only -z --diff-filter=ACM -- '*.md' | xargs -0 -r git add --
git diff --cached --name-only -z --diff-filter=ACM -- '*.md' | xargs -0 -r markdownlint --
```

注: home-manager が配布する git の pre-commit フック（`modules/home/parts/git.nix`）が
1〜3 をこの順で実行し、修正済みファイルを再ステージする。`run-checks.sh` の Markdown
ステージも同じ手順。手で実行するのは、hook を経由しない場面（`--no-verify` 後の確認など）に限る。
