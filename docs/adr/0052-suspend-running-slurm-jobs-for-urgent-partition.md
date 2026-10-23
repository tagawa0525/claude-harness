---
status: accepted
date: 2026-10-23
requires: [ADR-0007, ADR-0026, ADR-0029, ADR-0030, ADR-0031, ADR-0041]
supersedes: []
superseded-by:
amends: [ADR-0007]
issues: ["#456"]
---

# ADR-0052: 急ぐジョブを、優先の高いパーティション urgent から、走っているジョブを SUSPEND で止めて実行する

## 背景

r995 の Slurm は、ジョブを投げた順に 1 本ずつ流し（ADR-0007、ADR-0030）、急ぐ短いジョブは `slurm-run.sh --top`
（`scontrol top`）で待っているジョブの前に出す。走っているジョブは止めない（ADR-0007 の決定の要因の 1 つ目）。
そのため、何時間もの計算が走っている間は、急ぐ数分の計算もその終わりを待つ。

ユーザーは 2026-10-23 の夜に「長い計算を途中でぶった切って急ぐ計算の優先度を上げたい」「メモリが潤沢なので SUSPEND は
有効」と求めた。r995 は物理 16 コア（SMT で 32）、メモリ 91 GB で、xlc の格子計算のジョブは数 GB、xs のライブラリの
作り直しは最大 16 GB 程度なので、止めたジョブと急ぐジョブがメモリに同居できる（数値は依頼の値で、この ADR では
測っていない）。

今の設定（`modules/slurm/common.nix`）は、Slurm 26.05.5、`SelectType=select/cons_tres`、`SelectTypeParameters=CR_Core`、
パーティションは `main` の 1 つで `OverSubscribe` は既定の `NO`、`PreemptType` は無し（割り込みは無効）、QOS は
accounting（ADR-0031）の既定の `normal` だけである。

Slurm 26.05.5 の文書（slurm.conf(5)、`doc/html/preempt.shtml`）とソースから、割り込みについて次を確かめた（2026-10-23）。

- `PreemptMode=SUSPEND` は、全体の `PreemptMode` に `GANG` を要する（slurm.conf(5) の PreemptMode。`src/common/read_config.c` は
  `PreemptMode=SUSPEND requires GANG too` で拒む）。止めたジョブは SIGTSTP、`suspend_grace_time`（既定 2 秒）の後に
  SIGSTOP を受け、再開で SIGCONT を受ける
- `OverSubscribe=NO` のパーティションのジョブは、`SUSPEND,GANG` でも割り込まれない。通常の共有を許さずに割り込みだけを
  許すには `OverSubscribe=FORCE:1` にする（slurm.conf(5) の OverSubscribe の NO の NOTE）
- `preempt/qos` で、割り込む側と割り込まれる側が同じパーティションにあると、2 つは gang scheduler で時間を分け合う
  （`SchedulerTimeSlice`、既定 30 秒ごとに交代）。別のパーティションなら、割り込まれたジョブは割り込んだジョブが終わるまで
  止まったままになる（slurm.conf(5) の PreemptMode の SUSPEND）
- `preempt/partition_prio` では、`PriorityTier` の高いパーティションのジョブが、低いパーティションのジョブを割り込む。
  割り込まれる側のパーティションの `PreemptMode=OFF` は割り込みを禁じる（`src/plugins/preempt/partition_prio` の
  `preempt_p_preemptable`）。高い `PriorityTier` のパーティションの待っているジョブは、低いパーティションのジョブより先に
  スケジュールされる（slurm.conf(5) の PriorityTier）
- `--exclusive` のジョブも止められる。sbatch(1) の `--exclusive` は、排他のジョブに要求した分のメモリしか割り当てない理由を
  「gang scheduling のため。止めたジョブはメモリに残る」と書く。`OverSubscribe=FORCE` のパーティションでも、`--exclusive` の
  ジョブにはノード全体を割り当てる（`src/slurmctld/node_scheduler.c` の `_resolve_shared_status` の表の `whole/O`）
- 止めたジョブのメモリを Slurm が数えるには、`CR_Core_Memory` などでメモリを資源にすることが勧められているが、
  必須ではない（`preempt.shtml` の SelectTypeParameter。`read_config.c` に検査は無い）
