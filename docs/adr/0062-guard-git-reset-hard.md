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
- 案 3: 案 2 に加え、絶対パスの `git -C` / `env -C` で指していなければ、reset が走りうるほかのディレクトリも見る。`cd` が
  失敗すると、reset は最後に通った `cd` の先（無ければ hook の cwd）で走るので、hook の cwd と、前の各 `cd` の直前の
  ディレクトリ（`bash -c` の中の `cd` なら外側の `cd` の先も）を見る。`cd x && git reset --hard` のように `&&` で正しく
  つないだ形でも、cwd に変更があれば止まる（誤検出）。`git -C <絶対パス> reset --hard` に書き直せば通り、こちらは `cd` の
  成否に依らない。書き方からディレクトリを決められない形（変数のパス、`--git-dir` / `--work-tree`、`GIT_DIR` /
  `GIT_WORK_TREE`、変数に入れたコマンド）は止める側に倒す（ADR-0025）
- 案 4: `cd` の後に `;` でつないだ `reset --hard` を形で止める。シェルの木から `cd` の失敗で走る位置を割り出す必要があり、
  `cd a; cd b`、`||`、`if` などの形を数えきれない。消える変更が無い場合も止める

## 決定と理由

案 3 を採る。守るものは未コミットの変更なので、変更の有無で判定する（案 1 と案 4 は変更が無くても止める）。hook の cwd を
見ない案 2 は、今回の形を通す。案 3 の誤検出は、`-C` で書き直す 1 手で抜けられ、書き直した形は `cd` の失敗に依らないので、
拒否の文言の修正方法にそれを書く。前に `cd` が無い reset は、包んだ `env -C` と `git -C` だけでディレクトリが決まるので、
そのディレクトリだけを見る。追跡していないファイルは数えない（`git --no-optional-locks status --porcelain
--untracked-files=no`。ほかのセッションの worktree の index.lock を取らない）。サブモジュールの中の変更は、
`submodule.recurse` の設定があれば reset --hard が消すので数える（設定を読み分けず、止める側に倒す）。worktree の中で
`git status` が失敗するときは、消えるものを確かめられないので止める。まだ無いディレクトリは見ない（`git::not_a_directory`。
作ってから入る形がある）。ssh と `sudo -i` の中の reset は手元の worktree ではないので見ない。`--ha` などの省略形と、
`git -c alias.<名前>=` で書いた alias も reset --hard として見る。消してよい変更なら `ALLOW_GIT_RESET_HARD=<理由>` で外す。

最初の実装は対象と hook の cwd だけを見ていた。ローカルのレビュー（2026-10-28）が、一時のリポジトリで hook を実行して、
前の `cd` が通った後の `cd` の失敗、`bash -c` の中の `cd` の失敗、`--har`、`-c` の alias、変数のパス、`--git-dir`、
`GIT_DIR`、変数に入れたコマンドが変更を消すのに通ることと、絶対パスの `env -C` と ssh の先が cwd の変更で止まることを
示したので、今の形にした。2 周目のレビューは、`reset` の語で決められないコマンドを止める判定が `preset` などで誤検出すること
（`--hard` の語に替えた）、前に `cd` の無い相対の `-C` と包んだ `env -C` を cwd の変更で止めること、`export GIT_DIR`、
`pushd`、`submodule.recurse` の見逃しを示したので、直した。

### 帰結

- `git checkout -- <path>`、`git restore`、`git clean -f` も未コミットの変更や追跡していないファイルを消すが、今回は対象に
  しない。起きたら同じ形で足す
- 利用者の gitconfig の alias（`rh = reset --hard`）は見ない。読むには hook が設定を解釈する必要があり、今は使っていない
- 実行するプログラムを決められないコマンドがあり、文字列に `--hard` があれば止まる（`$CMD` で別のことをするコマンドでも）
- `git reset --hard <commit>` は、<commit> が追跡するパスにある追跡していないファイルも上書きする。追跡していないファイルを
  数えないので、これは見逃す（2 周目のレビューが示した。起きたら、対象のコミットの木と `ls-files --others` を比べる形で足す）

### 確認

- `.claude/tests/hooks.sh` の guard-git-reset-hard の節（対象の変更、`cd` の失敗の形、前の `cd` が通った後の失敗と
  `bash -c` の中、省略形と alias、決められない書き方、絶対パスの `-C` / `env -C` と ssh で通る、追跡していないファイルと
  `--hard` でない reset は通す、エスケープ）
