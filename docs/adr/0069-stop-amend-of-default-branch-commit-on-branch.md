---
status: accepted
date: 2026-10-30
requires: [ADR-0023]
supersedes: []
superseded-by:
issues: ["#538"]
---

# ADR-0069: 先端が既定ブランチに含まれる feature ブランチでの git commit --amend を hook で止める

## 背景

2026-10-30 01:1x（JST）、xlc の委任の担当が、main から作ったばかりのブランチで `git add docs/issues && git commit -q -m
"<件名 73 文字>" … && design-records check; echo rc=$?; …` を実行した。commit-msg の hook が「件名が 72 文字を超えています
(73 文字)」で拒んだが、`-q` と `;` のつなぎで失敗に気づかず、次の呼び出しの `git commit -q --amend -m …` が main の
マージコミット 4e9f726d8 を書き換えた a1a195361（親 2 つ）をブランチに作った。post-rewrite の `remap.commitRefs` は、issue の
本文の main のコミットの番号を a1a195361 に付け替えた。親のセッションが `reset --soft main` で 1 コミットに作り直した
（xlc main 63bbf7348 の第 2 親 a93780111）。

amend は新しいコミットを作るので main は変わらないが、ブランチの履歴の形と文書の番号が壊れる。一方、xlc にはマージの直後に
main の上でマージコミットの本文を amend で直した正当な例がある（reflog の 2026-10-21 02:02 と 02:05）。xlc には GitHub
リモートが無く、block-main-commit はそこでの main の commit を通す。

## 検討した案

- 案 1: 今のブランチが名前のある、既定ブランチでないブランチで、その先端が既定ブランチ（`git::default_branch`。
  guard-branch-base と同じ順で探す）に含まれる（`git merge-base --is-ancestor`）ときの `git commit --amend` を claude-hooks で
  止める。既定ブランチの上と detached HEAD（rebase の途中の edit）の amend、ブランチに固有のコミットがあるときの amend は通す
- 案 2: HEAD がマージコミットのときの amend を止める。既定ブランチの上の正当な amend（上の reflog）も止まり、fast-forward や
  squash でマージするリポジトリの既定ブランチのコミット（親 1 つ）は見逃す
- 案 3: 失敗を見落とす書き方（`git commit -q … ;`）を形で止める。今回の amend は別の呼び出しで、形からは見分けられない。
  `;` でつなぐ正当な形も多い
- 案 4: git の prepare-commit-msg の hook（第 2 引数 `commit` と第 3 引数のコミット）で amend を見分ける。githooks(5) は、
  `-m` / `-F` のときの第 2 引数を `message` と書き、`--amend -m`（今回の形）で `commit` になるかは確かめていない。
  リポジトリごとの hook の置き場（prek など）との重なりもある
- 案 5: 案 1 に加えて、同じコマンドの中で amend の前に先端か HEAD を動かしうる git のコマンド（commit、reset、checkout、
  rebase など）があれば止める（`git reset --soft main && git commit --amend` のような同じ行の形を拾う）。PR #539 で実装したが、
  reset と checkout の引数の解釈（パスか、コミットか、作業ツリーに無い名前か）が育ち、3 周のレビューの後の記録の周でも
  `git reset HEAD && git commit --amend` と、消した追跡ファイルを戻す `git checkout <file> && git commit --amend` を誤って止めた
  （#538）。同じ行の形は今回の事故（別の呼び出しの amend）と違い、hook の境界の外に置き、モデルの判断とレビューに任せる

## 決定と理由

案 1 を採る。守るものは既定ブランチのコミットがブランチで書き換えられないことで、それは先端が既定ブランチに含まれるかで
決まる。この状態で amend が正しい手順になる場面は無い（ブランチの最初のコミットは `git commit` で作り、既定ブランチの
コミットを直すなら既定ブランチの上で amend する）ので、block-main-commit や guard-git-stash と同じくエスケープは置かない
（ADR-0023 の形のエスケープは、外す理由がありうる検査のためのもの）。拒否の文言には、理由（2026-10-30 の xlc の #0203 の事故）と、
`git commit` で新しいコミットを作ること、拒まれた commit の後なら先に `git log -1` で先端を確かめること、既定ブランチの
コミットを直すなら既定ブランチの上で amend すること（GitHub リモートのあるリポジトリでは block-main-commit が止めるので
示さない）を書く。既定ブランチに含まれるかは、追跡ブランチとローカルの既定ブランチの両方で見る（push していないマージは
ローカルにだけある）。

判定は実行前の状態で行う。同じコマンドの前の `git switch` / `checkout` は、guard-branch-base の `branch_at` でその先の
ブランチを見る（移る先のブランチの ref を見て、まだ無いブランチを作って移るなら起点の今の HEAD を見る）。前の `git commit`
は見ない。前の commit が拒まれれば amend は実行前の HEAD を書き換えるので、`git commit -m a; git commit --amend` も止まる
（`&&` でつないだ同じ形も止まるが、続けて amend する書き方に正当な理由は乏しい）。

### 帰結

- 既定ブランチの ref は `origin/HEAD` → `origin/main` → … の順で探すので、origin があれば origin の追跡ブランチと比べる。
  fetch していない古い追跡ブランチでは、その後に既定ブランチに入ったコミットを先端に持つブランチを見逃す
- `git switch -c <新> <起点>` に続く amend は、起点でなく今の HEAD で判定する（起点を読まない）。`git switch -` などの
  移った先が分からない形に続く amend は通す
- `git -c alias.<名前>=` の alias と利用者の gitconfig の alias の amend は見ない

### 確認

- `.claude/tests/hooks.sh` の guard-git-amend の節（止める形と `-C` / `cd` / `env -C`、固有のコミット、既定ブランチの上と
  detached HEAD、`--amend` が値や pathspec の commit と `--no-am`、前の `git switch`、GitHub リモートのある origin/main との
  比較と文言、ローカルの既定ブランチにだけあるコミット）
