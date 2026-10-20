---
status: accepted
date: 2026-10-20
requires: [ADR-0007, ADR-0023, ADR-0026]
supersedes: []
superseded-by:
issues: ["#365"]
---

# ADR-0041: 排他の Slurm のジョブ中の全コアの手元の計算を hook で止める

## 背景

排他の Slurm のジョブ（時間を記録する測定。ADR-0007、ADR-0026）の間に手元で全コアを使うと、その時間が乱れて
測り直しになる。2026-10-13 に手元の 16 スレッドの格子計算で 10 分の測定が測り直しになり、2026-10-15 には委任した
担当が 5 回以上、排他のジョブ中に `cargo test`・`cargo build`（全コア）・格子の計算を手元で走らせた。
規則は `.claude/skills/slurm-run/SKILL.md` と delegate スキルに書いてあったが、守られなかった。

ADR-0026 は案 B（時間を測るジョブの間は手元で全コアを使わない）を、「長い測定の間はビルドを含むチェックも PR も
止まる」ことと「数分のビルドの乱れは誤差として小さい」ことから採らなかった。今回の事故は、
測り直しになった（10 分の測定）ほか、同じ違反が 1 日に 5 回以上繰り返された。

## 決定の要因

- 文章の規則は読み飛ばされる。squeue で機械的に判定できる禁止は hook に置く（`.claude/README.md` の使い分け）
- 測定の間も、1 コアに限れば手元の作業は続けられる。止めるのは全コアを使いうる形だけにする
- r995 以外のホストや squeue の失敗では判定の材料が無い。誤って止めるより通す（hook は検査であって環境の診断ではない）

## 検討した案

- 案 A: 規則を文章のまま強める。2026-10-15 に守られなかったため採らない
- 案 B: 排他のジョブ中は、手元の cargo と nix build をすべて止める。ADR-0026 の案 B と同じで、測定の間のチェックと PR が
  止まる。`-j 1` と `taskset` で 1 コアに限れば乱れは小さいので、そこを通す形にする
- 案 C: エスケープ（`ALLOW_…=<理由>`）を設ける。今回の違反は「わかって外す」でなく規則を見落とした例だった。
  外す必要が出たら（ADR-0023）hook の不足として issue に書く
- 案 D（採用）: 排他のジョブが走行中のときだけ、対象のコマンドを、1 コアの `taskset`（`cargo check|clippy` は `-j 1` も）でない限り止める
- 案 E: 当初の案。`cargo build|test|run|bench` も `-j 1` で通す。レビューで、`-j` はコンパイルの並列度だけで、rustc の codegen やテストの実行や
  実行するプログラムのスレッド数を限らず、事故の再発を許すと指摘され、採らなかった

## 決定と理由

案 D を採る。`claude-hooks` の `guard-exclusive-slurm` が、次をすべて満たすときに deny する。

- コマンドが `cargo build|check|test|clippy|run|bench`（alias の `b|c|t|r` も）、`dvc repro`、
  `bin/xs|lattice|core|chain` と `target/release|debug/` の同名のバイナリ（`../../bin/xs` など相対も）、`nix build`、`nix flake check` のどれか（`env`・`sudo` などの包みと、`nix develop|shell [<flake>] -c <コマンド>`
  の中身はほどいて判定する。xlc の DVC の段階は `nix develop ../..#references -c` で包む）
- `cargo check|clippy` に `-j 1`・`-j1`・`--jobs 1`・`--jobs=1`（サブコマンドの前でもよい）や `CARGO_BUILD_JOBS=1` が無く、`taskset` が中のコマンドを 1 つの CPU に
  限っていない（`cargo build|test|run|bench` は `-j 1` では限れないので `taskset` だけが通す）
  （`-c 3`、`-c 5-5`、マスク `0x4`。複数の CPU や読めない指定は 1 コアとみなさない）
- `squeue -h -t R -o '%h|%j|%i'` に OverSubscribe（`%h`）が `NO` の行がある（r995 の実機で、排他のジョブが `NO` と出ることを確認した）

squeue は対象のコマンドがあるときだけ呼び、出力は別のスレッドで並行して読む（パイプを満たして終われないのを避ける）。squeue が無いホスト、squeue の失敗、5 秒で終わらないとき（slurmctld が
応答しない）、排他のジョブが無いときは何もしない。`-O` の固定幅の列は長い名前がジョブ番号と続くので、区切りのある
`-o` で読む。`taskset` は、コマンドを直に包む最も内側のものが効き、`bash -c '…'` の中のコマンドにも外側の指定が効く。
`cargo` と `nix` のサブコマンドは、値を取るグローバルオプション（`--config`、`-Z`、`--extra-experimental-features`、
`--option`）を飛ばして読む。拒否の文言にジョブの名前と番号を入れ、直し方（1 コアに
限る、終わるまで待つ、`slurm-run.sh` で投げる）を示す。エスケープは設けない。

### 帰結

- 排他の測定の間、手元の `cargo test`・`cargo build`・`nix build`・格子計算は `taskset -c <1 コア>` に
  限られる。ビルドを含むチェックは遅くなるが止まらない。ADR-0026 の案 B を、1 コアに限る条件つきで取り入れる
- `-j 1` はコンパイルの並列度だけを限る。`cargo build` は rustc の codegen が、`test`・`run`・`bench` はテストの実行や
  実行するプログラムが複数スレッドで走るので、`-j 1` を通さず `taskset` だけを通す
- 対象は決め打ちの一覧で、別の重い計算（直接の `openmc`、`cargo nextest`、`nix run`、`nix-build`）は止めない。足りないものは見つかったときに足す
- 排他でないジョブ（`-shared`、OverSubscribe が YES）は対象外。#365 の run-checks.sh の直列化とは別の問題である

### 確認

- `.claude/tests/hooks.sh` が、偽の `squeue` で deny・許可・対象外・排他のジョブ無し・squeue の失敗を確かめる
