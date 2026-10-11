---
status: accepted
date: 2026-10-12
requires: [ADR-0007, ADR-0031]
supersedes: []
superseded-by:
issues: ["#340"]
---

# ADR-0030: Slurm の優先度に、全ジョブに同じ基礎と待った分の時間を与え、scontrol top で並べ替えられるようにする

## 背景

ADR-0007 で、待っている自分のジョブを `scontrol top` で先頭に出せるようにした（`SchedulerParameters=enable_user_top`）。
2026-10-11 の xlc の作業で、見積もりの長いジョブの後ろで待つ短いジョブに `scontrol top` と `scontrol update Nice=` を
使ったが、順番は変わらなかった（#340）。

r995 の設定（2026-10-12、`scontrol show config`）は `PriorityType=priority/multifactor` で、重み（`PriorityWeight*`）は
すべて 0 である。重みの和が 0 なので、multifactor は優先度を 1 に切り上げ（`priority_multifactor.c` の
`if (priority < 1) priority = 1;`）、`squeue -o '%i %Q'` でも全ジョブの優先度が 1 だった。

`scontrol top` の実装（Slurm 26.05.4 の `src/slurmctld/job_mgr.c` の `_set_top`）は、先頭に出すジョブより前のジョブの
nice を上げて優先度を下げる。そのとき、優先度が 1 以下のジョブは「下げると保留になる」として対象から外す。全ジョブが 1 なので
下げられるジョブが無く、何も変わらない。Nice も、優先度を下げた結果が 1 に切り上げられるので効かない。

## 決定の要因

- 並べ替えのない限り、待っているジョブは今と同じく投げた順に走ること（ADR-0007）
- 投げた本人が、`scontrol top` で待っているジョブを並べ替えられること
- 後ろに回したジョブが、その後に投げられたジョブに抜かれ続けないこと
- 走っているジョブを止めずに反映できること

## 検討した案

- 案 A（採用）: 基礎の優先度（`PriorityWeightPartition=10000`）と、待った 1 分ごとに 1 上がる優先度
  （`PriorityWeightAge=10080`）を足し、`PriorityCalcPeriod=1` で 1 分ごとに計算し直す
- 案 B: 基礎の優先度（`PriorityWeightPartition=10000`）だけを置く
- 案 C: 待った時間の優先度（`PriorityWeightAge`）だけを置く
- 案 D: `scontrol hold` と `release` で並べ替える（ADR-0007 の案 B）
- 案 E: 利用者に `scontrol update Priority=` を許す（Slurm の操作者か管理者にする）

## 決定と理由

採用: 「案 A」。優先度は「10000 + 待った分 − nice」になり、並べ替えのない限り先に投げたジョブほど高いので、投げた順に走る。
同じ分に投げたジョブは同じ優先度で、ジョブ番号の順に走る。`_set_top` は優先度の差を nice に移すので、後ろに回したジョブの
nice は「何分ぶん後ろに回したか」の意味になる。その後に投げられたジョブは、待った分が少ないので、回したジョブを抜かない。

- `PriorityWeightAge` は slurmdbd がないと無視される（slurmctld が `PriorityWeightAge can only be used with SlurmDBD,
  ignoring` を出す）ので、ADR-0031 の slurmdbd を前提にする
- `modules/slurm/common.nix` の `extraConfig` に `PriorityWeightPartition=10000`、`PriorityWeightAge=10080`、
  `PriorityCalcPeriod=1` を置く。`PriorityType` は既定の `priority/multifactor` のまま書かない
- 10080 は `PriorityMaxAge` の既定（7 日 = 10080 分）と同じ数で、優先度が 1 分に 1 上がる（`priority_multifactor.c` の
  `priority_age = diff / max_age` に重みを掛ける）。7 日を超えて待つジョブは上がり止まるが、使い方からは起きない
- `PriorityCalcPeriod=1` は、優先度を 1 分ごとに計算し直し、待った分を新しく投げたジョブの値（投げたときに計算する）と
  同じ刻みで比べるため。既定の 5 分では、計算し直す前に投げたジョブが、前から待つジョブを一時的に抜く