- 止めていた時間は、`scontrol show job` の `RunTime` に含まれない（`src/scontrol/info_job.c` は `pre_sus_time` と最後の
  再開からの時間の和を出す）。止めたことがあれば `SuspendTime` に最後に止めたか再開した時刻が入り、無ければ `None` と出る
  （`src/slurmctld/job_mgr.c` の `_job_suspend_op`）。`sacct` の `Suspended` は止めていた時間の合計で、`Elapsed` はそれを除く
  （`src/plugins/accounting_storage/mysql/as_mysql_jobacct_process.c`）。jobcomp/filetxt の行は止めた時間を持たない
  （`src/plugins/jobcomp/filetxt/jobcomp_filetxt.c` の `JOB_FORMAT`）

## 決定の要因

- 急ぐジョブが、走っている長いジョブを待たずに始まること（ユーザーの要望）
- 止めたジョブの計算が無駄にならないこと（ADR-0007 の決定の要因の 1 つ目の理由）
- 止められたジョブの時間を、黙って記録に使わせないこと（ADR-0026、ADR-0029。時間を記録する測定の乱れ）
- 設定を Nix の宣言に閉じること（ADR-0031 のユーザーの判断）
- 今の運用（投げた順、`--top`、`-c`、排他のジョブ中の hook。ADR-0029、ADR-0030、ADR-0041）を変えないこと

## 検討した案

割り込みの仕組み:

- 案 A（採用）: `PreemptType=preempt/partition_prio` で、`PriorityTier` の高いパーティション `urgent` を足し、`main` の
  ジョブを割り込む
- 案 B: `PreemptType=preempt/qos` で、急ぐ QOS（`urgent`）を `main` の中で `normal` を割り込ませる
- 案 C: 案 B の QOS に、案 A のパーティションを組み合わせる
- 案 D: 今の `--top` だけを続ける

割り込み方:

- 案 E（採用）: `PreemptMode=SUSPEND`（全体は `SUSPEND,GANG`）
- 案 F: `PreemptMode=REQUEUE`
- 案 G: SUSPEND と REQUEUE を QOS かパーティションで分ける

時間を記録する測定の扱い:

- 案 H（採用）: 測定も止められうるとし、止められたジョブを `slurm-run.sh` の TIME 行と `slurm-history.sh` で示す
- 案 I: 測定のジョブを、割り込めないパーティション（`PreemptMode=OFF`）か QOS に分ける

メモリ:

- 案 J（採用）: `CR_Core` のままにし、メモリの合計は投げる側が数える
- 案 K: `CR_Core_Memory` にしてメモリを Slurm に数えさせる

## 決定と理由

採用: 案 A、案 E、案 H、案 J。

これは ADR-0007 の決定の要因の「走っている計算は止めない（途中で止めると、それまでの計算が無駄になる。ユーザーの判断）」の
部分を置き換える。走っている計算は、`urgent` のジョブに限って一時停止（SUSPEND）され、その終わりに続きから再開する。
止めたジョブの計算は失われないので、要因の理由（無駄にしない）は保つ。ほかの部分は変わらない。

### 設定（`modules/slurm/common.nix`）

```text
PreemptType=preempt/partition_prio
PreemptMode=SUSPEND,GANG
PartitionName=main   Nodes=r995 Default=YES MaxTime=INFINITE State=UP OverSubscribe=FORCE:1 PriorityTier=1
PartitionName=urgent Nodes=r995 Default=NO  MaxTime=INFINITE State=UP OverSubscribe=FORCE:1 PriorityTier=2 PreemptMode=OFF
```

- `main` は全体の `PreemptMode`（SUSPEND）を受け、`urgent` のジョブに割り込まれる。`OverSubscribe=FORCE:1` は、割り込みを
  許しつつ、`main` の中の通常の共有（時間の分け合い）を許さない（slurm.conf(5) の FORCE と NO の NOTE）
- `urgent` は `PreemptMode=OFF` で、ほかのジョブに割り込まれない。`urgent` どうしも同じ `PriorityTier` なので割り込まない
- 割り込む側と割り込まれる側が別のパーティションなので、止めたジョブは `urgent` のジョブが終わるまで止まったままになる
  （時間を分け合わない）。`urgent` のジョブが必要とするコアに重なる `main` のジョブだけが止まり、空いているコアで足りれば
  何も止めない。排他（`slurm-run.sh` の既定）の `urgent` のジョブはノード全体を取るので、走っている `main` のジョブを
  すべて止める。少ないコアで足りる計算は `-c` と組んで投げる
- 2 つのパーティションの `PriorityJobFactor` は既定の 1 のままにする。ADR-0030 の基礎の優先度（`PriorityWeightPartition`）は
  全パーティションの `PriorityJobFactor` の最大で割った値に掛かるので、どちらのジョブも 10000 で変わらない
- `squeue` の既定の表示（`SQUEUE_FORMAT`）にパーティション（`%P`）を足し、`urgent` のジョブと、止められたジョブ
  （状態 `SUSPENDED`）を `squeue` だけで読めるようにする

