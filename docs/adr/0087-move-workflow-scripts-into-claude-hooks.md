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
`worktree-add.sh`、`post-merge-cleanup.sh`、`gh-wait-review.sh`、`gh-review-requests.sh`、`review-level.sh`、
`git-merge-state.sh`、`base-moved.sh`、`branch-topics.sh`、`rename-*.sh`、`check-each-commit.sh`、`git-info.sh`、
`gh-actions-diagnose.sh` と `lib/` の共通の部品、`skills/gh-pr-review/scripts/*.sh` で、Slurm のものを除いて 24 本、2,838 行
（`f0083452`、2026-11-13、`wc -l`）。うち 12 本が jq を使い（`grep -l jq`）、Windows の手順は jq を入れている（README の
「Windows で使う」）。`lib/pr-url.sh` と `lib/local-review.sh` は、ほかのスクリプトが source する部品で、単独では実行しない。

これらのスクリプトは、スキルの文から `~/.claude/scripts/<名前>.sh` か `~/.claude/skills/gh-pr-review/scripts/<名前>.sh` のパスで
呼ばれる（`f0083452` で、`review-level.sh` を名前で書く
スキルの文は `.claude/skills` の下の 6 ファイル。`git grep -l`）。claude-hooks の PreToolUse のルールも、スクリプトの名前で呼び出しを
見分ける（例: `require_background_wait.rs` は `gh-wait-review.sh` と `request-rereview.sh` をフォアグラウンドで実行させない）。
拒否の文言も、修正方法としてスクリプトの名前を示す（`pre_merge_check.rs`、`guard_gh_api.rs` など）。`lib/local-review.sh`
（`decide-next.sh`、`request-rereview.sh`、`record-local-review.sh` が source し、前の 2 本が判定の関数を呼ぶ）は、ローカルレビューの記録の判定を
`claude-hooks local-review-covers` に任せ、バイナリを `${CLAUDE_HOOKS_BIN:-$HOME/.claude/bin/claude-hooks}` で引く（PATH に
頼らない。`claude-code.nix` の注記）。Windows では `~/.claude/bin/claude-hooks` が無く、前の 2 本は今も動かない。

crate の分け方は、#101 の「決まっていないこと」で、段階 5 で 3 つ目のまとまりを作るときに決めるとしていた
（ADR-0086 は git hook を `claude-hooks` のサブコマンドにした）。#101 の「作業の順序」は、sh の実装を、呼び出し元を
付け替えた PR で消すとしていた。

決めること: Rust の中身の置き場、スクリプトのファイルと、スキルの文と hook のルールからの呼ばれ方、テスト。移す順は #101 の
「作業の順序」に書く。

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

- 案 A: スクリプトのファイル（`~/.claude/scripts/` と `~/.claude/skills/gh-pr-review/scripts/` の `<名前>.sh`）の名前と場所を変えず、中身を `claude-hooks script <名前> "$@"`
  を exec するだけの 1 行にする
- 案 B: スキルの文と hook のルールを、新しいコマンドの名前に書き換え、スクリプトのファイルを消す

## 決定と理由

採用: 案 1、案 A。#101 の「作業の順序」の、sh を呼び出し元の付け替えで消す、は段階 5 のスクリプトには当てはめない。

- 中身: `claude-hooks` に `script` のサブコマンドを足し、スクリプトごとの処理を移す。GitHub の API（`gh` の子プロセス）と
  ローカルレビューの記録の扱いは、今の `claude-hooks` の関数を使う
- スクリプトのファイル: 名前と場所を変えず、`exec "${CLAUDE_HOOKS_BIN:-$HOME/.claude/bin/claude-hooks}" script <名前> "$@"`
  だけの sh にする（`lib/local-review.sh` と同じ引き方。PATH に `claude-hooks` を置かない）。スキルの文と hook のルールは
  変えない。Windows では Git Bash がこの sh を実行し、jq を使わない
