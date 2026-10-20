---
name: gh-pr-create
description: GitHub PRを作成。変更内容からPRタイトルと本文を自動生成。
# 差分の要約とPR本文生成の品質を優先
model: sonnet
argument-hint: [--draft] [--reviewer REVIEWER]
allowed-tools:
  - Bash(git status*)
  - Bash(git branch*)
  - Bash(git log*)
  - Bash(git diff*)
  - Bash(git push*)
  - Bash(gh pr*)
  - Bash(gh auth*)
  - Bash(~/.claude/scripts/branch-topics.sh*)
  - Bash(~/.claude/scripts/review-level.sh*)
  - Bash(~/.claude/scripts/base-moved.sh*)
---

# GitHub PR Create Command

GitHub Pull Requestを作成する（gh CLI使用）。このスキルの本体はタイトルと本文を書くこと。

## 事前確認

!`gh auth status 2>&1 | head -3`
!`git status --short`
!`git branch -vv`
!`git log '@{upstream}..HEAD' --oneline 2>/dev/null || echo "(上流ブランチ未設定)"`

## トピックの確認

!`~/.claude/scripts/branch-topics.sh`

PR の Why を1文で書く。2文要る（無関係な決定や話題が混ざっている）なら PR を作らず /topic-triage で分ける。

## 比較元の進みの確認

比較元（既定ブランチ）が分岐点から進み、同じパスを触っていないかを調べる。他のセッションが同じ変更を
先に入れていると、マージ直前の rebase まで気付けない。ブランチ名を渡して実行する（引数なしは cwd のブランチを
見るので、worktree の外から呼ぶと別のブランチを比べる）:

```bash
~/.claude/scripts/base-moved.sh <ブランチ名>
```

- 何も出ず、`WARN:` も無ければ進める
- `WARN:` が出たら、出力が空でも比較は未確認として扱う（fetch の失敗か、分岐点が見つからない）。
  原因を直して再実行するか、未確認のまま進めると PR 本文に書く
- `COMMON:` が出たら、出力の分岐点と先頭の hash で `git diff <分岐点>..<先頭> -- <パス>` を読み、同じ変更の二重なら
  PR を出さずに止め、衝突しうるなら PR 本文の Summary に書いて進める（スクリプトは知らせるだけで、止めはしない）

積み重なったブランチは `--base origin/<土台のブランチ>` を付ける（fetch が更新するのは `origin/*` だけで、
ローカルの土台は古いまま比べることになる）。

## ローカルレビュー（PR を出す前）

GitHub の自動レビューを何周も回すより、ローカルの `/code-review` のほうが早い。PR を出す前に回し、
指摘がなくなってから出す。GitHub の自動レビューは最終ゲートとして残す:

- レベルと対象はスクリプトが決める。`LEVEL`（docs/ だけなら medium、他は high）と `WORKTREE` を
  `/code-review` に渡す。対象はブランチ名でなく `WORKTREE`（ブランチをチェックアウトしている worktree のパス）で
  渡す。引数なしの `/code-review` は呼び出し元の cwd の差分を見て、ブランチ名は cwd のリポジトリで解くので、
  cwd が別の worktree やリポジトリだと「指摘なし」と同じ形の空の結果が返る:

  ```bash
  ~/.claude/scripts/review-level.sh
  ```

  差分が空だとスクリプトが止まる。出力の `LEVEL` と `WORKTREE` で `/code-review <LEVEL> <WORKTREE>` を回す。
  レビューが対象を見つけられなかった・実行できなかったときの返答は、指摘ゼロでなく未実施として扱う（`[]` が指摘ゼロだと言えるのは、返答が差分を読んだと示しているときだけ）。
  まだマージしていない別のブランチの上に積んでいるときは、`--base <土台のブランチ>` を付ける
  （付けないと土台の変更までレビューの対象になる）
- 直すのは検証できる誤りだけ（バグ、コードや事実と食い違う記述、壊れた参照）。直したらコミットして、
  もう一度回す。その周に検証できる誤りが無ければ終わり（レビューは周ごとに新しい候補を出すので、
  指摘ゼロを待たない）。残った指摘は、PR 本文の Tests に見送った理由を添えて書く
- claude-hooks の、普通に書く形（`cd <worktree> &&`、`env` や `timeout` の前置き、`bash -c '…'`、`nix develop -c` など、ふつうの手順で書くコマンド。
  珍しい git の設定や全体オプションで行き先を変える、pipefail を途中で外す、のような作為的な書き方は含まない）でない抜け道の指摘は直さず、無いものとして打ち切りを決める
  （hook は境界ではなく、抜け道を 1 つ塞ぐと次の反例が出て周が終わらない。抜け道はモデルの判断と人のレビューに任せる）。
  普通の形を誤って止める指摘は直す
- 文書にはコードの挙動を重複して書かず、コードへの参照（ファイルと関数名）を書く。重複した記述は、
  コードが変わると古くなり、次のレビューの的になる
- 別の PR を先にマージしないと成り立たないとき（別リポジトリの変更を指す ADR など）は、PR 本文の
  Summary に先行 PR を書く（他リポジトリは `owner/repo#N`）。差分のレビューでは見えない依存なので、
  ここで書き残す

## 未プッシュコミットの確認

未プッシュコミットがある（または上流が未設定）なら先にプッシュする（上流は自動設定される）:

```bash
git push
```

`pre-pr-create-check` hook は未プッシュのまま `gh pr create` すると deny する
（gh が対話的に push 先を尋ねて固まるのを防ぐ）。hook はコマンド実行**前**の状態を見るので、
`git push && gh pr create …` のように 1 コマンドに繋ぐと push 前の状態で deny される。
push と `gh pr create` は必ず別々の Bash 呼び出しで実行する。

## PR内容の生成

ベースブランチとの差分を分析:

```bash
git log main..HEAD --oneline
git diff main..HEAD --stat
```

### PRタイトル

- 最初のコミットメッセージまたは変更の要約から生成
- 70文字以内

### PR本文テンプレート

見出し `## Summary` / `## Changes` / `## Tests` は必須。本文は、同じリポジトリへの PR なら日本語、
fork から上流へ出す PR（`-R owner/repo --head my-account:branch`）なら相手のリポジトリの言語
（ふつう英語）で書く:

```markdown
## Summary
[変更内容の要約]

## Changes
- [主要な変更点をリスト]

## Tests
- [ ] 動作確認済み
- [ ] テスト追加/更新済み
```

## PR作成

`--title` と `--body` は必ず指定する（`--fill` や本文省略はエディタが開いて止まる）。

### 通常のPR

```bash
gh pr create --title "[タイトル]" --body "$(cat <<'EOF'
## Summary
[要約]

## Changes
- [変更点]

## Tests
- [ ] 動作確認済み
EOF
)"
```

`--draft` / `--reviewer` が指定されていればそのまま付ける。

## 完了

`gh pr create` が出力したURLを報告する（ブラウザは開かない）。
URLを取り直す場合:

```bash
gh pr view --json url -q .url
```

## 次のステップ

```text
✅ PRを作成しました: [URL]

次のステップ:
- Copilotのレビューを待つ場合（任意。head のローカルレビューの記録があればマージできる） → ~/.claude/scripts/gh-wait-review.sh <作成した PR の URL>（約 10 分。`run_in_background=true` で実行）
- Copilotレビュー/CIの状況を確認する場合 → /gh-actions-check
- レビューコメントに対応する場合 → /gh-pr-review
- レビュー後にマージする場合 → /gh-pr-merge
- 状態を確認する場合 → /git-info
```
