---
status: accepted
date: 2026-11-06
requires: [ADR-0042, ADR-0069, ADR-0070]
supersedes: []
superseded-by:
issues: ["#595"]
---

# ADR-0075: GitHub のリモートが無いリポジトリでも main / master への直接のコミットを止める

## 背景

main / master への直接のコミットは、2 つの層で止めている。claude-hooks の `block-main-commit`（PreToolUse。
`modules/home/parts/claude-hooks/src/rules/block_main_commit.rs`）と、git の pre-commit の hook
（`modules/home/parts/git.nix`。Claude Code のセッションのコミットだけ）で、例外は 2 つの層で揃え、
`modules/home/parts/tests/main-commit-gates.sh` が一致を確かめる。どちらも、リモートに `github.com` が無いリポジトリを
対象から外していた。外した理由はコードのコメントに「GitHub リモートがなければ PR フロー適用外（PR を作れない以上
feature branch を強制してもマージする手段がない）」とある（git の層は 2026-09-02 の 079b47f0、PreToolUse の層は
2026-09-05 の移植 a3804c81）。

今は /git-merge が、GitHub のリモートが無いリポジトリでもブランチをローカルの main にマージする（マージの本文の
`Reviewed:` を hook が確かめる。ADR-0070）。外す理由は成り立たない。xlc（GitHub のリモートが無い）の main で
2026-10 以降にマージでないコミットは、最初の `chore: initialize cargo project`（d0bd72572、2026-10-06）だけで
（`git log --first-parent --no-merges main`。2026-11-06 に確かめた）、main に直接コミットする手順は無い。

2026-11-06 00:59 に、nixfiles の PR #594 の担当が、scratchpad の一時のリポジトリで git の hook を試すコマンドを投げた。
先の呼び出しが hook に止められてディレクトリが作られておらず、1 行目の `cd <scratchpad>/gitexp && …` が失敗した。
改行の後の `git commit -q --allow-empty …` は hook の cwd（xlc の main の worktree）で走り、xlc の main に空のコミットが
3 つ（124590f93、576250860、bc9a6c768）できた。担当が `git update-ref` で戻した（#595）。git の層はリポジトリの中で
走るので、`cd` の失敗に依らず止められた。PreToolUse の層は `cd` の先を対象にするので、`cd` が失敗した形では対象を
誤る（ADR-0062、ADR-0072 に続く例）。

親のセッションが、GitHub のリモートが無いリポジトリでも main への直接のコミットを止め、通すのは flake.lock だけの変更、
マージを締めるコミット、最初のコミットの 3 つとし、手元のほかのリポジトリも止まる側に入る案を推し、ユーザーは
「1は提案に従います。」（ユーザー 2026-11-06）と決めた。

## 決定の要因

- `cd` の失敗のような書き方の誤りで、別のリポジトリの main にコミットが入らない
- 正規の手順（/git-merge のマージ、`nix-rebuild update` の flake.lock、新しいリポジトリの最初のコミット）は止めない
- 2 つの層の例外は同じにする（片方だけが塞ぐと作業が止まる。PR #165）
- hook は機械的に判定できる禁止だけを、普通に書く形で守らせる（ADR-0042）

## 検討した案

- 案 1: 今のまま、GitHub のリモートが無いリポジトリを外す。今回の形を通す
- 案 2: 両方の層で、リモートによらず main / master への直接のコミットを止める。通すのは、flake.lock だけの変更、
  マージを締めるコミット（MERGE_HEAD がある）、HEAD の無いリポジトリの最初のコミットの 3 つ
- 案 3: git の層だけで止める。git の層は `cd` の失敗に依らないが、2 つの層の例外が食い違い、main-commit-gates.sh の
  一致の確認が成り立たない。`-c core.hooksPath=…` で git の hook を飛ばす形（scratchpad の一時のリポジトリでは
  ADR-0072 が通す）には PreToolUse の層しか残らない
