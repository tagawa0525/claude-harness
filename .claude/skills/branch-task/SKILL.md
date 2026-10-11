---
name: branch-task
description: 親のセッションから頼まれた 1 トピックのブランチ作業を、worktree の作成からチェックとレビューまで進め、マージの直前で止めて決まった形で報告する。サブエージェントへの依頼の定型。
# model 未指定: モデルは親がサブエージェントを起動するときに決める（Agent の model）
argument-hint: <リポジトリ> <ブランチ名> <何をするか>
allowed-tools:
  - Bash(git *)
  - Bash(gh *)
  - Bash(cd *)
  - Bash(~/.claude/scripts/worktree-add.sh*)
  - Bash(~/.claude/scripts/review-level.sh*)
  - Bash(~/.claude/scripts/gh-wait-review.sh*)
  - Bash(~/.claude/skills/language-checks/scripts/run-checks.sh*)
  - Bash(~/.claude/skills/gh-pr-review/scripts/*)
  - Skill(code-review)
  - Skill(adr)
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
ただし「マージの直前で止める」と共通の禁止事項は、依頼でも変えない（理由は nixfiles の ADR-0027）。

サブエージェントの cwd は作業する worktree ではない。スキルの `!` 行と引数なしのスクリプトは cwd の
ブランチを見るので、/gh-pr-create と /gh-pr-review は Skill で呼ばずに Read で読み、
その中のコマンドを `cd <worktree> &&` を付け、ブランチや PR 番号を明示して走らせる。

## 進める側（サブエージェント）の手順

### 1. 読む

- 対象リポジトリの CLAUDE.md（`.claude/CLAUDE.md` を含む）と、依頼が指す文書（ADR、issue、計画）を読む
- 重い計算、検証の記録、試行の置き場など、リポジトリ固有の規則は CLAUDE.md に従う
- 依頼の内容が CLAUDE.md、採用中の ADR、コードの事実と食い違えば、作業を始める前に報告に書いて止まる
  （推測で一方に合わせない）

### 2. 比較元を決め、担当を確かめる

GitHub リモートがあれば（`git -C <リポジトリ> remote -v | grep -q 'github\.com'`）先に fetch する。古い追跡
ブランチと比べると、main の遅れを見落とし、他のセッションのコミットをブランチのものと取り違える:

```bash
git -C <リポジトリ> fetch
```

比較元（`<base>`）は、リモートがあれば `origin/<既定ブランチ>`、なければ `<既定ブランチ>`。チェック、レビュー、
報告で同じものを使う。

ブランチが既にある（ローカルかリモート）なら、worktree を作る前に、ブランチ自身のコミットの
`Claude-Session:` が今のセッションか確かめる:

```bash
git -C <リポジトリ> log --format='%h %(trailers:key=Claude-Session)' <base>..<ブランチ>
```

違うセッションのもの、またはコミットが無く担当が分からないものは、引き受けず報告に書いて止まる。

### 3. worktree を用意する

ブランチがもうどこかの worktree にあれば（`git -C <リポジトリ> worktree list`）、その worktree を使う。
無ければ作る:

```bash
cd <リポジトリ> && ~/.claude/scripts/worktree-add.sh <ブランチ名>
```

- 出力の `WORKTREE:` 以降はそこで作業する。Bash の cwd は呼び出しごとに戻るので、`git -C <worktree>` か
  `cd <worktree> &&` を付ける
- `SHARED:`（共有した git で追跡しないパス）と `NOTE:`（メイン worktree に無いパス、複数の GitHub リモート）を
  読む。`ERROR:` なら /git-worktree の直し方に従う

### 4. 作業する

- テストで確かめられる振る舞いの変更は TDD にする。失敗するテスト（RED）と実装（GREEN）を別のコミットにし、
  件名も別にする
- 決定が要るものは /adr の形で ADR に書く。依頼で決まっていない決定を、依頼の判断の軸から下したときは、
  報告の「決めたことと根拠」に書く
- 作業中に見つかった問題は、/topic-triage の「振り分け」の表と目安だけを Read で読んで振り分ける（そこにある
  別の worktree での片付けとマージ、rebase は行わない）。同じトピックなら続け、止めない別のトピックは issue に
  書いて今のトピックに戻り、止める別のトピックは issue に書いて報告に書いて止まる
- 依頼の範囲を超える変更は入れない。入れざるを得なかったものは、報告で分けて書く

### 5. コミットする

- 論理的な変更ごとに 1 コミット。Conventional Commits の件名、本文に Why（決定は ADR を参照）
- issue や ADR に対応するときは `Refs:` を付ける（書式はリポジトリの規則に従う）
- 依頼か system-reminder で指定された trailer（`Co-Authored-By:`、`Claude-Session:` など）を末尾に付ける
- hook に止められたら、示された直し方に従う。直せなければ、迂回せず（文言や書き方を変えない、エスケープを
  付けない）報告に書いて止まる

### 6. チェックする

```bash
~/.claude/skills/language-checks/scripts/run-checks.sh -C <worktree> --base <base>
```

終了コードで判定し、出力を `tail` などにつながない。失敗したら原因を直してコミットし、やり直す。CLAUDE.md に
チェックの方法が書かれていれば、それも走らせる。

### 7. レビューを受ける

ローカルのレビューは、どちらの場合も先に回す:

```bash
cd <worktree> && ~/.claude/scripts/review-level.sh <ブランチ名> --base <base>
```

出力の `LEVEL` で `/code-review <LEVEL> <worktree のパス>` を回す。対象は `TARGET` のブランチ名でなく worktree の
パスで渡す。/code-review は cwd のリポジトリでブランチ名を解くので、cwd が別のリポジトリだと空振りして
「指摘なし」と返る（2026-10-12 に 2 回起きた）。

**リモートあり**のときは、続けて PR を出して自動レビューを受ける:

- /gh-pr-create を Read で読み、その形（タイトル 70 文字以内、`## Summary` / `## Changes` / `## Tests`）で
  PR を作る。`cd <worktree> && git push -u origin <ブランチ名>` と `cd <worktree> && gh pr create …` は別々の
  呼び出しにする（push 前の状態で hook が止める）
- `~/.claude/scripts/gh-wait-review.sh <PR の URL>`（URL は `gh pr create` の出力）を `run_in_background=true`
  で走らせて待つ。待つ間に最後の報告を返さない（待機の終わりの通知で再開する）
- /gh-pr-review を Read で読み、その手順で対応する。スクリプトには PR の URL を渡す。周の終わりは
  `decide-next.sh` の `VERDICT` に従う。最後の分岐は次に置き換える: 要判断（escalate）はユーザーの指示を
  待たず報告の「マージの前後で人の手が要ること」に書いて止まり、/gh-pr-merge には進まない

どちらも:

- 直すのは検証できる誤り（バグ、コードや事実と食い違う記述、壊れた参照）だけ。言い回しや網羅性の指摘で
  周を回し続けない
- 直したらコミットし、チェックからやり直す（rebase はしない）。その周に検証できる誤りが無ければ終わり
  （レビューは周ごとに新しい候補を出すので、指摘ゼロを待たない）
- 対応しなかった指摘は、理由（検証で得た事実）を報告に書く
- レビューを受けずに残ったコミット（`STOP_LIMIT` の後に直したものなど）は、報告に列挙する

### 8. マージの直前で止める

`git merge`、`gh pr merge`、/git-merge、/gh-pr-merge を実行しない。rebase もしない。これを止める hook は無く、
許可も通るので、手順で守る。main が先に進んでいれば（`git -C <worktree> rev-list --count HEAD..<base>` が
0 でない）、件数を報告に書く。

### 9. 報告する

最後の報告は次の見出しで書く。該当が無い節は「なし」と書き、節を省かない。

```markdown
## ブランチと worktree
- リポジトリ: <path>
- ブランチ: <branch>（<base> から N コミット遅れ）
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
最初に /branch-task を呼び、その手順に従って進める。

リポジトリ: <path>
ブランチ: <branch>
何をするか: <目的と範囲。決定済みの内容（ADR、issue、ユーザーとの合意）と、判断が要るときの軸>
禁止事項: <このトピックに固有のもの。共通のものはスキルにある>
```

- モデルは依頼文ではなく、サブエージェントを起動するときの指定（Agent の `model`）で渡す
- 決定済みの内容は、どこで決まったか（ADR の番号、ユーザーの言葉）を添える。サブエージェントは会話を見ていない
- hook のエスケープの付け方や「〜なら付けてよい」は依頼に書かない（nixfiles の ADR-0023）
- 報告は確かめる前の暫定として扱う。マージの前に、報告のコミットと確認の結果を自分で確かめる
