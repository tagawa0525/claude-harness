---
status: accepted
date: 2026-10-11
requires: [ADR-0007]
supersedes: []
superseded-by:
issues: []
---

# ADR-0017: Slurm のジョブの見積もりと実際の時間を、jobcomp と slurm-run.sh の出力で後から読めるようにする

## 背景

ADR-0007 で、`--time` を上限ではなく見積もりとして使い、超えても止めない（`OverTimeLimit=UNLIMITED`）ことにした。
見積もりが妥当だったかを振り返るには、ジョブが終わったあとに見積もりと実際の時間を並べて読める必要がある。

2026-10-11 に設定を確認すると、`modules/slurm/common.nix` に `AccountingStorageType` も `JobCompType` も無く、
Slurm の既定（`accounting_storage/none`）のままで、`sacct` は履歴を返せない。`OverTimeLimit=UNLIMITED` なので、
`State=TIMEOUT` でも見積もりの超過は分からない。終了後に `scontrol show job` で読めるのは `MinJobAge`（既定 300 秒）の間だけである。

## 決定の要因

- 履歴のための常駐サービスを増やさない（利用者は 1 人で、ジョブも 1 本ずつ流す）
- `slurm-run.sh` を通さない投入（直接の `sbatch`）でも、記録が残ること
- `slurm-run.sh` で投げた本人が、終わった直後に見積もりと実際の時間を見られること

## 検討した案

- 案 A: `AccountingStorageType=accounting_storage/slurmdbd` を使う（slurmdbd と MariaDB を足す）
- 案 B: `JobCompType=jobcomp/filetxt` で、終了したジョブを slurmctld が 1 行ずつファイルに書く
- 案 C: `slurm-run.sh` が、終わったあとに見積もりと実際の時間を出力する
- 案 D: 案 B と案 C の両方

## 決定と理由

採用: 「案 D」。案 B は `slurm-run.sh` を通らないジョブも含めて、全ジョブの記録が残る。案 C は投げた本人が終わった
直後に比べられる。互いの穴を埋める。決めたのはユーザー。

- `modules/slurm/common.nix` に `JobCompType=jobcomp/filetxt` と `JobCompLoc=/var/log/slurm/job_completions` を置き、
  `/var/log/slurm` を `slurm` ユーザーのディレクトリとして tmpfiles で作る
- `slurm-run.sh` は、ジョブが終わった直後に `scontrol show job` から `RunTime` と `TimeLimit` を読み、
  `TIME: elapsed=<実際> estimate=<見積もり>` を出力し、同じ行をジョブのログの末尾にも書き足す

### 帰結

- 良い点: 追加のサービスなしに、全ジョブの履歴と、`slurm-run.sh` の出力で見積もりとの比較が残る
- 悪い点: jobcomp のファイルは増え続ける。`sacct` のような集計や絞り込みはできない（`grep` と `awk` で読む）。
  `slurm-run.sh` の出力は `MinJobAge` の間に `scontrol` が読めたときだけ出る
- 見直す条件: 利用者が増える、または集計が要るようになったとき（案 A に移る）

### 確認

- `slurm-run.sh` の出力は `.claude/tests/scripts/slurm-run.sh` で確かめる
- 反映後に、短いジョブを 1 本流し、`/var/log/slurm/job_completions` に 1 行増えることと、`TIME:` が出ることを確かめる

## 各案の長所と短所

### 案 A（slurmdbd）

`sacct` で集計できるが、slurmdbd と MariaDB という常駐サービスが増え、保守の対象になる。QOS も使えるようになるが、
今は要らない（ADR-0007 の案 C を採らなかった）。

### 案 B（jobcomp/filetxt）だけ

直接投げたジョブも残るが、投げた本人が終わった直後に見積もりとの差を見るには、ファイルを探して読む手間がかかる。

### 案 C（slurm-run.sh の出力）だけ

直接の `sbatch` のジョブは残らない。

## 補足

- 2026-10-12: 案 A（slurmdbd）を、待った時間の優先度のために採った（ADR-0031）。この ADR の jobcomp と TIME 行は残す
