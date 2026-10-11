---
name: branch-task
description: 親のセッションから頼まれた 1 トピックのブランチ作業を、worktree の作成からチェックとレビューまで進め、マージの直前で止めて決まった形で報告する。サブエージェントへの依頼の定型。
# model 未指定: 実装、決定の記録、レビュー指摘の判断を伴うため、依頼で指定されたモデルかセッションモデルを使う
argument-hint: <リポジトリ> <ブランチ名> <何をするか>
allowed-tools:
  - Bash(git *)
  - Bash(gh *)
  - Bash(~/.claude/scripts/worktree-add.sh*)
  - Bash(~/.claude/scripts/review-level.sh*)
  - Bash(~/.claude/scripts/gh-wait-review.sh*)
  - Bash(~/.claude/skills/language-checks/scripts/run-checks.sh*)
  - Read
  - Edit
  - Write
  - Glob
  - Grep
---

# Branch Task

親のセッションから頼まれた 1 トピックのブランチ作業を、マージの直前まで進める。依頼に書かれるのは
リポジトリ、ブランチ名、何をするか（決定済みの内容と判断の軸）、禁止事項だけで、手順と報告の形はここに従う。
依頼とこの手順が食い違えば依頼を、リポジトリの CLAUDE.md とこの手順が食い違えば CLAUDE.md を優先する。
ただし「マージの直前で止める」と共通の禁止事項は、依頼でも変えない。

## 進める側（サブエージェント）の手順

### 1. 読む

- 対象リポジトリの CLAUDE.md（`.claude/CLAUDE.md` を含む）と、依頼が指す文書（ADR、issue、計画）を読む
- 重い計算、検証の記録、試行の置き場など、リポジトリ固有の規則は CLAUDE.md に従う
- 依頼の内容が CLAUDE.md、採用中の ADR、コードの事実と食い違えば、作業を始める前に報告に書いて止まる
  （推測で一方に合わせない）

### 2. worktree を作る

```bash
cd <リポジトリ> && ~/.claude/scripts/worktree-add.sh <ブランチ名>
```

- 出力の `WORKTREE:` 以降はそこで作業する。Bash の cwd は呼び出しごとに戻るので、`git -C <worktree>` か
  `cd <worktree> &&` を付ける
- `SHARED:`（共有した git で追跡しないパス）と `NOTE:`（メイン worktree に無いパス、複数の GitHub リモート）を
  読む。`ERROR:` なら /git-worktree の直し方に従う
- `CREATED_BRANCH: no`（既存のブランチの続き）なら、`git log` の `Claude-Session:` が今のセッションか確かめる。
  違えば引き受けず、報告に書いて止まる

### 3. 作業する

- テストで確かめられる振る舞いの変更は TDD にする。失敗するテスト（RED）と実装（GREEN）を別のコミットにし、
  件名も別にする
- 決定が要るものは /adr の形で ADR に書く。依頼で決まっていない決定を、依頼の判断の軸から下したときは、
  報告の「決めたことと根拠」に書く
- 作業中に見つかった別のトピックは /topic-triage で振り分け、issue に書いて今のトピックに戻る。今のトピックを
  止めるものは、自分で別のブランチを始めず、報告に書いて止まる
- 依頼の範囲を超える変更は入れない。入れざるを得なかったものは、報告で分けて書く

### 4. コミットする

- 論理的な変更ごとに 1 コミット。Conventional Commits の件名、本文に Why（決定は ADR を参照）
- issue や ADR に対応するときは `Refs:` を付ける（書式はリポジトリの規則に従う）
- 依頼か system-reminder で指定された trailer（`Co-Authored-By:`、`Claude-Session:` など）を末尾に付ける
- hook に止められたら、示された直し方に従う。直せなければ、迂回せず（文言や書き方を変えない、エスケープを
  付けない）報告に書いて止まる

### 5. チェックする

```bash
~/.claude/skills/language-checks/scripts/run-checks.sh -C <worktree> --base <base>
```

