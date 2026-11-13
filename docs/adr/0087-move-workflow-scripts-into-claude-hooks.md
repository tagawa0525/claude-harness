---
status: proposed
date: 2026-11-13
requires: [ADR-0082, ADR-0084, ADR-0086]
supersedes: []
superseded-by:
issues: ["#101"]
---

# ADR-0087: Git と GitHub の作業手順のスクリプトの中身を claude-hooks のサブコマンドに移し、スクリプトのファイルはそれを呼ぶだけにする

## 背景

Git と GitHub の作業手順のスクリプトを、#101 の段階 5 で Rust に移す（ADR-0082）。対象は `.claude/scripts/` の
`worktree-add.sh`、`post-merge-cleanup.sh`、`gh-wait-review.sh`、`review-level.sh`、`git-merge-state.sh`、`base-moved.sh`、
`branch-topics.sh`、`rename-*.sh`、`check-each-commit.sh`、`git-info.sh`、`gh-actions-diagnose.sh` と `lib/` の共通の部品、
`skills/gh-pr-review/scripts/*.sh` で、Slurm のものを除いて約 2,800 行（2026-11-13、`wc -l`）。どれも jq を使い、Windows の
手順は jq を入れている（README の「Windows で使う」）。

これらのスクリプトは、スキルの文から `~/.claude/scripts/<名前>.sh` のパスで呼ばれる（`review-level.sh` は 10 か所、
`worktree-add.sh` は 9 か所など）。claude-hooks の PreToolUse のルールも、スクリプトの名前で呼び出しを見分ける（例:
`gh-wait-review.sh` を裏で待たせる、`record-local-review.sh` と `request-rereview.sh` を通す）。`decide-next.sh` と
`lib/local-review.sh` は、ローカルレビューの記録の判定を `claude-hooks local-review-covers` に任せている。

crate の分け方は、#101 の「決まっていないこと」で、段階 5 で 3 つ目のまとまりを作るときに決めるとしていた
（ADR-0086 は git hook を `claude-hooks` のサブコマンドにした）。

決めること: Rust の中身の置き場、スクリプトのファイルと、スキルの文と hook のルールからの呼ばれ方、移す順とテスト。

## 決定の要因

- スキルの文と PreToolUse のルールは、スクリプトの名前で呼び、見分ける。名前を変えると、両方を同じ PR で揃える
  費用がかかり、揃え漏れは hook の判定の抜けになる
- GitHub の API とローカルレビューの記録の扱いは、すでに `claude-hooks` にある（`gh.rs`、`pre_merge_check.rs`）
- Windows で入れるものを増やさない（ADR-0084 の手順は crate ごとに `cargo install` する）

## 検討した案

置き場:

- 案 1: `claude-hooks` のサブコマンド（`claude-hooks script <名前> [引数]`）にする
- 案 2: 新しい crate（作業手順の CLI）にし、`claude-hooks` と共有する部品はライブラリの crate に出す
- 案 3: 新しい crate にし、共有する部品はコピーして持つ

呼ばれ方:

- 案 A: スクリプトのファイル（`~/.claude/scripts/<名前>.sh`）の名前と場所を変えず、中身を `claude-hooks script <名前> "$@"`
  を exec するだけの 1 行にする
- 案 B: スキルの文と hook のルールを、新しいコマンドの名前に書き換え、スクリプトのファイルを消す

## 決定と理由

採用: 案 1、案 A。

- 中身: `claude-hooks` に `script` のサブコマンドを足し、スクリプトごとの処理を移す。GitHub の API（`gh` の子プロセス）と
  ローカルレビューの記録の扱いは、今の `claude-hooks` の関数を使う
- スクリプトのファイル: 名前と場所を変えず、`exec claude-hooks script <名前> "$@"` だけの sh にする。スキルの文と hook の
  ルールは変えない。Windows では Git Bash がこの sh を実行し、jq を使わない
- `claude-hooks` を PATH に置く: Linux は `claude-code.nix` が包んだ `claude-hooks` を `home.packages` に足す（今は
  `~/.claude/bin/claude-hooks` の固定のパスだけ）。Windows は `cargo install` の置き場が PATH にある
- 移す順: 共通の部品（`lib/pr-url.sh`、`lib/local-review.sh`）を使うものから、gh-pr-review のスクリプト、
  `gh-wait-review.sh`、`review-level.sh`、`git-merge-state.sh`、`base-moved.sh`、`branch-topics.sh`、`worktree-add.sh`、
  `post-merge-cleanup.sh`、残り。スクリプトごとに 1 つ以上の PR にし、中身を移した PR で sh の実装を 1 行に置き換える
- テスト: 判定は `claude-hooks` の結合テスト（gh は偽のコマンドで置き換える）で、Linux と Windows の CI で流す。今の
  `.claude/tests/scripts.sh` は、1 行のスクリプトのファイルから呼ぶ配備の確認として残す

理由:

- 案 1 は、GitHub の API とローカルレビューの記録の扱いをそのまま使え、Windows で入れるものも増えない。案 2 は、
  ライブラリの crate を切り出す作業がかかり、得るのはバイナリの名前の分け方だけ（ADR-0086 の案 2 と同じ）。案 3 は、
  記録の判定が 2 つの crate に分かれ、`local-review-covers` で 1 つにした判定（`decide-next.sh` と pre-merge-check）が
  また食い違いうる
- 案 A は、スキルの文と hook のルールを変えないので、移す PR ごとの変更がスクリプト 1 つに収まる。案 B は、移すたびに
  スキルの文と hook のルールを揃える PR になり、揃え漏れが hook の判定の抜けになる。sh を 1 つ挟む費用は呼び出しごとに
  1 回で、これらのスクリプトは作業の節目に呼ぶもの（PreToolUse hook のように毎回は走らない）

### 帰結

- 良い点: 移した後のスクリプトは jq を使わない。Windows の手順から jq を外せるのは、段階 8 まで移し終えた後
- 悪い点: `claude-hooks` という名前のバイナリが、hook でない作業手順の処理も持つ。名前は段階 8 の後に見直す
- 悪い点: スクリプトの判定を変えるたびに、`claude-hooks` のビルドとテストが要る（ADR-0082 の帰結と同じ）

### 確認

- 移したスクリプトの結合テストが、Linux と Windows の CI で通る。期待値は、今の `.claude/tests/scripts.sh` が確かめて
  いるもの
- 移した後、対象のスクリプトのファイルは `claude-hooks script <名前>` を呼ぶ 1 行だけになり、jq を使わない