QOS は使わず、`sacctmgr` の操作は要らない。設定はすべて slurm.conf に置き、全ホストで同じにする。

### `slurm-run.sh` と記録

- `--urgent` を足す。`--partition=urgent` を渡す。排他（既定）とも `-c` とも組み合わせられる
- TIME 行: `scontrol show job` の `SuspendTime` が `None` でなければ、行末に `suspended: 止められていたので時間は当てにならない`
  を足す。`SuspendTime` の欄が見つからなければ、`suspended=unknown` の印を足して警告する。`elapsed`（`RunTime`）は止めていた時間を除くが、止める前後のキャッシュの冷え、`suspend_grace_time` の重なり、
  プログラムが自分で測る経過時間（止めていた時間を含む）が混ざるので、記録に使わない
- `slurm-history.sh`: jobcomp の行は止めた時間を持たないので、表に入れうるジョブの番号を
  `sacct --allusers --duplicates -X -j <番号,…>` に渡し、番号と開始の時刻の組で結んで（番号は再利用されうる）、
  `Suspended` が 0 でないジョブを表と集計から除き、`SUSPENDED: <件数> (<番号>…)` で示す。`sacct` は
  `SLURM_TIME_FORMAT=standard` で、TZ を外して（slurmctld が jobcomp を書くシステムの時刻帯で）読む。`sacct` に無いジョブは、
  この ADR の日（2026-10-23）より前に始まったもの（accounting を置いた ADR-0031 より前を含む）だけを止められていないものとして
  扱う（割り込みはこの ADR からで、それより前に止まったジョブは無い）。それより後のジョブが無ければ slurmdbd の記録の遅れか
  欠けで、番号はあるのに開始が合わなければ書式の違いか番号の再利用で、どちらも止められたかを決められないので失敗する。
  `sacct` が失敗したときも、表を出さずに失敗する
- TIME 行の判定に `sacct` でなく `scontrol` を使うのは、終わった直後のジョブを slurmctld が覚えていて（`RunTime` と同じ出どころ）、
  slurmdbd への記録の遅れを待たずに済むため

### 帰結

- 良い点: 急ぐ短いジョブが、走っている長いジョブを待たずに始まる。止めたジョブは続きから再開し、計算は失われない
- 良い点: 待ち行列の中では `urgent` の待っているジョブが `main` の待っているジョブより先に走るので、`--top` で `main` の
  待っているジョブ（ほかのセッションのものを含む）を後ろに回さずに済む。`scontrol top` は同じパーティションのジョブの中でしか
  並べ替えない（ADR-0030 の補足の `_set_top`）
- 悪い点: 止められたジョブの経過（壁時計）は、止めていた時間だけ延びる。止められた排他のジョブの時間は記録に使えず、
  再測定になりうる。`urgent` は、再測定より急ぐ短い計算に限る
- 悪い点: メモリは Slurm が数えない（`CR_Core`）。止めたジョブはメモリに残るので、`urgent` のジョブとの合計が搭載量を
  超えると、OOM killer がどちらかを殺す。投げる側が、走っているジョブのメモリ（`sstat -j <番号>.batch` の `MaxRSS`。ステップを書かないと batch のステップを出さない）を見て数える
- 悪い点: SIGSTOP はジョブのプロセスだけを止める。ジョブが外のデーモンに頼んだ仕事（nix-daemon のビルドなど）は止まらない
- 悪い点: r995 は nixpkgs の既定の `ProctrackType=proctrack/linuxproc` で、ジョブのプロセスを親をたどって見つける
  （2026-10-23、`scontrol show config`）。親から外れたプロセス（デーモン化、二重 fork）には SIGSTOP が届かず、止められた
  はずのジョブの一部が走り続けうる。`proctrack/cgroup` に移すかは #456 で扱う
- 悪い点: 止められたジョブは、ファイルのロック（cargo のビルドディレクトリ、DVC の `.dvc/tmp/lock`）を持ったまま止まる。
  同じロックを待つ `urgent` のジョブは進まず、止められたジョブはその終わりまで再開しないので、互いに待ち続ける。
  `urgent` のジョブは、止めるジョブと同じ worktree、target ディレクトリ、DVC のリポジトリで実行しない（スキル slurm-run）
- ADR-0029 の帰結の「排他のジョブは、ノードに走っているジョブが 1 つもないときにしか始まらない」は、`main` のジョブには
  そのまま成り立つ。`urgent` のジョブは、`main` のジョブを止めて始まる
