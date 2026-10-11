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
- 案 B: 検査をグローバルの git の pre-commit に移す。PreToolUse は gitleaks をやめ、`git commit --no-verify` を止める
- 案 C: 両方で検査する

## 決定と理由

採用: 案 B。

- git の pre-commit は、コミットするリポジトリの中で、`git add` と `commit -a` を済ませた後の index を見る。対象の
  ディレクトリの解決も、同じコマンドの add の見落としも起きない（main への直接コミットのガードを git の pre-commit
  にも置いているのと同じ理由。`git.nix`）
- 案 A は、同じコマンドの add の見落としが残る。コマンドの文字列から実行の後の状態を推すことになる
- 案 C は、PreToolUse の側が案 A と同じ誤りで正しいコミットを止め続ける
- git の hook は `--no-verify` で飛ばせるので、Claude Code のセッションでは PreToolUse で `git commit --no-verify`
  （`-n`）を止める。git の hook を飛ばす理由は、ほかの検査（main への直接コミット、形式の検査など）にも無い

検査の置き方:

- グローバルの pre-commit が、Claude Code のセッション（`CLAUDECODE=1`）のコミットで、`gitleaks git --staged` を
  走らせる。対象は今までと同じ（Claude Code のセッションのコミット）。プロジェクトのローカルの pre-commit を呼ぶ前
  に走らせ、fork でも飛ばさない（機密情報はどのリポジトリでも漏らさない）
- gitleaks は Nix store のパスで呼ぶ（PATH に頼らない）
- 漏えいは場所とルール ID だけを出し、値は出さない（`--redact`）。gitleaks が失敗したら、漏えいと分けて出して止める
- 迂回は今までと同じく `ALLOW_SECRET_COMMIT=1`（コマンドに付けた環境変数を hook が見る）。誤検出は、行の
  `gitleaks:allow`、`.gitleaksignore`、`.gitleaks.toml` で除く
- PreToolUse の規則は block-secret-commit から block-no-verify-commit にする。迂回は `ALLOW_NO_VERIFY=1`

### 帰結

- 良い点: 同じコマンドで作るディレクトリ、変数のパス、同じコマンドの `git add` でも、コミットに入る中身を検査する
- 良い点: `commit -a` を別に扱う必要が無い（pre-commit は -a で足した後の index を見る）
- 悪い点: Claude Code が `git commit --no-verify` を使えない。git の hook を飛ばしたいときは `ALLOW_NO_VERIFY=1` を付ける
- 悪い点: 検査はコマンドを実行した後に走るので、止まったときは `git add` まで済んでいる（コミットだけが止まる）

### 確認

- git の pre-commit の検査のテスト（`modules/home/parts/tests/pre-commit-secrets.sh`）
- PreToolUse の block-no-verify-commit のテスト（`.claude/tests/hooks.sh`）
