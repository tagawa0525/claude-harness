---
status: accepted
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
- slurmctld は起動時に slurmdbd に名乗り、slurmdbd は知らないクラスタを、slurmctld の ClusterID のまま登録する
  （`src/slurmdbd/proc_req.c` の `_register_ctld`）。slurmdbd が応じる前に名乗ると登録されず、記録が送れない
  （slurmdbd が `trying to register a cluster (r995) with no remote port` を出す。2026-10-12 に r995 で確かめた）。
  slurmdbd は `Type=simple` なので、`postStart` で `sacctmgr` が応じるまで待ち、slurmctld をその後に起こす
- `JobAcctGatherType=jobacct_gather/linux` を置き、ジョブのメモリと CPU の使用量を集める。既定（none）では `sacct` の
  `MaxRSS` が空になる
- MariaDB は、slurmdbd が推奨する値のうち `innodb_lock_wait_timeout=900` と `max_allowed_packet=16M` だけ合わせる。
  `innodb_buffer_pool_size` の推奨（4 GiB 以上。`accounting_storage_mysql.c` の `_check_database_variables`）は大きな
  クラスタ向けで、履歴だけの小さな DB には既定（128 MiB）のまま置き、slurmdbd の警告は受け入れる
- `innodb_snapshot_isolation=OFF` を置く。MariaDB 11.6.2 から既定で有効になり、slurmdbd は書き込みの競合で止まる
  （同じ `_check_database_variables`）。今の 11.4 では既定で無効だが、nixpkgs の `mariadb` が上がっても変わらないようにする
- slurmdbd は unix socket で接続するので、MariaDB は TCP を待ち受けない（`skip-networking`）
- ほかのホストの `sacct` が r995 の slurmdbd（6819）に届くよう、tailscale0 に開ける
- jobcomp（ADR-0017）は残す。`slurm-history.sh` がそれを読む

### 帰結

- 良い点: `PriorityWeightAge` が効く。`sacct` で履歴を集計でき、メモリの最大値も残る。QOS も使えるようになる
- 悪い点: MariaDB と slurmdbd が r995 に常駐する。DB は履歴だけなので、失っても計算は止まらない（バックアップは取らない）
- 見直す条件: 常駐の資源が計算の邪魔になったとき

### 確認

- 反映後、`sacctmgr show cluster` に r995 とポート 6817 が出ること、短いジョブを 1 本流して `sacct` に `MaxRSS` つきで出ること
- slurmctld のログに `PriorityWeightAge can only be used with SlurmDBD` が出ないこと

## 各案の長所と短所

### 案 B（slurm-run.sh が nice を戻す）

常駐を増やさないが、`slurm-run.sh` を通さない `scontrol top` の後始末はされない。並べ替えの後始末をスクリプトに持たせる
ので、ほかの `--top` と重なったときの扱いも要る。`sacct` は使えないまま。

## 補足

- 2026-10-12: クラスタの登録を、slurmctld より前に走る oneshot（`sacctmgr add cluster`）で行う方式を試して捨てた。
  `no remote port` を登録の欠けと取り違えていた（本当の原因は起動の順番）。r995 では slurmctld が slurmdbd なしで作った
  ClusterID（1236。`/var/spool/slurmctld/clustername`）と、`sacctmgr add cluster` で slurmdbd が振った ID（3295）が食い違い、
  slurmctld が `CLUSTER ID MISMATCH` で止まった。`clustername` を消して起動し直すと、slurmdbd の ID を受け入れて書き直し
  （`src/slurmctld/controller.c` の `clusteracct_storage_g_register_ctld` のあと）、ジョブの状態は別のファイルにあるので
  待っているジョブも走っているジョブも残った
- 2026-10-12: 反映の直後に測った常駐の量（`systemctl show -p MemoryCurrent -p CPUUsageNSec`）は、mysql が 96 MiB と
  起動からの CPU 0.52 秒、slurmdbd が 4.6 MiB と 0.02 秒（r995 の物理メモリは 92 GiB）
- 2026-10-12: `innodb_buffer_pool_size=4G` を r995 に当てて測り、採らなかった。slurmdbd の警告は消えたが、MariaDB の
  起動直後のメモリ（`systemctl show -p MemoryCurrent`）が 89 MiB から 167 MiB に増えた（RSS は 194 MiB）。得られるのは
  警告が消えることだけで、大きな DB の移行や記録の削除の速さは、履歴だけの小さな DB には効かない
- 2026-10-12: slurmdbd が動いていないときに投げたジョブ（124、125。01:56 に投入、slurmdbd の最初の起動は 02:14）は、
  QOS が空のまま走り、`sacct` の記録は JobName が `allocation`、User が `root`、QOS が `Unknown` になった。動いているときに
  投げたジョブ（126、127）は QOS が `normal` で、名前と利用者も残った。assoc は 4 本とも無い（`AssocID` が 0）。
  slurmctld は QOS を投入のときに決め、後から付け直さない（管理者は `scontrol update JobId=<番号> QOS=normal` で変えられる）。
  `_set_top`（`src/slurmctld/job_mgr.c`）は QOS の違うジョブを下げる対象から外すので、両方が待ち行列に混ざっている間は
  `scontrol top` が効かなかった（126 を 124、125 の前に出せず、126 と 127 の間では効いた）
- 2026-10-12: `MaxRSS` は `JobAcctGatherFrequency`（30 秒）ごとに集めるので、それより短いジョブには残らない。slurmdbd の
  後に同時に投げた 5 秒のジョブ（129）は空で、60 秒のジョブ（130）は `.batch` の行に 8780K が残った。確認の「短いジョブ」は
  30 秒より長く走らせる。重い計算（ADR-0026）は 30 秒より長いので、決定の要因のメモリを引けることは満たす