- Windows の置き場: crate を `cargo install --root ~/.claude` で `~/.claude/bin/` に入れ、Linux と同じパスで引く（Git Bash は
  `.exe` を補って実行する）。settings.json と git hook の登録もこのパスを指し、`ja-terms` と `commit-refs` は
  `claude-hooks` の隣のものを起動する（README の「Windows で使う」）
- `lib/` の部品: source されるので 1 行にしない。処理は `claude-hooks` の共通の関数に移し、source するスクリプトが
  すべて移った PR で消す
- PR の単位: スクリプトごとに 1 つ以上の PR にし、中身を移した PR で sh の実装を 1 行に置き換える
- テスト: 判定は `claude-hooks` の結合テスト（gh は偽のコマンドで置き換える。仕組みは最初に移す PR でテストとともに
  決める）で、`nix flake check` と Windows の CI（`windows.yml`）で流す。今の `.claude/tests/scripts.sh` は、
  1 行のスクリプトのファイルから呼ぶ配備の確認として残し、`CLAUDE_HOOKS_BIN` にブランチでビルドしたバイナリを渡して流す

理由:

- 案 1 は、GitHub の API とローカルレビューの記録の扱いをそのまま使え、Windows で入れるものも増えない。案 2 は、
  ライブラリの crate を切り出す作業がかかり、得るのはバイナリの名前の分け方だけ（ADR-0086 の案 2 と同じ）。案 3 は、
  記録の判定が 2 つの crate に分かれ、`local-review-covers` で 1 つにした判定（`decide-next.sh` と pre-merge-check）が
  また食い違いうる
- 案 A は、スキルの文と hook のルールを変えないので、移す PR ごとの変更がスクリプト 1 つに収まる。案 B は、移すたびに
  スキルの文と hook のルールを揃える PR になり、揃え漏れが hook の判定の抜けになる。sh を 1 つ挟む費用は呼び出しごとに
  1 回で、これらのスクリプトは作業の節目に呼ぶもの（PreToolUse hook のように毎回は走らない）
- バイナリを PATH でなく固定のパスと `CLAUDE_HOOKS_BIN` で引くのは、今の `lib/local-review.sh` と揃え、テストで
  ブランチのバイナリに差し替えられるようにするため。Windows も同じパスに置けば、スクリプトからの引き方が
  Linux と 1 つで済み、今の `lib/local-review.sh` も Windows で動く

### 帰結

- 良い点: 移した後のスクリプトは jq を使わない。Windows の手順から jq を外せるのは、段階 8 まで移し終えた後
- 悪い点: `claude-hooks` という名前のバイナリが、hook でない作業手順の処理も持つ。名前は段階 8 の後に見直す
- 悪い点: スクリプトの判定を変えるたびに、`claude-hooks` のビルドとテストが要る（ADR-0082 の帰結と同じ）
- 悪い点: `claude-sync` は 1 行のスクリプトをすぐ配るが、バイナリは rebuild まで前の版のままで、その間、移した
  スクリプトは前の版のバイナリにサブコマンドが無く失敗する。移す PR は `claude-hooks` を変えるので、マージの後は
  `claude-sync` でなく rebuild（activation が sync も実行する）で入れる。Windows も、`claude-setup sync` の前に
  `cargo install` を済ませる

### 確認

- 移したスクリプトの結合テストが、`nix flake check` と Windows の CI で通る。期待値は、今の `.claude/tests/scripts.sh` が確かめて
  いるもの
- 移した後、対象のスクリプトのファイルは `claude-hooks script <名前>` を呼ぶ 1 行だけになり、jq を使わない。`lib/` の
  部品は消えている

## 補足

- 2026-11-13: Windows の置き場は、はじめ `claude-setup` が `cargo install` の置き場（`~/.cargo/bin`）の `claude-hooks.exe` を
  `~/.claude/bin/` にコピーするとしていた。settings.json と git hook の登録は `~/.cargo/bin` を指したままなので、コピーを
  忘れると hook とスクリプトで版が分かれる（PR #142 のローカルレビュー）。`cargo install --root ~/.claude` で入れれば
  置き場が 1 つになり、コピーの手順も要らないので、そちらに変えた
