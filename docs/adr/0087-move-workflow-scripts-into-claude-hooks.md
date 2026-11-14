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
  を起動するだけの sh にする
- 案 B: スキルの文と hook のルールを、新しいコマンドの名前に書き換え、スクリプトのファイルを消す

## 決定と理由

採用: 案 1、案 A。#101 の「作業の順序」の、sh を呼び出し元の付け替えで消す、は段階 5 のスクリプトには当てはめない。

- 中身: `claude-hooks` に `script` のサブコマンドを足し、スクリプトごとの処理を移す。GitHub の API（`gh` の子プロセス）と
  ローカルレビューの記録の扱いは、今の `claude-hooks` の関数を使う
- スクリプトのファイル: 名前と場所を変えず、`lib/claude-hooks-script.sh` を読んで `claude_hooks_script <名前> "$@"` を
  呼ぶだけの sh にする。`claude_hooks_script` は `"${CLAUDE_HOOKS_BIN:-$HOME/.claude/bin/claude-hooks}" script <名前> "$@"`
  を exec し（`lib/local-review.sh` と同じ引き方。PATH で引かない）、バイナリが無ければ入れ方を示して 127 で終える。
  スキルの文と hook のルールは変えない。Windows では Git Bash がこの sh を実行し、jq を使わない
- Windows の置き場: crate を `cargo install --root ~/.claude` で `~/.claude/bin/` に入れ、Linux と同じパスで引く
  （`claude_hooks_script` は、拡張子の無いパスが無く `.exe` だけがあれば `.exe` のパスを起動する。`.exe` を補って起動
  しない sh でも動くように）。settings.json と git hook の登録もこのパスを指し、`ja-terms` と `commit-refs` は
  `claude-hooks` の隣のものを起動する。スキルと修正方法の文が名前で呼ぶ `claude-setup` と `commit-refs` のため、
  `~/.claude/bin` を Git Bash の `~/.bashrc` で PATH の先頭に足す（README の「Windows で使う」）
- `lib/` の部品: source されるので、呼ぶだけの sh にしない。処理は `claude-hooks` の共通の関数に移し、source するスクリプトが
  すべて移った PR で消す（`lib/claude-hooks-script.sh` は移したスクリプトが読むので残す）
- `check-each-commit.sh` は移さず、sh のまま残す（#169）。jq を使わず、Windows では呼ぶだけの sh と同じく Git Bash で動く。
  中断されたとき（Ctrl-C、TERM、HUP）に一時の worktree を消す `trap` は、Rust の標準ライブラリでは書けず、移すには
  シグナルを受ける crate（`ctrlc`、または `libc` と `windows-sys`）が要る
- PR の単位: スクリプトごとに 1 つ以上の PR にし、中身を移した PR で sh の実装を、上の呼ぶだけの sh に置き換える
- テスト: 判定は `claude-hooks` の結合テスト（gh は偽のコマンドで置き換える。仕組みは最初に移す PR でテストとともに
  決める）で、`nix flake check` と Windows の CI（`windows.yml`）で流す。今の `.claude/tests/scripts.sh` は、
  呼ぶだけのスクリプトのファイルから呼ぶ配備の確認として残し、`CLAUDE_HOOKS_BIN` にブランチでビルドしたバイナリを渡して流す

理由:

- `check-each-commit.sh` を移して得るのは、jq を外すことでも Windows で動くようにすることでもない（どちらも今の sh で
  満たす）。依存を足してまで移す理由が無い。中断の後片付けを、次の起動で前の一時の worktree を消す形に変える案は、
  同時に走る別の起動の worktree を消さないための名前の決め方が要り、今の `trap` より複雑になる
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
- 悪い点: `claude-sync` は呼ぶだけのスクリプトをすぐ配るが、バイナリは rebuild まで前の版のままで、その間、移した
  スクリプトは前の版のバイナリにサブコマンドが無く失敗する。移す PR は `claude-hooks` を変えるので、マージの後は
  `claude-sync` でなく rebuild（activation が sync も実行する）で入れる。Windows も、`claude-setup sync` の前に
  `cargo install` を済ませる
- 悪い点: Windows では、`cargo install --root ~/.claude` が入れた crate の記録（`.crates.toml`、`.crates2.json`）を
  `~/.claude` に置く

