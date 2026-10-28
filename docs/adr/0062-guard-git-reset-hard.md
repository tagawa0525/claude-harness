---
status: accepted
date: 2026-10-28
requires: []
supersedes: []
superseded-by:
issues: []
---

# ADR-0062: 未コミットの変更がある worktree での git reset --hard を止める

## 背景

2026-10-28 14:02、xlc のセッションで、/code-review のサブエージェントが、一時のリポジトリで pre-commit の振る舞いを
試すために `d=<scratchpad>/prektest; cd $d && git add … && git commit …; git log …; git rm … && prek run …; git reset -q --hard; …`
を実行した。直前のコマンドが hook（guard-git-add）に止められてディレクトリが作られておらず、`cd` が失敗した。`;` の後の
`git reset -q --hard` は 2 回とも hook の cwd（xlc のメインの worktree。ほかのセッションがマージや編集をする場所）で走った
（reflog の `reset: moving to HEAD` が 2 件）。メインの worktree はたまたま変更が無く、害は無かった。

`reset --hard` は、追跡しているファイルの未コミットの変更を戻せない形で消す。worktree はほかのセッションと共有しうるので、
消える変更がそのコマンドを書いたセッションのものとは限らない。/code-review は Claude Code の組み込みのスキルで、手順に
`reset --hard` は無い。レビューの担当がその場で書いたコマンドだった。

## 検討した案

- 案 1: メインの worktree（`--git-dir` と `--git-common-dir` が同じ）での `reset --hard` を、変更の有無によらず止める。
  一時のリポジトリもメインの worktree なので、試行の後片付けまで止まる。ほかのセッションの変更は linked worktree にもありうる
- 案 2: 対象の worktree に追跡しているファイルの未コミットの変更があるときだけ止める。対象は前の `cd` と `-C` を当てた
  ディレクトリ（`Shell::target_dir`）。今回の形は `cd` が失敗して hook の cwd で走るので、対象だけを見ると、一時のリポジトリ
  （まだ無いか、変更の無い）を見て通してしまう
- 案 3: 案 2 に加え、`-C` を書いていなければ hook の cwd も見る。`cd x && git reset --hard` のように `&&` で正しくつないだ
  形でも、cwd に変更があれば止まる（誤検出）。`git -C <dir> reset --hard` に書き直せば通り、こちらは `cd` の成否に依らない
- 案 4: `cd` の後に `;` でつないだ `reset --hard` を形で止める。シェルの木から `cd` の失敗で走る位置を割り出す必要があり、
  `cd a; cd b`、`||`、`if` などの形を数えきれない。消える変更が無い場合も止める

## 決定と理由

案 3 を採る。守るものは未コミットの変更なので、変更の有無で判定する（案 1 と案 4 は変更が無くても止める）。hook の cwd を
見ない案 2 は、今回の形を通す。案 3 の誤検出は、`-C` で書き直す 1 手で抜けられ、書き直した形は `cd` の失敗に依らないので、
拒否の文言の修正方法にそれを書く。追跡していないファイルは `reset --hard` が消さないので数えない
（`git status --porcelain --untracked-files=no`）。まだ無いディレクトリは見ない（`git::not_a_directory`。作ってから入る形が
ある）。消してよい変更なら `ALLOW_GIT_RESET_HARD=<理由>` で外す。

### 帰結

- `git checkout -- <path>`、`git restore`、`git clean -f` も未コミットの変更や追跡していないファイルを消すが、今回は対象に
  しない。起きたら同じ形で足す
- 文字列から実行するプログラムを決められない形（`$CMD`）は見ない

### 確認

- `.claude/tests/hooks.sh` の guard-git-reset-hard の節（対象の変更、`cd` の失敗の形、`-C` で通る、追跡していないファイルと
  `--hard` でない reset は通す、エスケープ）
