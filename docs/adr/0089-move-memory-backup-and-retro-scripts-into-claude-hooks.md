---
status: accepted
date: 2026-11-14
requires: [ADR-0074, ADR-0076, ADR-0082, ADR-0084, ADR-0085, ADR-0086, ADR-0087]
supersedes: []
superseded-by:
issues: ["#101"]
---

# ADR-0089: 自動メモリの退避（Stop の hook）と retro のスクリプトの中身を claude-hooks のサブコマンドに移す

## 背景

段階 8（#101）で、retro のスクリプトと `claude-memory-backup.sh` を Rust に移し、ネイティブの Windows でも動かす
（ADR-0082）。段階 7 までは済んだ（2026-11-14、`cf444ec1`。段階 3 の #108 は残る）。

retro のスクリプトは `.claude/skills/retro/scripts/` の `memory-index.sh`（134 行）、`prose-size.sh`（206 行）、
`session-events.sh`（257 行。うち jq を含む行が 12）で（`cf444ec1`、`wc -l`、`grep -c jq`）、スキルの文（`retro/SKILL.md`）が
`~/.claude/skills/retro/scripts/<名前>.sh` のパスで呼び、`allowed-tools` の `Bash(~/.claude/skills/retro/scripts/*)` で
許す。`memory-index.sh` は、崩れた日本語の語を `~/.claude/bin/ja-terms check` で検査する（ADR-0074）。形は、段階 5 の
作業手順のスクリプト（名前とパスで呼ばれ、hook でなく作業の節目にスキルが起動する）と同じで、ADR-0087 はそれらを
`claude-hooks script <名前>` に移し、ファイルは呼ぶだけの sh にした。

`claude-memory-backup.sh`（122 行）は、Claude Code の Stop の hook（応答の終わりごと）から呼ばれ、自動メモリ
（`~/.claude/projects/*/memory/`）を `~/.claude` の git の ref `refs/memory-backup` にコミットする（ADR-0076）。
Linux では `claude-code.nix` が `writeShellApplication`（git、jq、flock、coreutils）で包んで `~/.claude/bin/claude-memory-backup`
に置き、`claude-setup settings --stop <パス>` が settings.json の Stop に登録する（ADR-0084）。Windows には無い
（ADR-0084 の帰結。jq と flock に頼る sh のため）。処理は次のとおり:

- hook の JSON（標準入力。端末なら読まない）から `session_id` と `cwd` を jq で取り、コミットの本文に入れる
- `git rev-parse --local-env-vars` の変数を外す（親の環境の `GIT_DIR` などに引かれない）
- `<gitdir>/memory-backup.lock` を `flock -w 30` で取り、親を読んでから `update-ref` で ref を進めるまでを 1 つずつにする。
  ロックを外すと、同時の 6 本のうち 5 本が `update-ref` の古い値の不一致で落ちる（ADR-0076 の確認）。git の子にロックの
  fd を継がせないよう、`git` を `9>&-` で包む（fsmonitor の daemon などが fd を持つと、次の退避が待ちきれない）
- 一時の index（`GIT_INDEX_FILE`）に `git add -f` し、`write-tree`、`commit-tree`、`update-ref`（古い値に親）で進める。
  一時のディレクトリは `trap` で消す
- 失敗したら、1 行目に `claude-memory-backup:` で始まる理由を出して 1 で終える（Claude Code は 0 と 2 のほかを止めない
  失敗として扱い、1 行目を通知に出す）

振る舞いのテストは `modules/home/parts/tests/claude-memory-backup.sh`（205 行）と `.claude/tests/scripts/retro.sh`。

決めること: 退避の中身の置き場と、settings.json への登録の形、ロックの取り方。retro のスクリプトの呼ばれ方。テスト。

## 決定の要因

- Windows で入れるものを増やさない（ADR-0084 の手順は crate ごとに `cargo install` する）
- Windows で jq と flock を要らなくする（jq は winget で別に入れ、flock は Git Bash に無い）
- 同時に終わった応答の退避が、どれも失われない（ADR-0076）
- スキルの文と hook のルールが名前で呼ぶものは、名前を変えるなら同じ PR で揃える（ADR-0087）

## 検討した案

退避の置き場:

- 案 1: `claude-hooks` のサブコマンド（`claude-hooks stop [claude-dir]`）にし、settings.json の Stop には `claude-hooks` を
  引数 `stop` で登録する（PreToolUse の `claude-hooks pre-tool-use` と同じ形）
