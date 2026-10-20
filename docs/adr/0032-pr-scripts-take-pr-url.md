---
status: accepted
date: 2026-10-12
requires: []
supersedes: []
superseded-by:
issues: ["#339"]
---

# ADR-0032: PR のスクリプトは PR の URL で対象を受け、cwd のリポジトリに頼らない

## 背景

`gh-wait-review.sh`、`gh-review-requests.sh` と `gh-pr-review/scripts/` のスクリプトは、PR 番号を受け、`gh pr view <番号>`
と `gh api repos/{owner}/{repo}/...` で cwd のリポジトリの PR として読んでいた。Bash ツールの cwd は呼び出しごとに戻るので、
`cd <worktree> &&` を付け忘れると別のリポジトリで走る。GitHub のリモートが無いリポジトリなら `gh-wait-review.sh` は
`SKIP`（終了コード 2）で終わり、レビューを待たずに進む（2026-10-12 に PR #337 の待機で起きた）。別の GitHub の
リポジトリなら、同じ番号の別の PR を読み、返信や resolve までしうる。

CLAUDE.md に「PR のリポジトリの worktree で実行する」と書いた（#323）後も起きたので、文章の注意では防げない。

## 検討した案

- 案 A: スクリプトは第 1 引数に PR の URL（`https://github.com/<owner>/<repo>/pull/<番号>`）を必須で受け、
  `GH_REPO=<owner>/<repo>` を export して、以降の `gh` の呼び出しをすべてそのリポジトリに向ける
- 案 B: 案 A に加えて、`-R <owner>/<repo> <番号>` の形も受ける
- 案 C: claude-hooks に、PR のスクリプトを実行する先（cd と -C を畳み込んだ先）に GitHub のリモートが無ければ止める
  ルールを足す（2026-10-12 に作り、push せずに捨てた。#339）
- 案 D: 今のまま、CLAUDE.md とスキルの注意で `cd <worktree> &&` を促す

## 決定と理由

採用: 「案 A」。

- PR の URL は対象のリポジトリと番号を 1 つで決める。`gh pr create` が出力し、Copilot のコメントの URL もこの形を
  含むので、呼び出す側は手元にある値をそのまま渡せる
- `gh` は `GH_REPO` を `gh pr` の対象にも `gh api` の `{owner}` / `{repo}` にも使う（git のリポジトリの外でも効くことを
  2026-10-12 に `GH_REPO=cli/cli gh api 'repos/{owner}/{repo}'` と `gh api graphql -F owner='{owner}'` で確かめた）。
  スクリプトの中の個々の呼び出しを書き換えずに、cwd への依存をまとめて断てる
- `GH_HOST=github.com` も export する。`gh api` は `GH_REPO` のホストより環境の `GH_HOST` を使う
  （`GH_HOST=ghe.example.com GH_REPO=github.com/cli/cli gh api …` が ghe.example.com へつなぐことを 2026-10-12 に確かめた）
- 案内と下請けのスクリプトには、受けた URL からページ・クエリ・`#` 以降を除いた PR の URL を渡す
- 対象が URL で決まるので、`gh-wait-review.sh` の「cwd に GitHub のリモートが無ければ `SKIP`」の判定は外す。残すと、
  正しい URL を渡しても cwd 次第で待たずに終わる。GitHub のリモートの無いリポジトリでは PR の URL が無いので、
  呼ぶ場面が無い
- スクリプトの間の呼び出し（`decide-next.sh` から `get-latest-review.sh` など）も URL を渡す。番号だけを渡すと、
  受けた側が `GH_REPO` の継承に頼ることになり、単独で呼んだときと挙動が変わる
- URL の解析は `.claude/scripts/lib/pr-url.sh` の 1 か所に置く。10 本のスクリプトが同じ解析を持つため
- `/files` や通知のクエリの付いた URL も同じ PR を指すので受ける。`#` の付いた URL は、コメントを取り出す
  `get-pr-info.sh` と、PR だけを使う `gh-actions-diagnose.sh` が受ける。コメントを別の引数で受けるスクリプトで受けると、
  その comment_id と食い違っても気付けない。owner と repo は GitHub の名前に使える文字だけを受ける（案内のコマンドに書くため）
- `gh-actions-diagnose.sh` も PR の URL を受け、NEXT に書く gh の手順に `-R` を付ける。URL で動くスクリプトや
  hook が診断を番号で案内すると、案内の先で cwd の取り違えが戻るため。番号とブランチ名も受け続ける
  （ユーザーが `/gh-actions-check` を手で実行する形）
- push などの worktree を触る処理はスクリプトに無い（push はスキルの手順で、`cd <worktree> &&` を付けた git で行う）。
  issue の「URL の owner/repo と cwd の origin の一致を確かめる」は当てはまる処理が無いので作らない

採らなかった案:

- 案 B: 渡し方が 2 通りになり、解析とスキルの手順の分岐が増える。URL で足りない場面が無い
- 案 C: 別の GitHub のリポジトリで実行する取り違えを止められない。`cd "$WT"` のように cd の先が変数だと解けず、
  正しいコマンドも止める。スクリプトの名前だけで照らすので、プロジェクトの同名のスクリプトも止める。`GH_REPO` を
  付けた正しい呼び出しも止める
- 案 D: 注意を書いた後も起きた（背景）

### 帰結

- 良い点: cwd がどこでも、渡した URL の PR だけを読み書きする。`cd <worktree> &&` が要らなくなる
- 悪い点: 呼び出しが長くなる。PR の URL を最初に得る手間が要る（`gh pr create` の出力、または
  `cd <worktree> && gh pr view --json url -q .url`）
- `gh-wait-review.sh` の終了コード 2（`SKIP`）は無くなる。CLAUDE.md の「出力を `tail` につながない」の理由を書き換える
- `gh-actions-diagnose.sh` に番号かブランチ名を渡したときは、今も cwd のリポジトリで読む

### 確認

- `.claude/tests/scripts/gh-pr-review.sh` で、各スクリプトが URL の無い呼び出しを使い方の表示で拒むこと、`gh` が
  URL のリポジトリを `GH_REPO` で受けること、GitHub のリモートの無い cwd でも `gh-wait-review.sh` が待つこと
