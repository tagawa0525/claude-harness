---
status: accepted
date: 2026-10-11
requires: []
supersedes: []
superseded-by:
issues: ["#281"]
---

# ADR-0015: 機密情報の検査を PreToolUse から git の pre-commit に移し、PreToolUse は --no-verify を止める

## 背景

Claude Code のセッションのコミットは、PreToolUse hook の block-secret-commit（claude-hooks）が gitleaks で検査して
いた。PreToolUse はコマンドを実行する前に、コマンドの文字列から対象のディレクトリを決めて gitleaks を起動する。

2026-10-11 に、次のコマンドが「gitleaks を起動できません: No such file or directory」で止まった（#281）。gitleaks は
PATH にある。

- 同じコマンドの中で作るディレクトリに `cd` してコミットする: 評価する時点でディレクトリがまだ無い
- パスを変数で書く（`git -C $d commit`）: `shell.rs` の `target_dir` は展開をせず原文のまま扱うので、`$d` という
  名前のディレクトリを探す

`Command::current_dir` の失敗が ENOENT になり、gitleaks の起動の失敗と区別できていなかった。

同じ理由で、コマンドを実行する前の検査には穴がある。`git add secret.txt && git commit` は、評価する時点ではまだ
`secret.txt` をステージしていないので、gitleaks は見ない。`commit -a` も、未ステージの変更を `--pre-commit` で別に
見る必要があった。

## 決定の要因

- コミットに入る中身を、実際のリポジトリで、コミットの時点で見る
- Claude Code のセッションが検査を飛ばせない
- 検査できないことと、漏えいがあることを取り違えない

## 検討した案

- 案 A: PreToolUse のまま、対象のディレクトリが無い・変数を含むときは理由を正しく出して止める
- 案 B: 検査をグローバルの git の pre-commit に移す。PreToolUse は gitleaks をやめ、hook を飛ばすコマンドを止める
- 案 C: 両方で全てのコミットを検査する
- 案 D: 案 B を主にし、グローバルの git の hook を使わないリポジトリ（ローカルの core.hooksPath を持つもの）でだけ、
  PreToolUse でも検査する

## 決定と理由

採用: 案 D。

- git の pre-commit は、コミットするリポジトリの中で、`git add` と `commit -a` を済ませた後の index を見る。対象の
  ディレクトリの解決も、同じコマンドの add の見落としも起きない（main への直接コミットのガードを git の pre-commit
  にも置いているのと同じ理由。`git.nix`）
- ただし、ローカルの core.hooksPath（husky、lefthook など）を持つリポジトリではグローバルの git の hook が走らない。
  そこだけは前の PreToolUse の検査を残す（main への直接コミットのガードと同じ 2 つの層）
- 案 A は、同じコマンドの add の見落としが残る。コマンドの文字列から実行の後の状態を推すことになる
- 案 B は、ローカルの core.hooksPath を持つリポジトリで検査が無くなる（前より後退する）
- 案 C は、PreToolUse の側が案 A と同じ誤りで正しいコミットを止め続ける
- git の hook は飛ばせるので、Claude Code のセッションでは PreToolUse で、hook の検査を飛ばすコマンドを止める。git の
  hook を飛ばす理由は、ほかの検査（main への直接コミット、形式の検査、ローカルのマージの検査）にも無い

git の hook の検査（`modules/home/parts/scan-staged-secrets.sh`）:

- Claude Code のセッション（`CLAUDECODE=1`）のコミットで、`gitleaks git --staged` を走らせる。pre-commit は、index を
  変えるもの（Markdown の自動修正は作業ツリーをステージし直す。#282。プロジェクトのローカルの pre-commit）の後、
  ステージしたファイルが無くて抜ける前（種類の変更だけのコミット）、fork で抜ける前に呼ぶ。pre-merge-commit も
  呼ぶ（競合の無いマージでは pre-commit が走らない）。マージでは相手のブランチから入る中身も見る（相手の
  ブランチの中身が検査を通ったとは限らない）
- gitleaks は Nix store のパスで呼ぶ（PATH に頼らない）
- 漏えいは場所とルール ID だけを出し、値は出さない（`--redact`）。gitleaks の失敗は、漏えいと分けて出して止める。
  gitleaks は設定を読めないときも終了コード 1 を返すので、一覧が空でないときだけ漏えいとする。中の `git diff` の
  失敗はエラーの行を出すだけで終了コード 0 を返すので、`--log-level error` でエラーの行が出たら失敗とする
- 迂回は今までと同じく `ALLOW_SECRET_COMMIT=1`（コマンドに付けた環境変数を hook が見る）。誤検出は、行の
  `gitleaks:allow`、`.gitleaksignore`、`.gitleaks.toml` で除く

PreToolUse の規則:

- block-secret-commit: 対象のリポジトリで実際に効く core.hooksPath がグローバルの値と違うとき（グローバルの hook
  が走らないとき）だけ、前と同じく `gitleaks git --staged`（`-a` なら `--pre-commit` も）で検査する。対象の
  ディレクトリを決められないとき（まだ無い、変数を含む。#281）は、git の hook に任せて通す
- block-no-verify: `git commit` / `merge` / `pull` の `--no-verify`（git が受け付ける省略形と、commit の `-n`）、hook の
  置き場所を変えるもの（`-c` / `--config-env` / `git config` の core.hooksPath、`GIT_CONFIG_*` の環境変数）、hook が
  Claude Code のセッションと分かる `CLAUDECODE=1` を別の値にする・外すもの（`env -u`、`env -i`、`unset`）を止める。
  `env`、`command`、`exec`、`nohup`、`time` で包んだコマンドは、ほどいて見る。迂回は `ALLOW_NO_VERIFY=1`
- block-no-verify が塞ぐのは、Claude Code が誤って、または習慣で書きうる形（git のオプション、`git config`、
  シェルの代入と `env`）まで。設定ファイルを直接書き換える、別のプログラムから git を起動する、といった意図して
  回避する形は塞がない（コマンドの文字列からは全てを見分けられず、Claude Code に回避する理由も無い）

### 帰結

- 良い点: 同じコマンドで作るディレクトリ、変数のパス、同じコマンドの `git add` でも、グローバルの hook を使う
  リポジトリでは、コミットに入る中身を検査する
- 良い点: `commit -a` を別に扱う必要が無い（pre-commit は -a で足した後の index を見る）
- 悪い点: ローカルの core.hooksPath を持つリポジトリでは、前と同じく、同じコマンドの `git add` を見落とし、対象の
  ディレクトリを決められないときは検査しない
- 悪い点: Claude Code が `--no-verify` などで git の hook を飛ばせない。飛ばしたいときは `ALLOW_NO_VERIFY=1` を付ける
- 悪い点: git の hook の検査はコマンドを実行した後に走るので、止まったときは `git add` まで済んでいる（コミット
  だけが止まる）

### 確認

- git の hook の検査のテスト（`modules/home/parts/tests/pre-commit-secrets.sh`）: 同じコマンドの add、`commit -a`、
  Markdown の自動修正とローカルの pre-commit がステージし直した中身、種類の変更、fork、競合の無いマージ、迂回、
  Claude Code のセッションでないとき、gitleaks の失敗
- PreToolUse の block-secret-commit と block-no-verify のテスト（`.claude/tests/hooks.sh`）