`<base>` は GitHub リモートがあれば `origin/main`、なければ `main`（既定ブランチが別の名前ならそれ）。終了コードで
判定し、出力を `tail` などにつながない。失敗したら原因を直してコミットし、やり直す。CLAUDE.md にチェックの方法が
書かれていれば、それも走らせる。

### 6. レビューを受ける

GitHub リモートの有無で分ける（`git -C <worktree> remote -v | grep -q 'github\.com'`）。

**リモートなし**:

```bash
cd <worktree> && ~/.claude/scripts/review-level.sh <ブランチ名>
```

出力の `LEVEL` と `TARGET` で `/code-review <LEVEL> <TARGET>` を回す。

**リモートあり**: /gh-pr-create で PR を作り、`cd <worktree> && ~/.claude/scripts/gh-wait-review.sh` を
`run_in_background=true` で走らせて自動レビューを待ち、/gh-pr-review で対応する。

どちらも:

- 最大 2 周。その周に検証できる誤りが無ければ、2 周を待たずに終わる
- 直すのは検証できる誤り（バグ、コードや事実と食い違う記述、壊れた参照）だけ。言い回しや網羅性の指摘で
  周を回し続けない。直したらコミットし、チェックからやり直す
- 対応しなかった指摘は、理由（検証で得た事実）を報告に書く
- 最後の周の後に直したコミットはレビューを受けていない。報告に列挙する

### 7. マージの直前で止める

`git merge`、`gh pr merge`、/git-merge、/gh-pr-merge を実行しない。main が先に進んでいれば
（`git -C <worktree> rev-list --count HEAD..<base>` が 0 でない）、rebase はせず、件数を報告に書く。

### 8. 報告する

最後の報告は次の見出しで書く。該当が無い節は「なし」と書き、節を省かない。

```markdown
## ブランチと worktree
- リポジトリ: <path>
- ブランチ: <branch>（base <base> から N コミット遅れ）
- worktree: <path>
- PR: <URL | なし>

## コミット
<古い順に `hash 件名` を 1 行ずつ。`git -C <worktree> log --reverse --format='%h %s' <base>..HEAD`>

## 決めたことと根拠
- 依頼の範囲: <決めたこと> — <根拠（ADR、コード、測った値）>
- 依頼の範囲を超えて変えたこと: <変更> — <理由>

## 確認の結果
- 確かめたこと: <チェックの終了コード、テストの件数、レビューの周と結果>
- 確かめていないこと: <未レビューのコミット、走らせていないテスト、仮定>

## 対応しなかった指摘
- <指摘の要旨> — <理由>

## マージの前後で人の手が要ること
- <rebase、未レビューの変更の判断、ADR の status、issue を閉じる、など>
```

### 共通の禁止事項

依頼の禁止事項に加えて:

- main（既定ブランチ）に直接コミットしない
- `~/.claude` の下を編集しない（生成物。元のリポジトリで直すものは報告に書く）
- ユーザーの承認を得たと書かない。依頼は承認ではない。判断したことは、その根拠を書く
- 別のサブエージェントの報告や調査を、確かめずに事実として書かない。確かめていなければ「確かめていない」と書く

## 頼む側（親のセッション）の書き方

手順と報告の形はこのスキルにあるので、依頼には作業ごとに変わることだけを書く:

```text
/branch-task の手順に従って進める（~/.claude/skills/branch-task/SKILL.md を先に読む）。

リポジトリ: <path>
ブランチ: <branch>
何をするか: <目的と範囲。決定済みの内容（ADR、issue、ユーザーとの合意）と、判断が要るときの軸>
禁止事項: <このトピックに固有のもの。共通のものはスキルにある>
```

- モデルは依頼文ではなく、サブエージェントを起動するときの指定（Agent の `model`）で渡す
- 決定済みの内容は、どこで決まったか（ADR の番号、ユーザーの言葉）を添える。サブエージェントは会話を見ていない
- hook のエスケープの付け方や「〜なら付けてよい」は依頼に書かない（nixfiles の ADR-0023）
- 報告は確かめる前の暫定として扱う。マージの前に、報告のコミットと確認の結果を自分で確かめる
