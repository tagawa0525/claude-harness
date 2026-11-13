---
status: proposed
date: 2026-11-13
requires: [ADR-0015, ADR-0028, ADR-0082, ADR-0083, ADR-0084, ADR-0085]
supersedes: []
superseded-by:
issues: ["#101"]
---

# ADR-0086: git hook の中身を claude-hooks のサブコマンドに移し、hook のファイルはそれを呼ぶだけにする

## 背景

グローバルの git hook（pre-commit、pre-merge-commit、commit-msg、post-rewrite）は、`modules/home/parts/git-hooks.nix`
（411 行）の中の bash と、hook が store のパスで呼ぶ `staged-checks.sh`（219 行）、`scan-staged-secrets.sh`（60 行）で
できている（2026-11-13、`875f7206`）。#101 の段階 4 で、これを Rust に移して、ネイティブの Windows でも動かす
（ADR-0082）。ADR-0084 は、Windows の git hook の設置を「hook のファイルを、Rust のバイナリを呼ぶだけの sh にして置き、
`core.hooksPath` を向ける」と決め、中身の Rust と設置は段階 4 に回した。

同じ判定が、PreToolUse hook（`claude-hooks`）と git hook に書き分けてある:

- main への直接のコミットの例外。`block_main_commit.rs` と pre-commit の bash にあり、
  `modules/home/parts/tests/main-commit-gates.sh` が一致を確かめる
- 機密情報の検査。`block_secret_commit.rs` と `scan-staged-secrets.sh` にあり、どちらも gitleaks を呼ぶ
- fork（upstream のリモート）の判定と、崩れた語の検査（`ja-terms`）の呼び出し

git hook の今のテストは `modules/home/parts/tests/` の sh で、12 本が hook のファイルを `nix build` で作って実行する。
Nix の無い Windows では流せない。

決めること: Rust の中身をどの crate に置くか、hook のファイルの形、hook が呼ぶ外のコマンドの見つけ方、テストの置き場、
移す順。

## 決定の要因

- PreToolUse hook と git hook の同じ判定を 1 つの関数にする（ADR-0082 の理由。照合のテストが要らなくなる）
- Linux と Windows で、hook のファイルと中身を同じにする
- Linux の配備（Nix）は今のまま使う。git hook は Claude Code の外（利用者の端末の git）でも走るが、hook が呼ぶ検査は
  利用者の PATH に頼らない
- 移す途中も、hook が止めるものと通すものを変えない

## 検討した案

置き場:

- 案 1: `claude-hooks` のサブコマンド（`claude-hooks git-hook <名前> [引数]`）にする
- 案 2: 新しい crate（`git-hooks`）にし、`claude-hooks` と共有する判定はライブラリの crate に出す
- 案 3: sh のまま、`jq` と coreutils の違いだけを直して Git Bash で動かす

hook のファイル:

- 案 A: 中身を呼ぶだけの sh（`exec <claude-hooks> git-hook pre-commit "$@"`）にする。Linux は `git-hooks.nix` が、Windows は
  `claude-setup` が置く
- 案 B: hook のファイルを Rust のバイナリそのものにする（hook の名前で呼ばれたら、その hook として振る舞う）

## 決定と理由

採用: 案 1、案 A。

- 中身: `claude-hooks` に `git-hook` のサブコマンドを足し、4 つの hook を Rust に移す。pre-commit が呼ぶ既定の検査
  （今の `staged-checks.sh`）と機密情報の検査（今の `scan-staged-secrets.sh`）も移す。PreToolUse hook と同じ判定は、
  同じ関数を両方から呼ぶ。git hook は対象のリポジトリの中で走るので、判定の入力（ブランチ、index、MERGE_HEAD）は
  git から直に読む
- hook のファイル: どちらの OS も、`claude-hooks` の絶対パスに `git-hook <名前> "$@"` を付けて exec するだけの sh にする。
  Linux は `git-hooks.nix` が store のパスで書く。Windows は `claude-setup` が `cargo install` の置き場のパスで書き、
  グローバルの `core.hooksPath` を向ける（ADR-0084）。Git for Windows は、hook を同梱の sh で実行する
