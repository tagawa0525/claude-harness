---
status: accepted
date: 2026-11-03
requires: [ADR-0015, ADR-0023, ADR-0042]
supersedes: []
superseded-by:
issues: []
---

# ADR-0072: セッションの scratchpad の下に git init で作った一時のリポジトリでは、git add -A と hook の検査を飛ばすコマンドを止めない

## 背景

claude-hooks の 2 つのルールは、どのリポジトリでも同じに止める。

- `guard-git-add`: パスを指定しない `git add -A` / `--all` / `.` / `:/` / `*`。未追跡のファイル（.env や鍵）を
  まとめてステージし、機密情報の混入と無関係な変更の混在を招くため
- `block-no-verify`: `git commit` / `merge` / `pull` の `--no-verify`、`-c core.hooksPath=…` など、git の hook の
  検査（機密情報、main への直接コミット、形式、ローカルのマージ）を飛ばすもの（ADR-0015）

2026-11-03 に、xlc のセッションのレビューの担当が、レビューの指摘を再現するために、セッションの scratchpad
（`/tmp/claude-1000/<プロジェクト>/<セッションの ID>/scratchpad/`）の下に `git init` で作ったリポジトリで
`git add .` / `-A` と `-c core.hooksPath=/dev/null` を実行し、止められた。/retro の担当が session-events.sh で
数えた値では、14:35〜16:55 の区間で 11 回、セッション全体で 37 回（拒否 101 回のうち）。担当はエスケープ
（ADR-0023）を使わず、毎回書き直していた（回数はこの ADR では数え直していない）。

2 つのルールが守るのは本番のリポジトリ（コミットが残り、push される）である。scratchpad の下の一時の
リポジトリは、レビューの再現やツールの試験のために作って捨てるもので、未追跡のファイルは担当が今作ったものだけ、
コミットはどこにも送られない。

親のセッションは、scratchpad の下に git init で作ったリポジトリでだけ `git add -A` と hooksPath の変更を通す
例外を、通す側を推して判断待ちの 1 つに挙げ、ユーザーは、挙げられた 3 つをどれも推しのとおりにすると
決めた（ユーザー 2026-11-03）。

## 決定の要因

- 本番のリポジトリ（`~/github/…`、`~/nix/…` とその worktree）では、今までどおり止める
- 一時のリポジトリでの普通の書き方（`cd <dir> && git add -A`、`git -C <dir> …`、cwd）を、書き直しやエスケープ
  なしに通す
- 例外がパスの書き方（`..`、symlink、展開）で本番のリポジトリに広がらない
- 拒否の文言から、例外があることと、このコマンドが当たらない理由が分かる
- hook は普通に書く形で守らせ、作為的な抜け道は追わない（ADR-0042）

## 検討した案

- 案 A: 例外を作らない。今までどおり止め、要るときはエスケープ（ADR-0023）で通す
- 案 B: scratchpad の下にあるディレクトリなら、リポジトリを問わず通す
- 案 C: scratchpad の下に git init で作った一時のリポジトリだけを通す。対象のディレクトリ、作業ツリーの根、
  git のディレクトリがすべて scratchpad の下にあり、リモートが無いことを確かめる
- 案 D: 案 C に加え、scratchpad の下に `git clone --shared` で作った本番のリポジトリの複製も通す

## 決定と理由

採用: 「案 C」。

例外の条件（`modules/home/parts/claude-hooks/src/rules/scratchpad_repo.rs`）。すべてを満たすときだけ通す:

- 対象のディレクトリ: コマンドの前の `cd`、`git -C`、`env -C` を hook の cwd に順に当てたもの（ほかのルールと同じ
  `Shell::target_dir`）。target_dir は pushd / popd をたどらないので、前に pushd / popd があれば通さない。symlink と `..` を解いた実際のパスで判定する。判定の時点に無いディレクトリ（同じコマンドの
  `mkdir` が作る前、展開や `~user` を含む書き方）は通さない
- scratchpad の見分け方: 祖先に `<セッションの ID>/scratchpad` の名前の並びがあること。セッションの ID は
  hook の入力の `session_id`。scratchpad の根の一時ディレクトリは環境変数（`CLAUDE_CODE_TMPDIR` など）で
  変わるので、根のパスでなく、この 2 つの名前で見分ける。`session_id` が無ければ通さない
- リポジトリ: 対象のディレクトリで `git rev-parse --show-toplevel --git-common-dir` が返す作業ツリーの根と
  git のディレクトリが、どちらも同じ scratchpad の下にあること。本番のリポジトリの worktree を scratchpad の
  下に作っても、git のディレクトリは本番の側にあるので通さない。リポジトリが無い（同じコマンドの `git init` の
  前）か、git が読めないときも通さない
- リモートが無いこと（`git remote` が成功して空）
- `GIT_DIR`、`GIT_WORK_TREE`、`GIT_COMMON_DIR` の代入と、`--git-dir`、`--work-tree` で、リポジトリを指し替えて
  いないこと

例外を当てる範囲:

- `guard-git-add`: 止める形のすべて（クォートが不整合で引数を読めないものは、対象のディレクトリも決められないので除く）
- `block-no-verify`: `git commit` / `merge` / `pull` に掛かる判定（`--no-verify`、`-c` / `--config-env` の
  core.hooksPath、`GIT_CONFIG_*`、`CLAUDECODE` の書き換え）。`git config core.hooksPath …` は、同じ形で
  `--global` も書けるので、scratchpad の下でも止める
- 1 つのコマンドに複数の git があれば、git ごとに判定する（scratchpad の commit の後に本番のリポジトリへの
  `--no-verify` があれば止める）

拒否の文言には、例外の 1 行（「セッションの scratchpad の下に git init で作った、リモートの無い一時のリポジトリは
対象外です」）と、そのコマンドが当たらない理由（scratchpad の外、リモートがある、まだ無いので mkdir と git init を
先に別の呼び出しで実行する、など）を足す。`block-no-verify` では、commit / merge / pull のうち最初に当たらなかった
ものの理由を出す（`git config` だけなら例外の行を出さない）。

理由:

- 案 A は、11 回の書き直しが示すとおり、担当は止められた形を毎回書き直す。ルールの目的に当たらないのに止めるのは
  誤判定で、ADR-0042 の「止める側の誤判定（普通の形を誤って止める）は直す」に当たる。エスケープは、理由が機械で
  判定できるなら hook の不足として直すもの（ADR-0023 の決定の要因）で、scratchpad の下かは機械で判定できる
- 案 B は、scratchpad の下に作った本番のリポジトリの worktree（コミットとブランチを本番の git のディレクトリに
  書く）と複製も通す
- 案 D は、複製の origin が本番のリポジトリを指し、hook を通らずに作ったコミット（機密情報の検査を飛ばしたもの）を
  `git push` でそのまま本番へ送れる。本番の側に受け取る検査は無く、`guard-git-push` は GitHub のリモートが無い
  リポジトリを見ない。複製でも `git add <file>` と hook を通すコミットは使えるので、止めても再現の作業は続けられる
- 判定の時点に無いディレクトリとリポジトリを通さないのは、このブランチの最初の版が通していたものを、
  ローカルのレビューの反例で改めたもの。最初の版は、同じコマンドの `mkdir -p X && cd X && git init && git add -A` を
  1 回で通すために、無いディレクトリはある祖先を解いて scratchpad の下なら通し、rev-parse が失敗すれば「リポジトリが
  無い」として通した。反例は、(1) `cd <scratchpad>/X; git add -A` で cd が失敗すると git は hook の cwd（本番の
  リポジトリ）で走る、(2) scratchpad の下の worktree や複製の中の、まだ無いサブディレクトリでは git が祖先の
  リポジトリを使う、(3) `cd ~user/…` を hook は cwd からの相対のパスとして読み、まだ無いパスとして scratchpad の下に
  見える、(4) rev-parse と `git remote` の失敗（壊れた設定など）を「無い」と読む。どれも本番のリポジトリに掛かる側に
  誤る。mkdir と git init を先の呼び出しに分ける手間は 1 回で、その後のコマンドは通る

### 帰結

- 良い点: レビューの再現のような一時のリポジトリでの作業が、mkdir と git init を先の呼び出しに分ければ、
  書き直しとエスケープなしに通る。拒否の文言が
  例外の条件と当たらない理由を示すので、本番のリポジトリで止まったときに、一時のリポジトリへ移すかを判断できる
- 悪い点: 判定のために、止める形のコマンドでだけ git を 2 回（`rev-parse` と `remote`）起動する
- 残る抜け道: 次の形は止めない（普通に書く形でなく、ADR-0042 で追わない）
  - リモートの無い一時のリポジトリから、パスを指定した `git push /home/…/repo …` か、同じコマンドの後で足した
    リモート（`git remote add`。判定はコマンドの前の `git remote` を読む）で本番へ送る
  - `GIT_INDEX_FILE`、`GIT_OBJECT_DIRECTORY` などで、git が書く場所の一部を本番のリポジトリに向ける
- 反映: hook のバイナリは `claude-code.nix` がビルドするので、マージの後に rebuild が要る

### 確認

- `scratchpad_repo.rs` の単体テストが、`cd` / `-C` / cwd で通すこと、本番のリポジトリ、scratchpad の下の
  worktree と複製（その中のまだ無いディレクトリを含む）、同じコマンドで作るディレクトリとリポジトリ、`..` と
  symlink と展開と `~user` と glob、git が読めないリポジトリ、別のセッション、`GIT_DIR` の指し替えで通さないことを示す
- `guard_git_add.rs` と `block_no_verify.rs` の単体テストが、ルールとして通す形と、止めたときの文言の理由を示す
- サブエージェントの呼び出しの hook の入力の `session_id` が、scratchpad のパスの ID（親のセッションの ID）と
  同じかは確かめていない（サブエージェントの環境変数 `CLAUDE_CODE_SESSION_ID` は親のものと同じだった）。違えば
  サブエージェントでは例外に当たらず、今までどおり止まる（緩む側には誤らない）