- 案 2: 新しい crate `claude-memory-backup` にし、名前と登録の形を変えない
- 案 3: `claude-hooks` の crate に 2 つ目のバイナリ（`[[bin]] claude-memory-backup`）を足す

ロック:

- 案 A: 標準ライブラリの `File::try_lock`（Rust 1.89 で安定。Unix は flock、Windows は LockFileEx）を、30 秒まで間を置いて
  繰り返す
- 案 B: crate（`fs4`、`fd-lock`）の時間つきのロックを使う
- 案 C: ロックをやめ、`update-ref` の古い値の不一致で落ちたら、親を読み直して作り直す（回数に上限を置く）

retro のスクリプト:

- 案 ア: ADR-0087 の段階 5 のスクリプトと同じく、中身を `claude-hooks script <名前>` に移し、ファイルは
  `lib/claude-hooks-script.sh` を読んで `claude_hooks_script <名前> "$@"` を呼ぶだけの sh にする
- 案 イ: スキルの文を `claude-hooks script <名前>` に書き換え、スクリプトのファイルを消す

## 決定と理由

採用: 案 1、案 A、案 ア。

- 退避: `claude-hooks stop [claude-dir]` に、今の `claude-memory-backup.sh` の振る舞い（退避の範囲、symlink のメモリの
  ディレクトリと上の階層のリポジトリでの失敗、`commit-tree` と古い値つきの `update-ref`、失敗の 1 行目と終了コード）を
  移す。失敗の 1 行目の名前は `claude-hooks stop:` にする（通知で、どのコマンドの失敗かが分かるように）。git は今と同じく子のプロセスで呼び、`--local-env-vars` の変数は子の環境から外す。hook の JSON は serde_json で読む。
  引数の誤りを含め、0 と 1 のほかで終えない（Stop の hook の 2 は応答を続けさせる）
- 登録: `claude-setup settings` は Stop に `claude-hooks`（`~/.claude/bin/claude-hooks`）を引数 `stop` で登録し、前の
  `claude-memory-backup` の登録を外す。Linux は `claude-code.nix` の `writeShellApplication` と `~/.claude/bin/claude-memory-backup`
  を消す。Windows は、`cargo install` で入れた `claude-hooks` をそのまま登録する（手順の追加は無い）
- ロック: `<gitdir>/memory-backup.lock` を開き、`File::try_lock` を 30 秒まで繰り返す。取れなければ今と同じ理由を出して失敗
  する。Rust の `Command` は子に開いたファイルを継がせない（Unix は `O_CLOEXEC`、Windows は継承しないハンドル）ので、
  `9>&-` に当たる手当ては要らない
- retro のスクリプト: ADR-0087 の決定（中身、スクリプトのファイル、Windows の置き場、テスト）をそのまま当てる。
  `memory-index` の辞書の検査は、hook と同じく `claude-hooks` の隣の `ja-terms` を起動する（`tools.rs`、ADR-0085）
- PR の単位: 退避で 1 つ、retro のスクリプトごとに 1 つ以上。sh の実装は、中身を移した PR で消す（retro は呼ぶだけの sh に
  置き換える）
- テスト: 退避の振る舞いは `claude-hooks` の結合テストに移し、`modules/home/parts/tests/claude-memory-backup.sh` は移した
  PR で消す（呼ぶだけのファイルが無いので、配備の確認として残すものが無い）。同時の 6 本が落ちないこと、ロックを別の
  プロセスが持つと失敗すること、git が起動して残る子にロックを継がせないことも、そこで確かめる。retro は ADR-0087 のとおり、結合テストを足し、
  `.claude/tests/scripts/retro.sh` を配備の確認として残す

理由:

- 案 1: Windows で入れる crate が増えず、hook の JSON の読み方と、`~/.claude/bin/claude-hooks` の置き場をそのまま使える。
  登録の形を変える費用は `claude-setup` の 1 か所で、前の登録を外す処理は、旧 bash hook の登録の掃除（`--legacy`）と同じく
  置き換えの判定に足すだけで済む。案 2 は、Windows の手順に `cargo install` が 1 つ増え、版のずれうるバイナリが 1 つ増える。
  案 3 は、`claude-hooks` をライブラリとバイナリに分けるか、モジュールを `#[path]` で 2 つのバイナリから読む作業がかかり、
  得るのは名前を変えないことだけ（名前で見分けるのは `claude-setup` だけ）