- 外のコマンド: `ja-terms` は今の `CLAUDE_HOOKS_JA_TERMS` で見つける（無ければ Windows は PATH。ADR-0085）。`gitleaks`
  と `commit-refs` も同じ形の環境変数（Linux の `package.nix` が store のパスを渡す）と PATH で見つける。書式と lint の
  ツール（nixfmt、ruff、markdownlint、rustfmt）と `prek` は、今のとおり利用者の PATH のものを使い、無ければ飛ばす。
  pre-merge-commit が呼ぶ `run-checks.sh` は段階 6 まで sh のままにし、`bash` で呼ぶ
- `staged-checks` のコマンド: `run-checks.sh` の Markdown の段が PATH の `staged-checks` を呼ぶ。Linux は、
  `claude-hooks git-hook staged-checks` を呼ぶだけの `staged-checks` を、段階 6 まで PATH に残す
- テスト: `modules/home/parts/tests/` の git hook の sh を、`claude-hooks` の `tests/` の結合テストに置き換える。結合テストは、
  一時のリポジトリで `git commit` などを実行し、hook のファイルから中身を呼ぶ。Linux（`nix flake check`）と Windows の CI
  （ADR-0083）で流す。`main-commit-gates.sh` は、両方が同じ関数を呼ぶので消す
- 移す順: commit-msg、post-rewrite、pre-commit（既定の検査と機密情報の検査を含む）、pre-merge-commit。hook ごとに 1 つ以上の
  PR にし、sh の実装と sh のテストは、Rust の版に付け替えた PR で消す。Windows の設置（`claude-setup` が hook のファイルを
  置く）は、4 つが揃った後の PR にする。揃う前に `core.hooksPath` を向けると、移していない hook が走らなくなる

理由:

- 案 1 は、判定がすでに `claude-hooks` にあり、関数を呼ぶだけで揃う。バイナリが 1 つなので、Windows で入れるものも
  増えない。案 2 は、ライブラリの crate を切り出す作業と、2 つのバイナリの版を揃える費用がかかり、得るのはバイナリの
  名前の分け方だけ。crate の分け方（#101 の「決まっていないこと」）は、段階 5 で 3 つ目のまとまりを作るときに決める
- 案 3 は、ADR-0082 が採らなかった案 A と同じ（Windows に jq を入れる手順が要り、sh の移植性を確かめる手段が無い）
- 案 A は、hook のファイルの形を OS で分けない。案 B は、git が hook を名前で探すので、同じバイナリを 4 つの名前で置く
  ことになる。Windows の symlink は権限が要るのでコピーになり、更新のたびに 4 つを入れ直す。案 A の sh を 1 つ挟む費用は
  コミットのたびに 1 回で、Bash ツールの呼び出しごとに走る PreToolUse hook（ADR-0084 の案 X）ほど効かない

### 帰結

- 良い点: main への直接のコミットの例外、機密情報の検査、fork の判定が 1 か所になる
- 悪い点: git hook の判定を変えるたびに、`claude-hooks` のビルドとテストが要る（sh なら hook のファイルを書き換えて
  その場で試せた。ADR-0082 の帰結と同じ）
- 悪い点: `claude-hooks` の `package.nix` が包むものが、`ja-terms` に加えて `gitleaks` と `commit-refs` に増える
- Windows では、4 つの hook が揃って設置されるまで、コミットとマージのメッセージの検査が走らない（README の
  「Windows で使う」）

### 確認

- 4 つの hook の結合テストが、Linux と Windows の CI で通る。テストの期待値は、今の sh のテストが確かめているものと
  同じ（止めるもの、通すもの、文言の要点）
- 移した後、`git-hooks.nix` の hook のファイルは `claude-hooks git-hook` を呼ぶ 1 行だけになり、`staged-checks.sh`、
  `scan-staged-secrets.sh`、`main-commit-gates.sh` は無い
- Windows の CI の `setup` のジョブで、設置した hook が main への直接のコミットと、件名の形の誤ったコミットを止める
