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
- Linux の配備（Nix）は今のまま使う。git hook は Claude Code の外（利用者の端末の git）でも走るので、Linux では、hook が
  呼ぶハーネスの検査（`ja-terms`、`gitleaks`、`commit-refs`）を利用者の PATH に頼らない
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
- プロジェクトのローカルの hook（`.git/hooks/<名前>`）: 今の sh は実行の権限（`[ -x ]`）で見つけて直に起動する。Windows には
  実行の権限が無く、スクリプトを直に起動できない。`core.hooksPath` を向けると git はローカルの hook を探さないので、
  Windows でも中身が `.git/hooks/<名前>` を自分で探し、ファイルがあれば hook とみなして、Git for Windows の sh に exec
  させる（sh の exec は shebang の行を読むので、sh 以外のスクリプトも動く）。Linux は今のまま
- 外のコマンド: `ja-terms` は今の `CLAUDE_HOOKS_JA_TERMS` で見つける（無ければ Windows だけ PATH。ADR-0085）。`gitleaks`
  と `commit-refs` も同じ形にする（環境変数は Linux の `package.nix` が store のパスで渡し、無ければ Windows だけ PATH）。Windows の手順
  （README の「Windows で使う」）は、`commit-refs` も `cargo install` で入れる。今の `commit-refs` は
  `std::os::unix` を使い Windows でビルドできないので、post-rewrite を移す PR で Windows に対応させ、`windows.yml` の
  matrix に足す。見つからないときの扱いは今と同じ
  （gitleaks が無ければ止め、post-rewrite の付け直しは失敗を示して対応を残す）。書式と lint のツール（nixfmt、ruff、
  markdownlint、rustfmt と、edition を読む cargo）と `prek` は、今のとおり利用者の PATH のものを使い、無ければ飛ばす。
  段階 6 まで sh と Python のままのもの（pre-merge-commit が呼ぶ `run-checks.sh`、Markdown の自動修正の
  `fix-markdown-lint.py`）は、今のとおり `~/.claude` の下のものを `bash` と `python3` で呼び、`python3` が無ければ
  自動修正を飛ばす
- `staged-checks` のコマンド: `run-checks.sh` の Markdown の段が PATH の `staged-checks` を呼ぶ。Linux は、
  `claude-hooks git-hook staged-checks` を呼ぶだけの `staged-checks` を、段階 6 まで PATH に残す
- テスト: `modules/home/parts/tests/` の git hook の sh を、次の 2 つに置き換える
  - 判定: `claude-hooks` の `tests/` の結合テスト。一時のリポジトリで `git commit` などを実行し、hook のファイルから
    中身を呼ぶ。外のツール（gitleaks、書式と lint のツール、`~/.claude` の `run-checks.sh`）は、テストが偽のものを
    渡す（crate のビルドの sandbox には本物が無い）。渡し方は本物の見つけ方に合わせ、`ja-terms`、`gitleaks`、`commit-refs`
    は環境変数で、ほかは一時の HOME と PATH に置く。Linux（`nix flake check`）と Windows の CI
    （ADR-0083）で流す
  - 配備: Linux の hook のファイルと、`package.nix` が包む store のパス（`ja-terms`、`gitleaks`、`commit-refs`）は、
    `nix build` した hook のファイルで本物の検査を 1 回ずつ通すテストで確かめる（今の sh のテストの組み立てを残す）。
    Windows は CI の `setup` のジョブ
  - `main-commit-gates.sh` は、2 つの層が同じ関数に渡す入力（PreToolUse hook はコマンドを読んで実行の前に、git hook は
    コミットの中で、ブランチとステージしたファイルを得る）を同じ場合で比べる結合テストに置き換える
- 移す順: commit-msg、post-rewrite、pre-commit（既定の検査と機密情報の検査を含む）、pre-merge-commit。hook ごとに 1 つ以上の
  PR にし、sh の実装と sh のテストは、Rust の版に付け替えた PR で消す。Windows の設置（`claude-setup` が hook のファイルを
  置く）は、4 つが揃い、clone の改行の扱い（ADR-0084 の帰結。#104）を同期する sh のスクリプトまで決めた後の PR にする。揃う
  前に `core.hooksPath` を向けると、移していない hook が走らなくなる。CRLF の clone では、pre-merge-commit が `bash` で呼ぶ
  `run-checks.sh` が動かず、main へのマージがすべて止まる。途中の PR でも、pre-commit は今のとおり同じ置き場の
  `pre-merge-commit` を呼ぶ（マージを締めるコミット）