### 確認

- 移したスクリプトの結合テストが、`nix flake check` と Windows の CI で通る。期待値は、今の `.claude/tests/scripts.sh` が確かめて
  いるもの
- 移した後、対象のスクリプトのファイル（`check-each-commit.sh` を除く）は `lib/claude-hooks-script.sh` を読んで `claude_hooks_script <名前>` を呼ぶだけに
  なり、jq を使わない。`lib/` の部品は `claude-hooks-script.sh` のほかは消えている

## 補足

- 2026-11-13: Windows の置き場は、はじめ `claude-setup` が `cargo install` の置き場（`~/.cargo/bin`）の `claude-hooks.exe` を
  `~/.claude/bin/` にコピーするとしていた。settings.json と git hook の登録は `~/.cargo/bin` を指したままなので、コピーを
  忘れると hook とスクリプトで版が分かれる（PR #142 のローカルレビュー）。`cargo install --root ~/.claude` で入れれば
  置き場が 1 つになり、コピーの手順も要らないので、そちらに変えた。`~/.claude/bin` は PATH に無いので、名前で呼ぶ
  `claude-setup` と `commit-refs` のために PATH の先頭に足し、前の `~/.cargo/bin` の版は外す。PATH は、はじめ PowerShell で
  User の Path に足したが、User の Path の展開する項目（`%USERPROFILE%\...`）を固定のパスに潰し、System の Path の後に
  並ぶので先頭にならず、`$HOME` と `USERPROFILE` が違えば置き場とずれる（PR のローカルレビュー）ので、Git Bash の
  `~/.bashrc` に変えた
- 2026-11-14: スクリプトのファイルは、はじめ `exec "${CLAUDE_HOOKS_BIN:-$HOME/.claude/bin/claude-hooks}" script <名前> "$@"`
  だけの sh としていた。バイナリが無いと sh の `exec: not found` だけで止まり、何を入れるかが分からない（claude-sync は
  スクリプトだけを入れる）。decide-next.sh は `local_review_covers` で入れ方を示していたが、呼ぶスクリプト
  （get-review-comments.sh、get-latest-review.sh）を移すと、その前で案内なしに止まった（`.claude/tests/scripts/gh-pr-review.sh`
  の「claude-hooks が無ければ、入れ直しを案内して止める」が落ちた）。decide-next.sh の確認の順を入れ替えても、次に移す
  スクリプトがまたその前に来るので、入れ方を示すのを sh の共通の部品（`lib/claude-hooks-script.sh`）に移した
- 2026-11-14: `claude_hooks_script` は、はじめ `.exe` の付いたパスも在るものとして数え、起動は拡張子の無いパスのまま exec
  していた。`.exe` だけがあり、`.exe` を補って起動しない sh では、在ると数えた後に `not found` で止まる（PR #148 の Copilot
  のレビュー。その版のテストは入れ方の文が出ないことしか見ず、起動の失敗でも通っていた）ので、`.exe` だけがあれば
  そのパスを起動するように変え、テストは起動して 0 で終えることを確かめる
- 2026-11-14: 案内に書くスクリプトのパス（gh-wait-review の「次の一手」の request-rereview.sh など）は、sh の版では
  自分の場所から辿り、デプロイ先の古い版を案内しないようにしていた。Rust に移すと自分の場所が分からないので、
  `lib/claude-hooks-script.sh` が呼んだ sh の場所を `CLAUDE_HOOKS_SCRIPT_DIR` で渡し、`script::claude_root` がそこから
  `<root>`（`~/.claude` か、リポジトリの `.claude`）を引く（渡されなければ `~/.claude`）。Windows の Git Bash の `pwd` は
  `/c/...` の形でネイティブの exe が読めないので、`pwd -W` の形で渡す
- 2026-11-14: worktree-add の .worktreelink の symlink は、sh の版では `ln -s` で作っていた。Git Bash の `ln -s` は既定で
  コピーを作るので、Windows では共有にならなかった。Rust に移した版は、Windows では `std::os::windows::fs` の
  symlink_dir / symlink_file で作る。開発者モードか管理者の権限が無ければ作れないので、作った symlink を消して、その旨を
  示して止める