- ADR-0041 の hook と `run-checks.sh` は、走っている（`-t R`）排他のジョブを見る。止められた排他のジョブ（`SUSPENDED`）の
  間は手元の全コアの計算を止めない。止められた測定はもう記録に使わないので、守るものが無い
- 見直す条件: xlc の格子計算などに途中から再開する機能ができ、止めるより取り消して後で再開する方がよい場面（メモリが
  足りない、`urgent` のジョブが長い）が出たとき。案 F（REQUEUE）を `main` と別のパーティションで足せる

### 確認

- `slurm-run.sh` の `--urgent` と TIME 行の印、`slurm-history.sh` の `SUSPENDED` を、`.claude/tests/scripts/` の
  `slurm-run.sh` と `slurm-history.sh` で確かめる
- 設定の構文: `nix build` で r995、t14g4、x1ng1 の toplevel を作る。2026-10-23 に、このブランチの 7f8b5601 で 3 つとも作れた。
  生成した slurm.conf を `SLURM_CONF=<それ> scontrol show hostnames r995` が読めた（`PreemptMode=SUSPEND` だけにすると
  `PreemptMode=SUSPEND requires GANG too` で止まるので、全体の行は読まれている）。パーティションの行（`PreemptMode=OFF` など）は
  slurmctld だけが解するので、反映後の確認で確かめる
- 反映後に r995 で確かめる（ユーザーと親のセッション）:
  - `scontrol show config | grep -i preempt` が `PreemptMode = GANG,SUSPEND` と `PreemptType = preempt/partition_prio` を、
    `scontrol show partition` が 2 つのパーティションの `OverSubscribe=FORCE:1` と `PriorityTier` を出す
  - 排他の長いジョブ（`main`）を実行し、`--urgent` のジョブを投げると、長いジョブが `SUSPENDED` になり、`urgent` のジョブが
    走り、終わると長いジョブが `RUNNING` に戻る。長いジョブの TIME 行に `suspended:` が付き、`sacct` の `Suspended` が 0 でない
  - `-c` のジョブ（`main`）を実行している間、`main` の排他のジョブが `PENDING` のままである（ADR-0029 の帰結が
    `OverSubscribe=FORCE:1` でも成り立つ）
  - 排他のジョブの `squeue -o %h` が `NO` のままである（ADR-0041 の hook の判定）
  違えば、帰結を直す新しい ADR を書く

## 各案の長所と短所

### 案 B（preempt/qos、同じパーティション）

QOS は slurmdbd にあり、`sacctmgr add qos` で作る。Nix の宣言では持てず、activation script で冪等に作るか手順に
書くことになる。さらに、同じパーティションの割り込みは時間の分け合いになり（背景）、急ぐジョブは 30 秒ごとに長いジョブと
交代するので、全コアで走らない。

### 案 C（QOS とパーティション）

止めたままにするには結局パーティションを分ける（背景）。そのうえで QOS を足しても、利用者が 1 人で、割り込みの規則が
「urgent は main を割り込む」の 1 つだけのうちは、案 A に足すものが無く、`sacctmgr` の状態だけが増える。

### 案 D（`--top` だけ）

走っているジョブの終わりを待つことは変わらない。ユーザーの要望に応えない。

### 案 F（REQUEUE）

割り込まれたジョブは取り消されて待ち行列に戻り、最初からやり直す（slurm.conf(5) の REQUEUE）。途中から再開する機能の
無い計算は、それまでの計算が無駄になり、ADR-0007 の要因の理由に反する。メモリを解放できる利点はあるが、今のジョブは
メモリに同居できる（背景）。xlc で作っている再開の機能と組めるので、見直す条件に挙げる。

### 案 G（SUSPEND と REQUEUE を分ける）

再開の機能のあるジョブがまだ無く、分ける先が無い。案 F と同じく見直す条件で扱う。

### 案 I（測定を割り込めなくする）

時間を記録する測定は排他のジョブで、長いジョブの多くがそれに当たる。割り込めなくすると、`urgent` のジョブは長いジョブの
多くを待つことになり、要望に応えない。止められた測定は印で分かるので、再測定するかは投げた側が決める。

### 案 K（CR_Core_Memory）

止めたジョブのメモリを数え、同居できないときは割り込まない。ただし `DefMemPerNode` の既定は 0（無制限）で、`--mem` を
書かないジョブはノードのメモリ全体を取るので、`-c` のジョブも同居できなくなる。すべてのジョブにメモリの見積もりを書かせるか、
既定の値を決める必要があり、今の要望（メモリは潤沢）に対して負担が大きい。メモリが足りなくなったら見直す。