理由:

- 案 1 は、判定がすでに `claude-hooks` にあり、関数を呼ぶだけで揃う。バイナリが 1 つなので、Windows で入れるものも
  増えない。案 2 は、ライブラリの crate を切り出す作業と、2 つのバイナリの版を揃える費用がかかり、得るのはバイナリの
  名前の分け方だけ。crate の分け方（#101 の「決まっていないこと」）は、段階 5 で 3 つ目のまとまりを作るときに決める
- 案 3 は、同じ判定を sh と Rust に書き分けたまま残す（決定の要因の 1 つめ）。jq は Windows の手順がすでに入れる
  （README の「Windows で使う」。段階 5 までスキルの sh が使う）が、段階 8 で sh を移し終えたら外す依存で、git hook の
  ために残すことになる
- 案 A は、hook のファイルの形を OS で分けない。案 B は、git が hook を名前で探すので、同じバイナリを 4 つの名前で置く
  ことになる。Windows の symlink は権限（か開発者モード）が要り、hard link は `cargo install` がファイルを置き換えると
  切れるので、コピーになり、更新のたびに 4 つを入れ直す。案 A の sh を 1 つ挟む費用は
  コミットのたびに 1 回で、Bash ツールの呼び出しごとに走る PreToolUse hook（ADR-0084 の案 X）ほど効かない

### 帰結

- 良い点: main への直接のコミットの例外、機密情報の検査、fork の判定が 1 か所になる
- 悪い点: git hook の判定を変えるたびに、`claude-hooks` のビルドとテストが要る（sh なら hook のファイルを書き換えて
  その場で試せた。ADR-0082 の帰結と同じ）
- 悪い点: `claude-hooks` の `package.nix` が包むものが、`ja-terms` に加えて `gitleaks` と `commit-refs` に増える
- Windows では、4 つの hook が揃って設置されるまで、git hook の検査（コミットとマージのメッセージ、ローカルの main への
  マージの品質のチェック、競合の無いマージの機密情報の検査）が走らない（README の「Windows で使う」）。PATH の
  `staged-checks` も Linux にしか置かないので、段階 6 までは Windows の `run-checks.sh` の Markdown の段が飛ぶ

### 確認

- 4 つの hook の結合テストが、Linux と Windows の CI で通る。テストの期待値は、今の sh のテストが確かめているものと
  同じ（止めるもの、通すもの、文言の要点）
- 移した後、`git-hooks.nix` の hook のファイルは `claude-hooks git-hook` を呼ぶ 1 行だけになり、`staged-checks.sh`、
  `scan-staged-secrets.sh`、`main-commit-gates.sh` は無い
- Windows の CI の `setup` のジョブで、設置した hook が main への直接のコミットと、件名の形の誤ったコミットを止める

## 補足

- 2026-11-13: 機密情報の検査（PreToolUse の block-secret-commit と git hook の scan-secrets が共有する
  `claude-hooks` の `src/gitleaks.rs`）で、gitleaks の標準エラーの扱いを 2 つ試して捨てた（PR #131）。gitleaks
  8.30.1 は、中の git diff が標準エラーに何か出すと、警告（`* text=auto` の下の CRLF のファイルの「CRLF will be
  replaced by LF」）でも `ERR [git] warning: …` と `stderr is not empty` を出し、差分を読むのをやめて `[]` と
  終了コード 0 を返す。scratchpad の一時のリポジトリで、同じ `ghp_` のトークンが、LF の差分では `github-pat` で
  見つかり、CRLF の警告の出る差分では見つからなかった。
  - `--pre-commit`（`git commit -a`）の標準エラーを見ない: 警告で誤って止めないために試したが、警告の出る差分の
    秘密を通す
  - `[git] warning:` と `stderr is not empty` の行を除く: 同じく、警告で読むのをやめた差分の秘密を通す
  - 採った形: 色を外した標準エラーの行が 1 つでもあれば、どちらのモードでも検査できなかったとして止め、警告の原因を
    直せば検査できると書く
