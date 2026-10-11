---
status: accepted
date: 2026-10-11
requires: [ADR-0015, ADR-0020]
supersedes: []
superseded-by:
issues: ["#303", "#300"]
---

# ADR-0025: hook は bash -c の文字列を深さ 5 まで再帰的に読み、包みの表を広げ、読めない形は対象語を含む文字列なら止める

## 背景

claude-hooks の PreToolUse hook は、Bash ツールのコマンド文字列を tree-sitter-bash で解析し、各ルールが「実際に走る
プログラム」を `Cmd::program()` と `shell.commands()` で見る。PR #301 で、`env`、`nice`、`timeout` などの包みを
`unwrap.rs` の表でほどく土台ができた。その上で、2026-10-11 に次の見逃しが分かった（#303、#300）。

- `bash -x <script>`、`bash -e <script>`、`sh -eu <script>` は、`bash` / `sh` の次の語をそのままスクリプトと読むので、
  オプションをプログラムと取り違える（basename の判定の頃からある穴。ADR-0020 の「既知の穴」）
- `bash -c '<文字列>'` の文字列の中を見ない
- `sudo`、`xargs`、`flock` で包んだ形を見ない（#300 で ADR-0015 の線引きの外として見送っていた）
- `$CMD`、`eval "$CMD"`、`source`、`bash -s` のように、文字列からは走らせるものが決まらない形を通す

同日に公開ソースを調べた結果:

- Claude Code 自身の権限判定（https://code.claude.com/docs/en/permissions）が剥がす包みは `timeout`、`time`、`nice`、
  `nohup`、`stdbuf`、`command`、`builtin` と素の `xargs` だけで、`bash -c '…'` は再帰せず、「deny に当たらず、境界では
  ない」と限界を明記している
- 公開の類似ツール（destructive_command_guard、claude-code-safety-net）は `bash -c` / `python -c` の内側も見る方向

## 決定の要因

- ガードは、見落としたら通す側ではなく止める側に倒す（ADR-0020）。ただし `echo sbatch` や `grep sbatch` のような
  引数に現れるだけの形を止める偽陽性は増やさない
- 共通の土台（`shell.rs` の `Cmd`、`unwrap.rs`）を直し、ルールごとの分岐を増やさない。`guard_sbatch`、
  `require_background_wait`、`guard_git_add`、`guard_git_push`、`block_no_verify` など、実行プログラムを見るルール
  すべてに同じ読み方が効くこと
- 包みごとの「値を取るオプション」と「コマンドが始まる位置」は man から取る（推測で書かない）
- 入れ子の文字列を読む処理は、入力の形によらず有限の時間で終わる

## 検討した案

- 案 A: `bash` / `sh` のオプションを表で持ち、`-c` の文字列は同じ構文解析を再帰的に当てる（深さの上限つき）。
  包みの表に `sudo`、`xargs`、`flock`、`chrt`、`taskset`、`watch`、`script -c`、`ssh <接続先> …`、`eval` を足す。
  走らせるものが決まらない形は、コマンド文字列のどこかに対象語があれば止める側に倒す
- 案 B: 未知のコマンド（`$CMD` など）は許可する。Claude Code 自身の権限判定と同じ考え方
- 案 C: 引数のどこかに対象語があれば止める保険を入れる（全部のコマンド、または未知の先頭語に限る）
- 案 D: `bash -c` の文字列は見ない（Claude Code 自身の権限判定と同じ境界）。包みの表だけ広げる
- 案 E: 文字列の再帰に深さの上限を置かない、または上限を超えたら通す

## 決定と理由

採用: 「案 A」。

採らなかった案:

- 案 B: 権限判定は「許可するか」を決めるもので、未知なら人に聞けばよい。ガードは「禁じた形を止める」もので、
  未知を通すと偽陰性がそのまま害になる（sbatch の直接の投入、--no-verify）
- 案 C: `echo sbatch`、`grep -n sbatch CLAUDE.md`、コミットの本文に書いた `git add -A` を止める偽陽性が多い。
  未知の先頭語に限る形でも、`$TOOL sbatch.log` のような引数を止める。文字列に対象語があり、かつ走らせるものが
  決まらないコマンドがある、という 2 つの条件に絞った（案 A）