- 案 4: 案 2 に加え、scratchpad の下に `git init` で作った一時のリポジトリ（ADR-0072 の見分け方）も通す。git の層は
  hook の入力の `session_id` を持たず、同じ見分け方ができない

## 決定と理由

案 2 を採る。外していた理由（マージの手段が無い）は /git-merge で無くなり、外す範囲が今回の事故の通り道になった。
git の層は対象のリポジトリの中で走るので、`cd` が失敗した形でも止まる。PreToolUse の層も同じ例外で止め、2 つの層の
判定を main-commit-gates.sh で揃える。

通す 3 つと、その理由:

- flake.lock だけの変更（`git diff --cached --name-status` が `M\tflake.lock` の 1 行）: `nix-rebuild update` の正規の
  手順（今と同じ）
- マージを締めるコミット（`git rev-parse -q --verify MERGE_HEAD` が成功する）: /git-merge の「マージ」の節は、本文の語で
  拒まれたマージを `git -C [DEFAULT_WORKTREE] commit -m "…"` で終える。競合を解決した後の `git commit` /
  `git merge --continue` も同じ形になる。git はこのコミットで pre-merge-commit を呼ばないので、git の層の pre-commit が
  同じ検査を呼ぶ（今と同じ）
- HEAD の無いリポジトリの最初のコミット（`git rev-parse -q --verify HEAD` が失敗する）: 分けるブランチの起点が無い

これ以外の例外は足さない。案 4 は 2 つの層で同じ判定にできない。一時のリポジトリは main / master 以外のブランチで
作れば（`git init -b <名前>`、`git switch -c <名前>`）止まらないので、拒否の文言にその書き方を示す。

拒否の文言（2 つの層で同じ趣旨）には、理由（GitHub のリモートが無くても /git-merge でブランチをマージするので、main に
直接コミットする理由が無い。2026-11-06 に、`cd` が失敗した後の `git commit` が別のリポジトリの main に空のコミットを
作った）と、修正方法（ブランチを作る。対象のディレクトリを確かめる。一時のリポジトリならほかの名前のブランチで作る）を
書く。PC 全体で読む文言なので、ADR の番号は書かない。

ADR-0069 の拒否の文言は、既定ブランチの上で amend する道を「GitHub リモートのあるリポジトリの main / master」では
示さないとしていた。main / master はリモートによらず止まるので、main / master では示さない形に直す（ADR-0069 の
「## 変更」）。

### 帰結

- GitHub のリモートが無いリポジトリでも、main / master の上の `git commit --amend` は止まる。xlc の 2026-10-21 の
  マージコミットの本文の amend（ADR-0069 の背景）のような直しは、できなくなる。マージの本文は、マージの前に hook が
  確かめる
- `git merge --no-commit` の後に別の変更をステージしてから締めるコミットは、MERGE_HEAD があるので通る。普通の手順の
  形ではないので塞がない（ADR-0042）
- MERGE_HEAD の無い cherry-pick / revert / `merge --squash` の後のコミットは止まる
- scratchpad の一時のリポジトリで main のまま 2 つ目のコミットを作る形は止まる（最初のコミットは通る）
- `guard-git-push`（main への push）は、今のまま GitHub のリモートがあるリポジトリだけに効く
- 手元の GitHub のリモートが無いほかのリポジトリ: `~/github/waypipe`（GitLab の origin。CLAUDE.md もスキルも無く、
  Claude Code のセッションのコミットは feature ブランチにある）。`~/github/Implem.Pleasanter-cs` は gitdir の指す先が無く、
  リポジトリとして読めない（どちらも 2026-11-06 に確かめた）

### 確認

- `modules/home/parts/tests/main-commit-gates.sh`: リモートなしと GitHub 以外のリモートの main の通常の変更、
  flake.lock だけ、MERGE_HEAD のあるコミット、HEAD の無いリポジトリで、2 つの層が同じ判定になる
- `.claude/tests/hooks.sh` の block-main-commit と guard-git-amend の節