- 基礎の 10000 は、投げたばかりのジョブ（待った分が 0）の優先度を 1 より大きくし、`_set_top` が下げる対象に入れるため
- `squeue` の既定の表示（`SQUEUE_FORMAT`。ADR-0007）に、CPU 数（`%C`）と優先度（`%Q`）を足し、並べ替えた結果を
  `squeue` だけで読めるようにする。`%C` は、待っている排他のジョブでは 1（割り当てた後に 32）と出るので、排他かどうかは
  連絡先（`%k`）の `shared-cpus=` の印で見分ける

### 帰結

- 良い点: `slurm-run.sh --top` と `scontrol top` が働き、急ぐ短いジョブを長いジョブの前に出せる。後ろに回したジョブは、
  回された分のあとに投げられたジョブより先に走る
- 悪い点: `scontrol top` は同じ利用者の待っているジョブをすべて後ろに回す。エージェントはみな同じ利用者で投げるので、
  ほかのセッションのジョブも遅れる
- 悪い点: `--nice` に基礎（10000）より大きい数を書くと、優先度が 1 に切り上げられ、`_set_top` が下げる対象から外れる
- 前提: パーティションが `main` の 1 つだけで、`PriorityJobFactor` が既定の 1 であること。パーティションを足すか
  `PriorityJobFactor` を変えると、基礎の優先度がジョブごとに変わる
- 見直す条件: 利用者が増えて公平さが要るようになったとき（ADR-0007 の見直す条件と同じ）

### 確認

2026-10-12 に、このブランチの構成（ADR-0031 の slurmdbd を含む）を r995 に `nixos-rebuild test` で当てて確かめた。

- `sprio -l` の AGE が待った分になった。01:42 に投げた A（120）と B（121）が 32、01:56 に投げた 122〜125 が 18 で、優先度は
  10032 と 10018 だった
- 先に投げた A と B は、後から投げた 122〜125 より、待った分（14 分）だけ優先度が高かった。`--top` で付く nice が待った分の差より
  小さい限り、後ろに回したジョブは後から投げたジョブに抜かれない。nice を付けたままでの比較は、A の nice を手で 0 に戻した
  あとだったので試していない

## 各案の長所と短所

### 案 B（基礎の優先度だけ）

2026-10-12 に、このブランチの最初の版（基礎だけ）を r995 に `nixos-rebuild test` で当てて試した。`--top` で投げた B が
10000、先に投げた A が 9999（nice 1）になり、`squeue` の順と始まりの見込みが B、A に入れ替わった。
ただし、A の nice は残り続け、優先度は時間で変わらないので、その後に投げられるジョブ（10000）にすべて抜かれる。
新しいジョブが来続ける限り、A は走らない。

### 案 C（待った時間の優先度だけ）

投げたばかりのジョブは待った分が 0 で、優先度が 1 になる。`_set_top` は優先度が 1 のジョブを下げる対象から外すので、
投げてから 1 分の間は、そのジョブより前に出られない。

### 案 D（hold と release）

今の権限でできるが、別のエージェントが held のジョブを「他人の保留」と誤解して自分のジョブを取り消した（#340）。
戻し忘れたジョブは走らない。

### 案 E（`Priority=` の権限）

順番を直に決められるが、操作者の権限は他人のジョブにも及ぶ。エージェントに渡す権限が広すぎる。

## 補足

- 反映で走っているジョブは止まらない。slurmd は `KillMode=process` と `Delegate=yes` で動く（2026-10-12、
  `systemctl show slurmd`）ので、slurmd を再起動しても slurmstepd とジョブは残る。slurmctld は状態を保存して読み直す
- 反映の前から待っているジョブの優先度も、multifactor が slurmctld の起動時と `PriorityCalcPeriod`ごとに
  計算し直す（Slurm 26.05.4 の `priority_multifactor.c` の `_decay_apply_new_usage_and_weighted_factors`）ので、
  1 のまま取り残されない