- 案 D: `bash -c 'sbatch …'` は Claude Code が書く形で、読まない理由が無い。tree-sitter の解析は文字列にも
  そのまま当てられる
- 案 E: `bash -c "bash -c \"…\""` の入れ子は、1 段ごとにエスケープが倍になり、深さは Claude Code が書く形では
  2 を超えない。上限を超えた文字列を通すと、深く包めば抜けられる。超えたら止める側に倒す

### 読み方

`unwrap.rs` の表（値を取る短縮形、続けて書いたときだけ値を取る短縮形、長い形、コマンドの前の位置引数の数、
`NAME=value` を代入として読むか）と、包みごとの意味:

- `bash` / `sh`: 1 文字のオプションは `set` と同じ（`-e`、`-x`、`-o opt`、`+o opt`、`-O shopt`）。長い形は
  `--rcfile` / `--init-file` が値を取る。`-c` は次の位置引数を文字列として走らせ、残りは `$0` 以降。`-s` と
  位置引数の無い呼び出しは標準入力を読む（決まらない）。`-n` と `-D` は走らせない。それ以外は次の位置引数が
  スクリプト
- `sudo`: `[VAR=value]` を代入として読む（env と同じく、最初の代入でオプションは終わる。getopt の `+`）。`-E` /
  `--preserve-env` が無ければ環境変数を受け継がない（env_reset。`--preserve-env=list` は list だけなので、受け継が
  ない側）。`-D` / `--chdir` はディレクトリを変える。`-e`、`-l`、`-v`、`-V`、`-K` は走らせない。`-s` / `-i` は、
  コマンドがあればそれを（sudo が英数字以外をエスケープして渡すので、語は変わらない）、無ければ対話シェル
  （決まらない）
- `xargs`: `-e[eof]`、`-i[replace]`、`-l[max-lines]` は続けて書いたときだけ値を取る（getopt の optional_argument）。
  標準入力から足す引数は見ない（`xargs git add -A` は、パスが足されるとしても `-A` だけの形として止める）
- `flock`: 位置引数 1 つ（ファイル）の直後の `-c <文字列>` / `--command` を sh -c で走らせる（flock.c は getopt の
  後に `argv[optind + 1]` を比べる）。`flock <fd>` は何も走らせない
- `chrt`: 位置引数 1 つ（優先度）。`-p` と `-m` は走らせない。`taskset`: 位置引数 1 つ（マスク）。`-p` は走らせない
- `watch`: 残りの語を空白でつないで sh -c で走らせる。`-x` は語をそのまま exec する
- `script`: `-c <文字列>` を sh -c で走らせる（getopt が語を並べ替えるので、ログのファイルの後ろの `-c` も読む。
  util-linux 2.42.3 で確かめた）。無ければ `--` の後ろの語をコマンドとして走らせ、それも無ければ対話シェル
  （決まらない）
- `ssh`: 位置引数 1 つ（接続先）の残りの語を空白でつないで、接続先のシェルが走らせる。接続先の後ろのオプション
  も読む（ssh.c は接続先を取ると getopt をやり直す。`ssh host -p 3333` で確かめた）。環境変数は渡らない。
  コマンドが無ければ接続先のシェルが標準入力を読む（決まらない）。`-G`、`-N`、`-O`、`-Q`、`-V`、`-Z`、`-s` は
  走らせない
- `eval`: 残りの語を空白でつないで走らせる。`source` / `.`: ファイルを読んで走らせる（決まらない）

`shell.rs`:

- 文字列（`Unwrapped::source`）は `Shell::parse` を再帰的に当て、中のコマンドを外側の `commands()` の、外側の
  コマンドの直後に含める。深さの上限は `MAX_DEPTH = 5`（トップレベルが 0）。超えた文字列は読まず、そのコマンドを
  「決まらない」にする
- 文字列の中は 1 つのスコープ。前の `cd` は、同じか外側のスコープ（スコープの列の前置き）にあれば効く。これで
  `cd x; (git commit)` や `cd x; bash -c 'git commit'` の `cd` も効く（前は同じスコープだけだった）。文字列の中の
  `cd` は外に漏れない。外側の環境変数とディレクトリの変更（`env -u`、`env -C`、`sudo` の env_reset）は中に効く
