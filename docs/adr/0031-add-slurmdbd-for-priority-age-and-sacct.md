---
status: proposed
date: 2026-10-12
requires: [ADR-0007, ADR-0017]
supersedes: []
superseded-by:
issues: ["#340"]
---

# ADR-0031: r995 に slurmdbd と MariaDB を置き、待った時間の優先度と sacct を使えるようにする

## 背景

ADR-0017 では、履歴のための常駐サービスを増やさないことを要因にして、slurmdbd を採らなかった（案 A）。

Issue #340 で、`scontrol top` で待っているジョブを並べ替えられるようにする。並べ替えで後ろに回したジョブが、その後に投げられた
ジョブに抜かれ続けないためには、待った時間で優先度を上げる `PriorityWeightAge` が要る。2026-10-12 に r995 で試すと、
slurmctld は `error: PriorityWeightAge can only be used with SlurmDBD, ignoring` を出して重みを無視した。

Slurm 26.05.4 の accounting の置き場は `accounting_storage/slurmdbd` だけで、slurmdbd の DB は MySQL 系
（`src/plugins/accounting_storage/mysql`）だけである。SQLite や PostgreSQL は使えない。

## 決定の要因

- 待った時間の優先度を、Slurm の機能で使えること（#340）
- エージェントが、終わったジョブの時間とメモリを名前や期間で引けること（今は jobcomp のファイルを grep し、メモリは残らない）
- 保守の手間を Nix の設定に閉じること（ユーザーの判断。2026-10-12）

## 検討した案

- 案 A（採用）: slurmdbd と MariaDB を r995 に置き、`AccountingStorageType=accounting_storage/slurmdbd` にする
- 案 B: slurmdbd を置かず、`slurm-run.sh --top` が、投げたジョブが走り始めたら後ろに回したジョブの nice を戻す

## 決定と理由

採用: 「案 A」。待った時間の優先度が Slurm の中で効き、`slurm-run.sh` を通さない `scontrol top` にも効く。`sacct` で
履歴を引け、ジョブのメモリの最大値（`MaxRSS`）も残る。設定は nixpkgs の `services.slurm.dbdserver` と
`services.mysql` で済み、ユーザーは Nix の設定なら保守の手間は大きくないと判断した。

- `modules/slurm/server.nix` で `services.slurm.dbdserver` と `services.mysql`（MariaDB）を有効にし、DB `slurm_acct_db`
  と、その全権を持つ `slurm` の利用者を作る
- DB の認証は unix socket で、パスワードを置かない。slurmdbd は `SlurmUser`（`slurm`）になってから DB に接続する
  （`src/slurmdbd/slurmdbd.c` の `become_slurm_user` のあとの `acct_storage_g_get_connection`）
- `modules/slurm/common.nix` に `AccountingStorageType=accounting_storage/slurmdbd` と `AccountingStorageHost=r995` を
  置く。slurm.conf は全ホストで同じにする
- クラスタ（r995）の登録は、slurmctld より前に走る oneshot（`slurm-register-cluster`）が、登録されていなければ
  `sacctmgr add cluster` で行う。登録がないと、slurmctld は記録を送れない（slurmdbd が `trying to register a cluster
  (r995) with no remote port` を出す。2026-10-12 に r995 で確かめた）
- MariaDB は、slurmdbd が推奨する値のうち `innodb_lock_wait_timeout=900` と `max_allowed_packet=16M` だけ合わせる。
  `innodb_buffer_pool_size` の推奨（4 GiB 以上。`accounting_storage_mysql.c` の `_check_database_variables`）は大きな
  クラスタ向けで、履歴だけの小さな DB には既定（128 MiB）のまま置き、slurmdbd の警告は受け入れる
- ほかのホストの `sacct` が r995 の slurmdbd（6819）に届くよう、tailscale0 に開ける
- jobcomp（ADR-0017）は残す。`slurm-history.sh` がそれを読む

### 帰結

- 良い点: `PriorityWeightAge` が効く。`sacct` で履歴を集計でき、メモリの最大値も残る。QOS も使えるようになる
- 悪い点: MariaDB と slurmdbd が r995 に常駐する。DB は履歴だけなので、失っても計算は止まらない（バックアップは取らない）
- 見直す条件: 常駐の資源が計算の邪魔になったとき

### 確認

- 反映後、`sacctmgr show cluster` に r995 が出ること、短いジョブを 1 本流して `sacct` に出ること
- slurmctld のログに `PriorityWeightAge can only be used with SlurmDBD` が出ないこと

## 各案の長所と短所

### 案 B（slurm-run.sh が nice を戻す）

常駐を増やさないが、`slurm-run.sh` を通さない `scontrol top` の後始末はされない。並べ替えの後始末をスクリプトに持たせる
ので、ほかの `--top` と重なったときの扱いも要る。`sacct` は使えないまま。
