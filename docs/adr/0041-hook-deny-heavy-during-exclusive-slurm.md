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

ADR-0026 は案 B（時間を測るジョブの間は手元で全コアを使わない）と、その結果の「数分のビルドも手元でよい」を、「長い測定の間はビルドを含むチェックも PR も
止まる」ことと「数分のビルドの乱れは誤差として小さい」ことから採らなかった。今回の事故は、
測り直しになった（10 分の測定）ほか、同じ違反が 1 日に 5 回以上繰り返された。

## 決定の要因

- 文章の規則は読み飛ばされる。squeue で機械的に判定できる禁止は hook に置く（`.claude/README.md` の使い分け）
- 測定の間も、1 コアに限れば手元の作業は続けられる。止めるのは全コアを使いうる形だけにする
- r995 以外のホストでは判定の材料が無い。誤って止めるより通す（hook は検査であって環境の診断ではない）。
  ただし squeue があるのに結果を使えないときは、黙って通さず利用者に見せる（ADR-0033）
- 判定できない形（何を走らせるか決められないコマンド）は、止める側に倒す（ADR-0025）
- マージの前のチェック（`run-checks.sh`）は、排他のジョブの間（数時間）に止めると、マージが数時間できない。
  hook で止めず、スクリプト自身が 1 コアに限って遅く走らせる

## 検討した案

- 案 A: 規則を文章のまま強める。2026-10-15 に守られなかったため採らない
- 案 B: 排他のジョブ中は、手元の cargo と nix build をすべて止める。ADR-0026 の案 B と同じで、測定の間のチェックと PR が
  止まる。`taskset` で 1 コアに限れば乱れは小さいので、そこを通す形にする
- 案 C: エスケープ（`ALLOW_…=<理由>`）を設ける。今回の違反は「わかって外す」でなく規則を見落とした例だった。
  外す必要が出たら（ADR-0023）hook の不足として issue に書く
- 案 D（採用）: 排他のジョブが走行中のときだけ、対象のコマンドを、1 コアの `taskset` でない限り止める
- 案 E: 当初の案。`-j 1` も通す。レビューで、`-j` は cargo のコンパイルの並列度の一部しか限らない（rustc の
  codegen、proc-macro と build.rs、テストの実行、実行するプログラムは別にスレッドを使う）と指摘され、採らなかった
- 案 F: `run-checks.sh` を hook で止める。排他のジョブの間マージできなくなるため採らず、スクリプトが自己制限する

## 決定と理由

案 D を採る。`claude-hooks` の `guard-exclusive-slurm` が、次をすべて満たすときに deny する。

- コマンドが次のどれか。`env`・`sudo` などの包みと、`nix develop|shell [<flake>] -c <コマンド>` の中身はほどいて
  判定する（xlc の DVC の段階は `nix develop ../..#references -c` で包む）
  - `cargo build|check|test|clippy|run|bench`（alias の `b|c|t|r` も）
  - `dvc repro`
  - `nix build`、`nix flake check`
  - `bin/` か、`release`・`debug` ディレクトリ（`target/`、`target/<triple>/`、`CARGO_TARGET_DIR` の下）にある
    `xs|lattice|core|chain`（`../../bin/xs` など相対も）
  - 何を走らせるか決められないコマンド（`$CMD test`、`bash -s`、`source x.sh`）。サブコマンドを決められない
    `cargo`・`nix`・`dvc`（知らないオプションの後の語が、その値かサブコマンドか分からない）も同じ扱い
- `taskset` が中のコマンドを 1 つの CPU に限っていない（`-c 3`、`-c 5-5`、マスク `0x4`。複数の CPU や読めない
  指定は 1 コアとみなさない）。コマンドを直に包む最も内側の `taskset` が効き、`bash -c '…'` の中のコマンドにも
  外側の指定が効く。`-j 1` と `CARGO_BUILD_JOBS=1` は限りとみなさない
- `squeue -h -t R -o '%h|%j|%i'` に OverSubscribe（`%h`）が `NO` の行がある。r995 の実機で、排他のジョブが `NO`、
  `-shared` などで共有できるジョブが `OK` か `YES` と出る

squeue は、対象のコマンドがあるときだけ呼ぶ。出力は別のスレッドで並行して読み（パイプを満たして終われないのを
避ける）、5 秒で打ち切る。`-O` の固定幅の列は長い名前がジョブ番号と続くので、区切りのある `-o` で読む。
squeue が無いホストと、排他のジョブが無いときは何もしない。squeue が失敗した、5 秒で終わらない、出力を読めない
ときは、通すが理由を systemMessage に出す。`cargo` と `nix` のサブコマンドは、値を取るグローバルオプションを
飛ばして読む。`nix develop|shell` の `-c` は、値を取るオプション（`--option k v`、`--expr` など）の値を飛ばして
最初のものを取る（`--help` があれば何も走らせない）。

拒否の文言にジョブの名前と番号を入れ、直し方（`taskset` で 1 コアに限る、終わるまで待つ、`slurm-run.sh` で
投げる）を示す。エスケープは設けない。

`run-checks.sh` は、同じ squeue の判定（`.claude/scripts/lib/slurm-exclusive.sh`）で排他のジョブを見つけたら、
冒頭に知らせて 1 コアで走らせる（`cargo` を `taskset -c <最後の CPU>` と `-j 1` で、`nix flake check` を
`--cores 1 -j 1` で）。このスクリプトと `run-python-tests.sh` は hook の対象にしない。

### 帰結

- ADR-0026 の「数分のビルドも手元でよい」を、排他のジョブ中に限って覆す。排他の測定の間、手元の全コアを使う
  `cargo`・`nix build`・格子計算は `taskset -c <1 コア>` に限られる。ビルドを含むチェックは遅くなるが止まらない。
  ADR-0026 自体は書き換えない（案 B を、1 コアに限る条件つきで取り入れる）
- 対象は決め打ちの一覧で、別の重い計算（直接の `openmc`、`cargo nextest`、`nix run`、`nix-build`）は止めない。
  足りないものは見つかったときに足す
- 何を走らせるか決められないコマンドは、排他のジョブ中は `taskset` で包まない限り止まる。誤って止めるのは、
  排他のジョブの間だけである
- 排他でないジョブ（`-shared`、OverSubscribe が `OK`・`YES`）は対象外。#365 の run-checks.sh の直列化とは別の問題である

### 確認

- `.claude/tests/hooks.sh` が、偽の `squeue` で deny・許可・対象外・排他のジョブ無しと、`nix develop -c sbatch` が
  guard-sbatch で止まることを確かめる。`modules/home/parts/claude-hooks/tests/git_spawn.rs` が、squeue の失敗の
  通知、squeue が無いときの沈黙、UTF-8 でない名前を確かめる
- `.claude/tests/scripts/language-checks.sh` が、`run-checks.sh` の自己制限を確かめる
