---
status: accepted
date: 2026-10-24
requires: [ADR-0023]
supersedes: []
superseded-by:
issues: []
---

# ADR-0056: 新しい ADR の数を gh pr create でも数え、数えられなければ通す

## 背景

新しい ADR を 2 件以上加えるブランチ（1 ブランチ 1 トピックに反する目安）は、マージの時点で止まる
（`gh pr merge` の `pre-merge-check` の検証 9 と、ローカルの `git merge` の `pre-git-merge-check`）。2026-10-24 に、
/retro の担当が新しい ADR 2 件（ADR-0054、ADR-0055）を 1 本の PR #463 に入れ、レビューの周を回した後のマージで止められ、
2 本（#464、#465）に作り直した。回したレビューの周は無駄になった。

## 検討した案

- 案 A: `pre-pr-create-check` に同じ判定（`multi_topic_reason`、`ALLOW_MULTI_TOPIC=<理由>`）を足す。base は `--base`
  （`origin/<base>`）、無ければ既定ブランチ（`git::default_branch`）、head は `--head`（`origin/<head>`）、無ければ HEAD。
  base が手元に無いなど数えられなければ通す。fork（`upstream` リモートのある clone）では数えない
- 案 B: 案 A で、数えられなければ止める（マージの時点の 2 つの検査と同じく、確かめられない状態で進めない）
- 案 C: /gh-pr-create のスキルの手順に、PR を出す前に新しい ADR の数を確かめる手順を書く。hook を経由しない
- 案 D: 案 A で、docs/adr の無いリポジトリは数えず、docs/adr のあるリポジトリでは数えられなければ止める

## 決定と理由

採用: 「案 A」。マージの時点のゲートはそのまま残し、PR を作る時点では早めの知らせとして同じ判定を当てる。
エスケープと文言はマージの時点と共有する（ADR-0023 の形。条件は文言に列挙しない）。

採らなかった案:

- 案 B: ローカルのレビューで作り直す前の版。既定ブランチが main / master でなく origin/HEAD の無い clone（`git remote add`
  の後に push しただけのもの）や、fetch していない `--base` で、docs/adr の無い他のプロジェクトでも PR を作れなくなる
  （`.claude/tests/hooks.sh` の `prcreate-trunk` が反例）。PR の時点の検査はゲートではなく、ゲートは GitHub の変更ファイルから
  数える `pre-merge-check` なので、止める利点が誤って止める害に見合わない
- 案 C: スキルを経由しない `gh pr create`（サブエージェントの手順など）に効かない。判定は機械でできる
- 案 D: docs/adr のあるリポジトリでも、fetch していない `--base` や origin でない名前のリモートで数えられないことは
  ADR の数と関係が無く、止めると fetch やエスケープの往復が増える。ゲートはマージの時点にあるので、PR の時点は
  数えられたときだけ知らせれば足りる

### 帰結

- 良い点: 新しい ADR が 2 件以上のブランチは、レビューの周を回す前に止まる
- 悪い点: base を手元の追跡ブランチで数えるので、fetch していない古い base や、GitHub の既定ブランチと origin/HEAD の
  食い違いで、数がマージの時点と違うことがある。そのときもマージの時点の検査が正
- `owner:branch` の head（別のリポジトリのブランチ）と、cd 先に移れないコマンドでは数えない

### 確認

- `.claude/tests/hooks.sh` の pre-pr-create の節で、新しい ADR が 2 件以上なら ADR の一覧と /topic-triage と
  `ALLOW_MULTI_TOPIC=<理由>` を示して止め、1 件以下、fork、理由つきのエスケープ、base が見つからないときは通すことを確かめる