- 案 A: 依存を足さずに、今の flock と同じ排他と待ち方になる。案 B は、標準ライブラリにある機能のために依存を足す。
  案 C は、ADR-0076 が確かめた振る舞い（ロックで 1 つずつ）を変え、上限の回数を決めて確かめる作業が増える
- 案 ア: ADR-0087 の案 A の理由（スキルの文と `allowed-tools` を変えず、移す PR がスクリプト 1 つに収まる）がそのまま
  当てはまる。案 イ は、`allowed-tools` の許可の形を変えることになり、揃え漏れが許可の抜けになる

### 帰結

- 良い点: Windows でも自動メモリが退避される（2026-11-14 から。`~/.claude` が git のリポジトリの直下であることは、Linux と
  同じく利用者が用意する。README の「Windows で使う」）。retro の 3 本も移し終えた（2026-11-14）ので、スキルと hook が呼ぶ
  ハーネスの sh で jq を使うものは無い。テストの sh（Windows の CI が流す `tests/merge-ja-terms.sh` など）と `windows.yml` は、まだ
  jq を使う。Windows の手順（README）の前もって入れるものからは、jq を外した（2026-11-14、#209）
- 悪い点: `claude-hooks` が、Stop の hook と retro の処理も持つ。名前は段階 8 の後に見直す（ADR-0087 の帰結、#101）
- 悪い点: `claude-hooks` の不具合が、PreToolUse の hook とメモリの退避の両方を止めうる（今は別のバイナリ）
- 悪い点: ADR-0087 の帰結と同じく、移す PR の後は `claude-sync` でなく rebuild（Windows は `cargo install` の後に
  `claude-setup`）で入れる。登録を `claude-hooks stop` に替えた settings.json は、そのサブコマンドを持つバイナリと揃って入る。
  揃わないとき（Windows で `claude-setup` だけを新しくした、`claude-hooks` を前の版に戻した、Linux で home-manager の前の世代に
  戻した。前の世代の `claude-setup` は `claude-hooks stop` の登録を知らずに残す）は、前の版の `claude-hooks` が
  知らないサブコマンドに使い方を出して 2 で終え、応答の終わりごとに Claude Code が応答を続ける
- 悪い点: `claude-sync` だけで呼ぶだけの `memory-index.sh` を入れ、rebuild の前の `claude-hooks` が `memory-index` を
  知らない間は、`TERMS:` だけでなく索引の照合（`ENTRIES:`、`MISSING:`、`UNINDEXED:`、`BROKEN_LINK:`）の行も出ず、
  `ERROR:` で始まらない使い方の文と入れ直しの案内を出して 2 で終える（ADR-0074 の帰結の、rebuild の前でもほかの行は出る、は
  sh の版のもの）

### 確認

- `claude-hooks` の結合テスト（`tests/stop.rs`）が、`modules/home/parts/tests/claude-memory-backup.sh` の確かめていた振る舞いに、
  引数の誤りの終了コード 1、読めない hook の JSON、リポジトリの下の階層、ロックを別のプロセスが持つときの 30 秒の待ちを
  足して、`nix flake check` と Windows の CI で通る（symlink のメモリのディレクトリ、git が起動して残る子、失敗する git hook と CLAUDECODE、UTF-8 でないプロジェクトの名前の
  4 件は Unix だけ）
- `claude-setup` のテストで、Stop の登録が `claude-hooks stop` の 1 件になり、前の `claude-memory-backup` の登録が消える
- retro のスクリプトの結合テスト（`tests/script_memory_index.rs`、`tests/script_prose_size.rs`、`tests/script_session_events.rs`）が、
  `nix flake check` と Windows の CI で通る。session-events は、手元の 19 の記録（`~/.claude/projects` の、2026-11-14 の時点のもの）を、区間なしと 3 つの区間の
  76 通りで前の sh（`LC_ALL=C`）と移した版の両方で読ませ、出力と終了コードがすべて一致した。この 19 の記録には、jq が止まる
  形の行は無かった。前の sh と違えたのは、同じ回数の並び（前の sh ではロケールで変わった）と、jq が止まる形の行の扱い
  （5 で止まらず、理由を出して 1 で終える）で、どちらも結合テストで確かめる
- 移した後、retro のスクリプトのファイル（`memory-index.sh`、`prose-size.sh`、`session-events.sh`）は `lib/claude-hooks-script.sh` を
  読んで `claude_hooks_script <名前>` を呼ぶだけになり、jq を使わない。`.claude/tests/scripts/retro.sh` は、それらをブランチの
  バイナリで呼ぶ配備の確認として通る
