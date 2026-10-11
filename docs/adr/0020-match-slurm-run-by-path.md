---
status: accepted
date: 2026-10-11
requires: [ADR-0007]
supersedes: []
superseded-by:
issues: []
---

# ADR-0020: guard-sbatch は slurm-run.sh を .claude/scripts のパスで判定する

## 背景

`guard-sbatch`（`modules/home/parts/claude-hooks/src/rules/guard_sbatch.rs`）は、`slurm-run.sh` が Bash ツールの
フォアグラウンドで走るのを止める（ADR-0007）。実行するプログラムを basename だけで判定していたので、同名のテスト
`.claude/tests/scripts/slurm-run.sh` も止めた。このテストは偽物の sbatch で走り、Slurm には触れない。2026-10-11 に
`bash .claude/tests/scripts/slurm-run.sh` が止められ、`run_in_background=true` で実行する手間が毎回かかった。

## 決定の要因

- 実際に Slurm へ投げるスクリプトは止め続ける（偽陽性より、見逃しのほうが害が大きい）
- 禁止する場所の一覧を足さない。本物の置き場を肯定形で示す
- テストはスクリプトと同名にする規約（`.claude/tests/scripts/<スクリプト名>`）を崩さない

## 検討した案

- 案 A: パス付きの指定は、`.claude/scripts/slurm-run.sh` で終わるものだけを本物とみなす。ディレクトリ無しの指定は、
  cwd が分からないので本物とみなす
- 案 B: `/tests/` を含むパスを除外する
- 案 C: テストのファイル名を変える（`slurm-run.test.sh` など）

## 決定と理由

採用: 「案 A」。本物の置き場は `.claude/scripts/` の 1 か所（`~/.claude/scripts/`、リポジトリの `.claude/scripts/`、worktree の同じ場所）で、肯定形で書ける。
`./slurm-run.sh` と `slurm-run.sh` は cwd を知る手段が無いので、止める側に倒す。

- 案 B: 除外の一覧（`/tests/`、あとで `/fixtures/` など）が増えていく。テスト以外の同名ファイルの置き場を、
  先に数え上げることはできない
- 案 C: 他のテストは `.claude/tests/scripts/<スクリプト名>` で揃っており、1 本だけ名前を変えると規約が崩れる

### 帰結

- 良い点: テストを止められずに走らせられる。本物のスクリプトは、どの書き方（`~`、`$HOME`、相対、絶対、`bash` 経由、
  `env` などの前置き越し）でも今までどおり止める
- 悪い点: `cd .claude/tests/scripts && bash slurm-run.sh` のように、cwd を変えてディレクトリ無しで呼んだテストは、
  本物と区別できず止める。そのときは `run_in_background=true` で実行する
- 見直す条件: `.claude/scripts/` 以外の置き場から本物を呼ぶ運用が生まれたとき

### 確認

`.claude/tests/hooks.sh` の `guard-sbatch` の節で、`.claude/scripts/slurm-run.sh` の各書き方が止まることと、
`.claude/tests/scripts/slurm-run.sh` の各書き方が通ることを確かめる。
