---
status: accepted
date: 2026-10-11
requires: [ADR-0007]
supersedes: []
superseded-by:
issues: ["#303"]
---

# ADR-0020: guard-sbatch は slurm-run.sh のうち、同名のテストだけを止めない

## 背景

`guard-sbatch`（`modules/home/parts/claude-hooks/src/rules/guard_sbatch.rs`）は、`slurm-run.sh` が Bash ツールの
フォアグラウンドで走るのを止める（ADR-0007）。実行するプログラムを basename だけで判定していたので、同名のテスト
`.claude/tests/scripts/slurm-run.sh` も止めた。このテストは偽物の sbatch で走り、Slurm には触れない。2026-10-11 に
`bash .claude/tests/scripts/slurm-run.sh` が止められ、`run_in_background=true` で実行する手間がかかった。

## 決定の要因

- 実際に Slurm へ投げるスクリプトは、どの書き方でも止める（ガードは、見落としたら通す側ではなく止める側に倒す）
- 禁止する場所の一覧を足さない
- テストはスクリプトと同名にする規約（`.claude/tests/scripts/<スクリプト名>`）を崩さない

## 検討した案

- 案 A: 本物の置き場を肯定形で認める。パス付きの指定は `.claude/scripts/slurm-run.sh` で終わるものだけを本物として止め、残りは通す
- 案 B: 除くのを、同名のテスト自身（`.claude/tests/scripts/slurm-run.sh`）の 1 つだけにする。残りは止める
- 案 C: `/tests/` を含むパスを除外する
- 案 D: テストのファイル名を変える（`slurm-run.test.sh` など）

## 決定と理由

採用: 「案 B」。除くのは同名のテスト（`.claude/tests/scripts/slurm-run.sh`）だけで、本物の書き方がどれだけあっても（`~`、`$HOME`、相対、絶対、`bash` 経由、
`env` などの前置き越し、nix store のパス）、見落としは止める側に倒れる。

- 案 A（実装して捨てた）: 実装してローカルレビューにかけたところ、通す側に倒れる書き方が見つかった。`scripts/slurm-run.sh`、
  `../scripts/slurm-run.sh`、`~/.claude/scripts/./slurm-run.sh`、`/nix/store/…-claude-scripts/slurm-run.sh` は、
  どれも本物を指すが `.claude/scripts/` で終わらず、通した。本物の書き方を数え上げることはできないので、肯定形で
  認める判定は漏れる
- 案 C: 除外の一覧（`/tests/`、あとで `/fixtures/` など）が増えていく。`/tmp/x/tests/scripts/slurm-run.sh` のような、
  本物でも `tests` を含むパスを通してしまう
- 案 D: 他のテストは `.claude/tests/scripts/<スクリプト名>` で揃っており、1 本だけ名前を変えると規約が崩れる

### 帰結

- 良い点: テストを止められずに走らせられる。本物のスクリプトは、書き方によらず止める
- 悪い点: テストを `cd .claude/tests/scripts && bash slurm-run.sh` のように、ディレクトリ無しで呼ぶと止める
  （cwd を知る手段が無い）。`.claude/tests/./scripts/slurm-run.sh` のような正規でない綴りも止める。
  そのときは `run_in_background=true` で実行する
- 既知の穴: `bash -x <script>` のようにオプションを付けた呼び出しは、元からプログラムを取り違えて見逃す（この決定の
  前からある。#303）。「どの書き方でも止める」は、これを直すまでは `bash` / `sh` のオプション付きを除く
- 除くのは、パスが `.claude/tests/scripts/slurm-run.sh` で終わるもの（前に `/` か何も無いもの）。別の worktree や
  clone のテストも同じ名前で除く
- 見直す条件: 同名のテストが増えたとき（除くものが 1 つでなくなる）

### 確認

`.claude/tests/hooks.sh` の `guard-sbatch` の節で、本物の各書き方（案 A が通した 4 つを含む）が止まることと、
`.claude/tests/scripts/slurm-run.sh` の各書き方が通ることを確かめる。

## 補足

- 2026-10-21: 決定の要因の「どの書き方でも止める（見落としたら止める側に倒す）」と、それに基づく案 A を捨てた
  理由は、ADR-0042 で今後の判断の基準から外した（hook はモデルが普通に書く形で判定し、作為的な抜け道は塞がない）。
  決定（同名のテストだけを除く）と、入っているコードとテストはそのまま
