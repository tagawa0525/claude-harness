---
status: accepted
date: 2026-10-12
requires: [ADR-0007]
supersedes: []
superseded-by:
issues: ["#340"]
---

# ADR-0030: Slurm の全ジョブに、パーティションの重みで同じ基礎の優先度を与え、scontrol top と Nice で並べ替えられるようにする

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

- 重みを足しても、待っているジョブは今と同じく投げた順（ジョブ番号の順）に走ること（ADR-0007）
- 投げた本人が、`scontrol top` か `--nice` で待っているジョブを並べ替えられること
- 走っているジョブを止めずに反映できること

## 検討した案

- 案 A（採用）: `PriorityWeightPartition=10000` を置く。パーティションは `main` の 1 つだけなので、全ジョブの優先度が同じ
  10000 になり、そこから nice の分だけ下がる
- 案 B: `PriorityWeightAge` を置き、待った時間で優先度を上げる
- 案 C: `scontrol hold` と `release` で並べ替える（ADR-0007 の案 B）
- 案 D: 利用者に `scontrol update Priority=` を許す（Slurm の操作者か管理者にする）

## 決定と理由

採用: 「案 A」。全ジョブの基礎の優先度が同じなので、同じ優先度どうしはジョブ番号の順に走り、今の順番は変わらない。
基礎が 1 より十分大きいので、`_set_top` が前のジョブの優先度を下げられ、`--nice` も効く。優先度の値は時間で変わらず、
`squeue -o '%i %Q'` を見れば、誰がどれだけ下げたかが nice の分として読める。

- `modules/slurm/common.nix` の `extraConfig` に `PriorityWeightPartition=10000` を置く。`PriorityType` は既定の
  `priority/multifactor` のまま書かない
- 10000 は、`scontrol top` が 1 回に下げる量（同じ優先度どうしなら 1）と、`--nice` に書く数より十分大きい値として選んだ

### 帰結

- 良い点: `slurm-run.sh --top` と `scontrol top` が働き、急ぐ短いジョブを長いジョブの前に出せる
- 悪い点: `scontrol top` を繰り返すと、前にいたジョブの nice が少しずつ積み上がる。基礎の 10000 を使い切るほど繰り返すと、
  また下げられなくなる
- 見直す条件: 利用者が増えて公平さが要るようになったとき（ADR-0007 の見直す条件と同じ）

### 確認

- 反映後、`squeue -o '%i %Q'` で待っているジョブの優先度が 10000 になること
- `-c 1` の 1 分のジョブを 2 本投げ、後ろのジョブに `scontrol top` を当てると、`squeue` の順と走る順が入れ替わること

## 各案の長所と短所

### 案 B（待った時間の重み）

古いジョブほど優先度が上がるので順番は投げた順に近いが、優先度が時間で変わる。`_set_top` が付けた差は待った時間の差で
縮み、`PriorityMaxAge`（既定 7 日）に届くまでは値が動き続けるので、`squeue` で並びを読み解きにくい。

### 案 C（hold と release）

今の権限でできるが、別のエージェントが held のジョブを「他人の保留」と誤解して自分のジョブを取り消した（#340）。
戻し忘れたジョブは走らない。

### 案 D（`Priority=` の権限）

順番を直に決められるが、操作者の権限は他人のジョブにも及ぶ。エージェントに渡す権限が広すぎる。

## 補足

- 反映で走っているジョブは止まらない。slurmd は `KillMode=process` と `Delegate=yes` で動く（2026-10-12、
  `systemctl show slurmd`）ので、slurmd を再起動しても slurmstepd とジョブは残る。slurmctld は状態を保存して読み直す