- 文字列の中の代入、`unset`、エスケープ（ADR-0023 の `ALLOW_*=<理由>`）も、外側と同じ規則で読む
- 走らせるプログラムが決まらないコマンド（`Cmd::opaque`）: 名前の basename に展開がある（`$CMD`、`~/x/$SCRIPT`、
  `$(which sbatch)`、`${SB:-/usr/bin/sbatch}`。区切りの `/` は展開の外のものだけ数えるので、
  `$HOME/.claude/scripts/slurm-run.sh` と `${DIR}/slurm-run.sh` は決まる）、標準入力やファイルから読む（上の表）、
  深さの上限を超えた
- block-no-verify は、環境変数の変更（CLAUDECODE が渡らない、`GIT_CONFIG_*`）を hook が走る `git commit` /
  `merge` / `pull` でだけ見る。`sudo ls` や `ssh host nvidia-smi` も環境変数を受け継がないが、hook は走らない
- `Shell::runs_program(word, is_target)`: `is_target` を満たすプログラムを走らせるコマンドがあるか、または決まらない
  コマンドがあり、コマンド文字列のどこか（ヒアドキュメントの本文や注釈を含む）に `word` が現れる。実行プログラムの
  名前だけで止めるルール（`guard_sbatch`、`require_background_wait`）はこれを使う。git の引数を見るルールは、
  決まらないコマンドの引数を評価できないので、今までどおり見ない
- バックスラッシュのエスケープ（`a\ b`、`"c\"d"`）は bash と同じに外す。`printf %q` で 1 段ずつ包んだ文字列を
  読むのに要る
- `/usr/bin/git` のようなパスは、PR #301 の時点で `basename` に揃えてある

### 足さなかった包み

- `doas`、`su -c`: この環境に無い。要るときに表へ足す
- `python -c`、`perl -e`: 別の言語で、bash の構文解析は当てられない
- `find -exec`、`parallel`、`nix develop -c`、`nix-shell --run`: コマンドを引数の途中や値として取り、形が多い。
  Claude Code が重い計算や git にこれらを使う形は見ていない。要るときに足す
- `zsh`、`dash`、`ksh`: Bash ツールは bash を走らせ、スクリプトの起動は `bash` / `sh` で書く

### 帰結

- 良い点: `bash -x script`（#303）、`bash -c '…'`、`sudo` などの包み、`$CMD` の形を、実行プログラムを見るすべての
  ルールが同じ読み方で見る。`cd x; (git commit)` の `cd` も効く
- 悪い点: 決まらないコマンドと対象語が同じ文字列にあると止める（`echo sbatch; $CMD` のような形。その場で分かる
  ので書き直せる）。`xargs git add -A` は、足されるパスで範囲が限定されていても止める。`env -C x bash -c 'cd y && …'`
  は中の `cd` を先に当てる（順序は持たない。まれな形）
- `sudo git commit`（`-E` 無し）と `ssh <接続先> git commit` は、CLAUDECODE が渡らず hook が Claude Code の
  セッションと分からなくなるので、block-no-verify が止める。これは ADR-0015 の「Claude Code のセッションが検査を
  飛ばせない」の通り
- 包みが受け付けるオプションが変わったら（man が変わったら）表を直す。知らないオプションは今までどおり読み飛ばす
- 見直す条件: 決まらない形の偽陽性が作業を繰り返し止めたとき。深さ 5 を超える正当な入れ子が出たとき

### 確認

- `.claude/tests/hooks.sh`: `guard-sbatch` の節で、`bash` / `sh` のオプション付き、`bash -c` の入れ子（6 段で止める側）、
  各包み、走らせない形、決まらない形（対象語の有無で分かれる）を確かめる。`require-background-wait`、
  `block-no-verify`、`guard-git-add`、`guard-git-push`、`block-main-commit` の節で、同じ土台が効くことを確かめる
- `modules/home/parts/claude-hooks/src/unwrap.rs` と `shell.rs` の単体テストで、表の読み方、文字列の位置と
  スコープ、エスケープの外し方、深さの上限を確かめる
