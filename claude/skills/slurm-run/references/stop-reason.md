# ジョブが止まった理由を読む

ジョブが止まった理由は、`MaxRSS` と `ReqMem` を比べて推さず、下の 3 つで読む。r995 の Slurm はジョブのメモリを数えず、制限も
しない（`scontrol show config` で `SelectTypeParameters = CR_CORE`、`TaskPlugin = (null)`、cgroup.conf に `ConstrainRAMSpace` なし）。
`ReqMem`（排他のジョブでは RealMemory の 86000M）は要求の値で、超えても Slurm は止めない。メモリで止まるのは、機械の全体を
使い切ったときのカーネルの OOM killer である。

1. `sacct -j <番号> -X -o JobID,State,ExitCode,Start,End`（`-X` が無いと `.batch` と `.extern` の行も出る）。`ExitCode` は `--wrap` の
   一番外のコマンドの値で、中のプログラムの値でない（`dvc repro` の中で `lattice` が SIGKILL で止まっても、ジョブは `FAILED` の
   `255:0`）。Slurm がメモリで止めた形（`OUT_OF_MEMORY`）は、この設定では出ない
2. ログの末尾。中のプログラムの終わり方（`強制終了`、dvc の `exited with 137`）が出る。137 は SIGKILL で、OOM killer に限らない
3. r995 で `journalctl _TRANSPORT=kernel --since '<Start>' --until '<End>' | grep -E 'oom-kill:|Out of memory'`（時刻は 1 の `Start` と
   `End`。`-k` は今の起動だけを読むので使わない）
   - カーネルの OOM killer なら、`global_oom` と `task=<プログラム名>,pid=<番号>` の行と、`Killed process <番号> (<名前>)` の
     `anon-rss`（殺したときの使用量）が出る
   - global の OOM は機械の全体から殺すプロセスを選ぶので、`task=` の名前がジョブのプログラムで、時刻がログの止まった所と合うかを
     見る（止められたジョブや `-c` のジョブと重なっていれば、別のジョブのプロセスでありうる）
   - ジャーナルが残っている期間（`journalctl --list-boots` の最初の起動から）で行が無ければ、カーネルの OOM killer でない。r995 で動く
     systemd-oomd が殺したときは `journalctl -u systemd-oomd` に出る
