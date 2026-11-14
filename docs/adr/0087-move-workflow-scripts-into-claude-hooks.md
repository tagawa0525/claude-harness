---
status: proposed
date: 2026-11-13
requires: [ADR-0082, ADR-0084, ADR-0086]
supersedes: []
superseded-by:
issues: ["#101", "#169"]
---

# ADR-0087: Git と GitHub の作業手順のスクリプトの中身を claude-hooks のサブコマンドに移し、スクリプトのファイルはそれを呼ぶだけにする

## 背景

Git と GitHub の作業手順のスクリプトを、#101 の段階 5 で Rust に移す（ADR-0082）。対象は `.claude/scripts/` の
`worktree-add.sh`、`post-merge-cleanup.sh`、`gh-wait-review.sh`、`gh-review-requests.sh`、`review-level.sh`、
`git-merge-state.sh`、`base-moved.sh`、`branch-topics.sh`、`rename-*.sh`、`check-each-commit.sh`、`git-info.sh`、
`gh-actions-diagnose.sh` と `lib/` の共通の部品、`skills/gh-pr-review/scripts/*.sh` で、Slurm のものを除いて 24 本、2,838 行
（`f0083452`、2026-11-13、`wc -l`）。うち 12 本が jq を使い（`grep -l jq`）、Windows の手順は jq を入れている（README の
「Windows で使う」）。`lib/pr-url.sh` と `lib/local-review.sh` は、ほかのスクリプトが source する部品で、単独では実行しない。

これらのスクリプトは、スキルの文から `~/.claude/scripts/<名前>.sh` か `~/.claude/skills/gh-pr-review/scripts/<名前>.sh` のパスで
呼ばれる（`f0083452` で、`review-level.sh` を名前で書く
スキルの文は `.claude/skills` の下の 6 ファイル。`git grep -l`）。claude-hooks の PreToolUse のルールも、スクリプトの名前で呼び出しを
見分ける（例: `require_background_wait.rs` は `gh-wait-review.sh` と `request-rereview.sh` をフォアグラウンドで実行させない）。
拒否の文言も、修正方法としてスクリプトの名前を示す（`pre_merge_check.rs`、`guard_gh_api.rs` など）。`lib/local-review.sh`
（`decide-next.sh`、`request-rereview.sh`、`record-local-review.sh` が source し、前の 2 本が判定の関数を呼ぶ）は、ローカルレビューの記録の判定を
`claude-hooks local-review-covers` に任せ、バイナリを `${CLAUDE_HOOKS_BIN:-$HOME/.claude/bin/claude-hooks}` で引く（PATH に
頼らない。`claude-code.nix` の注記）。Windows では `~/.claude/bin/claude-hooks` が無く、前の 2 本は今も動かない。

crate の分け方は、#101 の「決まっていないこと」で、段階 5 で 3 つ目のまとまりを作るときに決めるとしていた
（ADR-0086 は git hook を `claude-hooks` のサブコマンドにした）。#101 の「作業の順序」は、sh の実装を、呼び出し元を
付け替えた PR で消すとしていた。

段階 6 では、`.claude/skills/language-checks/scripts/` の `run-checks.sh` と `fix-markdown-lint.py` を移す（#101 の
「作業の順序」）。`run-checks.sh` は、段階 5 のスクリプトと同じく、スキルの文から名前とパスで呼ばれる。`fix-markdown-lint.py`
は、markdownlint --fix の直せない MD040（コードの塊の言語）と MD060（表の桁揃え）を直す。呼び出し元は
`python3 ~/.claude/skills/language-checks/scripts/fix-markdown-lint.py` と Python を名指しする
（`references/markdown-checks.md`、`SKILL.md`、`.claude/README.md`。2026-11-14 の `grep -rn fix-markdown-lint`）。PreToolUse の
ルールは名前で見分けない。git hook の `staged-checks`（ADR-0086。`claude-hooks` の中）は、`~/.claude` の下の .py を `python3`
で起動し、どちらかが無ければ自動修正を飛ばす。history-rewrite（ADR-0045）は、.py を Python のモジュールとして読み込み、
`fence_open`、`fence_end`、`is_table_row`、`is_separator_row`、`format_table` を呼んで、書き換えた行を含む表だけを揃え直す
（`history_rewrite.py` の `_realign_tables`）。.py の文字の幅（`display_width`）は、East Asian Width が F と W の文字だけを
2 に、ほかを 1 文字 1 に数える（幅の曖昧な文字を、MD060 が使う string-width と同じく 1 にするため）。

決めること: Rust の中身の置き場、スクリプトのファイルと、スキルの文と hook のルールからの呼ばれ方、テスト。段階 6 の
`fix-markdown-lint.py` の呼ばれ方と、.py の扱いと、文字の幅の数え方。移す順は #101 の「作業の順序」に書く。

## 決定の要因

- スキルの文と PreToolUse のルールは、スクリプトの名前で呼び、見分ける。名前を変えると、両方を同じ PR で揃える
  費用がかかり、揃え漏れは hook の判定の抜けになる
- GitHub の API とローカルレビューの記録の扱いは、すでに `claude-hooks` にある（`gh.rs`、`pre_merge_check.rs`）
- Windows で入れるものを増やさない（ADR-0084 の手順は crate ごとに `cargo install` する）

## 検討した案

置き場:

- 案 1: `claude-hooks` のサブコマンド（`claude-hooks script <名前> [引数]`）にする
- 案 2: 新しい crate（作業手順の CLI）にし、`claude-hooks` と共有する部品はライブラリの crate に出す
- 案 3: 新しい crate にし、共有する部品はコピーして持つ

呼ばれ方:

- 案 A: スクリプトのファイル（`~/.claude/scripts/` と `~/.claude/skills/gh-pr-review/scripts/` の `<名前>.sh`）の名前と場所を変えず、中身を `claude-hooks script <名前> "$@"`
  を起動するだけの sh にする
- 案 B: スキルの文と hook のルールを、新しいコマンドの名前に書き換え、スクリプトのファイルを消す

中断の後片付けを持つ `check-each-commit.sh`（`trap` で一時の worktree を消す。#169）:

- 案 甲: `ctrlc` の crate を足して中断を受け（TERM と HUP も受けるには `termination` の feature が要る）、検査のコマンドが終わった後に片付けて 130 で終える
- 案 乙: `libc`（Unix の sigaction）と `windows-sys`（SetConsoleCtrlHandler）を足して同じことをする
- 案 丙: 中断の後片付けをやめ、次の起動の始めに前の一時の worktree を消す
- 案 丁: 呼ぶ sh が exec せずに `claude-hooks` を起動し、一時のディレクトリを作って自分の `trap` で片付ける
- 案 戊: 移さず、sh のまま残す

段階 6 の `fix-markdown-lint.py` の呼ばれ方:

- 案 C: `.py` の名前と場所を残し、中身を `claude-hooks script fix-markdown-lint` を起動するだけの Python にする
- 案 D: 呼ぶだけの `fix-markdown-lint.sh` を足し、呼び出し元の文書を付け替える

`fix-markdown-lint.py` の扱い:

- 案 ア: 消し、history-rewrite も `claude-hooks script fix-markdown-lint` を子のプロセスで呼ぶ
- 案 イ: スキルの置き場に残し、history-rewrite の段階 7 で消す
- 案 ウ: history-rewrite の隣（`modules/home/parts/history-rewrite/`）に移して残し、history-rewrite を Rust に移す段階 7 で消す

文字の幅:

- 案 い: `unicode-width` の crate（事実上の標準）の幅を使う
- 案 ろ: East Asian Width が F と W の範囲の表（122 の範囲）を、Python の unicodedata から作って持つ

## 決定と理由

採用: 案 1、案 A、案 戊。段階 6 の `fix-markdown-lint.py` は案 D、案 ウ、案 ろ。#101 の「作業の順序」の、sh を呼び出し元の付け替えで消す、は段階 5 のスクリプトには当てはめない。

- 中身: `claude-hooks` に `script` のサブコマンドを足し、スクリプトごとの処理を移す。GitHub の API（`gh` の子プロセス）と
  ローカルレビューの記録の扱いは、今の `claude-hooks` の関数を使う
- スクリプトのファイル: 名前と場所を変えず、`lib/claude-hooks-script.sh` を読んで `claude_hooks_script <名前> "$@"` を
  呼ぶだけの sh にする。`claude_hooks_script` は `"${CLAUDE_HOOKS_BIN:-$HOME/.claude/bin/claude-hooks}" script <名前> "$@"`
  を exec し（`lib/local-review.sh` と同じ引き方。PATH で引かない）、バイナリが無ければ入れ方を示して 127 で終える。
  スキルの文と hook のルールは変えない。Windows では Git Bash がこの sh を実行し、jq を使わない
- Windows の置き場: crate を `cargo install --root ~/.claude` で `~/.claude/bin/` に入れ、Linux と同じパスで引く
  （`claude_hooks_script` は、拡張子の無いパスが無く `.exe` だけがあれば `.exe` のパスを起動する。`.exe` を補って起動
  しない sh でも動くように）。settings.json と git hook の登録もこのパスを指し、`ja-terms` と `commit-refs` は
  `claude-hooks` の隣のものを起動する。スキルと修正方法の文が名前で呼ぶ `claude-setup` と `commit-refs` のため、
  `~/.claude/bin` を Git Bash の `~/.bashrc` で PATH の先頭に足す（README の「Windows で使う」）
- `lib/` の部品: source されるので、呼ぶだけの sh にしない。処理は `claude-hooks` の共通の関数に移し、source するスクリプトが
  すべて移った PR で消す（`lib/claude-hooks-script.sh` は移したスクリプトが読むので残す）
- `check-each-commit.sh` は移さず、sh のまま残す（案 戊。#169）。jq を使わず、Windows では、呼ぶだけの sh と同じく Git Bash が
  実行する（Windows の CI はこのスクリプトを流していない）。
  中断されたとき（Ctrl-C、TERM、HUP）に一時の worktree を消す `trap` は、Rust の標準ライブラリに手段が無く、移すには
  シグナルを受ける crate（`ctrlc`、または `libc` と `windows-sys`）か、OS の関数（sigaction、SetConsoleCtrlHandler）を直に呼ぶ
  unsafe のコードが要る
- PR の単位: スクリプトごとに 1 つ以上の PR にし、中身を移した PR で sh の実装を、上の呼ぶだけの sh に置き換える
- テスト: 判定は `claude-hooks` の結合テスト（gh は偽のコマンドで置き換える。仕組みは最初に移す PR でテストとともに
  決める）で、`nix flake check` と Windows の CI（`windows.yml`）で流す。今の `.claude/tests/scripts.sh` は、
  呼ぶだけのスクリプトのファイルから呼ぶ配備の確認として残し、`CLAUDE_HOOKS_BIN` にブランチでビルドしたバイナリを渡して流す
  （sh のまま残す `check-each-commit.sh` は、そこで振る舞いを確かめる）
- 段階 6 の `run-checks.sh` は、名前とパスで呼ばれるので、段階 5 のスクリプトと同じ形（中身を移し、呼ぶだけの sh にする）にする
- 段階 6 の `fix-markdown-lint.py`（案 D、案 ウ、案 ろ）: 中身を `claude-hooks script fix-markdown-lint` に移し、振る舞い（表の
  桁揃え、言語の推定、CommonMark のフェンスの判定、字下げの保ち方、`Fixed:` と `OK:` の出力）を .py と同じにする。
  `lib/claude-hooks-script.sh` を読んで `claude_hooks_script fix-markdown-lint "$@"` を呼ぶだけの
  `skills/language-checks/scripts/fix-markdown-lint.sh` を足し、呼び出し元の文書をそれに付け替える。git hook の
  `staged-checks` は、同じバイナリの中の関数を直に呼ぶ（子のプロセスにせず、`python3` もスクリプトの置き場も要らない）。
  文字の幅は、.py と同じく East Asian Width の F と W だけを 2 に数える表を持ち、空白は Python の `str.strip` と同じ文字
  （Unicode の White_Space と \x1c〜\x1f）を外す
- 二重の実装の期間: .py は、段階 7 で history-rewrite を Rust に移す PR で消し、history-rewrite は Rust の関数を呼ぶ。それまで、
  `.claude/tests/scripts/language-checks.sh` が、サロゲートと改行と `|` のほかのすべての文字で、Rust の版と .py の表の桁を
  突き合わせる。表の段の振る舞いを変えるときは両方を直す（#46、#99）

理由:

- 案 戊: `check-each-commit.sh` を移しても、jq を外すことにも、Git Bash を要らなくすることにもならない（jq は使わず、
  呼ぶだけの sh も Git Bash が実行する）。案 甲と案 乙は、得るもののために依存を足す。案 丙は、同時に走る別の起動の
  worktree を消さないための名前の決め方が要り、今の `trap` より複雑になる。案 丁は、片付けを sh に残したまま中身を
  分けるので、sh と Rust の両方を読むことになる
- 案 1 は、GitHub の API とローカルレビューの記録の扱いをそのまま使え、Windows で入れるものも増えない。案 2 は、
  ライブラリの crate を切り出す作業がかかり、得るのはバイナリの名前の分け方だけ（ADR-0086 の案 2 と同じ）。案 3 は、
  記録の判定が 2 つの crate に分かれ、`local-review-covers` で 1 つにした判定（`decide-next.sh` と pre-merge-check）が
  また食い違いうる
- 案 A は、スキルの文と hook のルールを変えないので、移す PR ごとの変更がスクリプト 1 つに収まる。案 B は、移すたびに
  スキルの文と hook のルールを揃える PR になり、揃え漏れが hook の判定の抜けになる。sh を 1 つ挟む費用は呼び出しごとに
  1 回で、これらのスクリプトは作業の節目に呼ぶもの（PreToolUse hook のように毎回は走らない）
- 案 D: 呼び出し元は文書の 3 か所だけで、PreToolUse のルールは名前で見分けないので、付け替えの費用は小さい。案 C は、
  Windows で呼ぶのに `python3` が要るままで、移す目的（Python を外す）にならない
- 案 ウ: history-rewrite は、ファイルの全体でなく、書き換えた行を含む表だけを揃え直すので、ファイルを受ける CLI を呼ぶ
  案 ア では置き換えられず、表を渡すモードを CLI に足すことになる。そのモードは、段階 7 で history-rewrite を Rust に
  すれば関数を呼べるので要らなくなる。案 イ は、配られた `~/.claude` の下に古い入口が残り、呼ぶ側がそちらを選びうる。
  案 ウ は、history-rewrite の `package.nix` の参照を隣のファイルにするだけで済み、.py の使い手が 1 つだと置き場で分かる
- 案 ろ: `unicode-width`（0.2.3）は、結合文字、ゼロ幅の文字、ハングルの中声などを 0 に数え、絵文字の列をまとめて 2 に
  数えるので、.py と 7,479 の符号位置で幅が違う（2026-11-14、すべての符号位置で測った。うち 3,845 は未割り当ての文字、
  2,012 は結合文字）。.py と同じ桁にするには例外の規則が要り、crate を使う意味が残らない。表は、作り方を表の上に書き、
  Unicode の版を上げるときに作り直す（.py の幅も、動かす Python の unicodedata の版で変わる）
- 言語の推定の正規表現は、Rust の `\w`、`\s`、`\b`（Unicode の定義）のままで、Python と非 ASCII の一部の文字で食い違う。
  推定が外れても付く言語が変わるだけで、文書は壊れないので合わせない
- バイナリを PATH でなく固定のパスと `CLAUDE_HOOKS_BIN` で引くのは、今の `lib/local-review.sh` と揃え、テストで
  ブランチのバイナリに差し替えられるようにするため。Windows も同じパスに置けば、スクリプトからの引き方が
  Linux と 1 つで済み、今の `lib/local-review.sh` も Windows で動く

### 帰結

- 良い点: 移した後のスクリプトは jq を使わない。Windows の手順から jq を外せるのは、段階 8 まで移し終えた後
- 悪い点: `claude-hooks` という名前のバイナリが、hook でない作業手順の処理も持つ。名前は段階 8 の後に見直す
- 悪い点: スクリプトの判定を変えるたびに、`claude-hooks` のビルドとテストが要る（ADR-0082 の帰結と同じ）
- 悪い点: `claude-sync` は呼ぶだけのスクリプトをすぐ配るが、バイナリは rebuild まで前の版のままで、その間、移した
  スクリプトは前の版のバイナリにサブコマンドが無く失敗する。移す PR は `claude-hooks` を変えるので、マージの後は
  `claude-sync` でなく rebuild（activation が sync も実行する）で入れる。Windows も、`claude-setup sync` の前に
  `cargo install` を済ませる
- 悪い点: Windows では、`cargo install --root ~/.claude` が入れた crate の記録（`.crates.toml`、`.crates2.json`）を
  `~/.claude` に置く
- 悪い点: 段階 7 まで、表の桁揃えの実装が Rust と Python の 2 つになる。照合のテストは Linux の sh のテストだけで流れる
- 悪い点: fix-markdown-lint の幅は、.py と同じく、結合文字、ゼロ幅の文字、制御文字、絵文字の列で MD060（string-width）と
  食い違う（#182）

### 確認

- 移したスクリプトの結合テストが、`nix flake check` と Windows の CI で通る。期待値は、今の `.claude/tests/scripts.sh` が確かめて
  いるもの。sh のまま残す `check-each-commit.sh` は、`.claude/tests/scripts.sh` の振る舞いのテストで確かめ続ける
- `claude-hooks script fix-markdown-lint` の結合テスト（`tests/script_fix_markdown_lint.rs`）が .py の出力を期待値にして通り、
  `.claude/tests/scripts/language-checks.sh` の照合が、すべての文字で .py と同じ桁になる。`staged-checks` は
  `python3` の無い環境で表と言語を直す（`tests/git_hook_staged_checks.rs`）
- 移した後、対象のスクリプトのファイル（`check-each-commit.sh` を除く）は `lib/claude-hooks-script.sh` を読んで `claude_hooks_script <名前>` を呼ぶだけに
  なり、jq を使わない。`lib/` の部品は `claude-hooks-script.sh` のほかは消えている

## 補足

- 2026-11-13: Windows の置き場は、はじめ `claude-setup` が `cargo install` の置き場（`~/.cargo/bin`）の `claude-hooks.exe` を
  `~/.claude/bin/` にコピーするとしていた。settings.json と git hook の登録は `~/.cargo/bin` を指したままなので、コピーを
  忘れると hook とスクリプトで版が分かれる（PR #142 のローカルレビュー）。`cargo install --root ~/.claude` で入れれば
  置き場が 1 つになり、コピーの手順も要らないので、そちらに変えた。`~/.claude/bin` は PATH に無いので、名前で呼ぶ
  `claude-setup` と `commit-refs` のために PATH の先頭に足し、前の `~/.cargo/bin` の版は外す。PATH は、はじめ PowerShell で
  User の Path に足したが、User の Path の展開する項目（`%USERPROFILE%\...`）を固定のパスに潰し、System の Path の後に
  並ぶので先頭にならず、`$HOME` と `USERPROFILE` が違えば置き場とずれる（PR のローカルレビュー）ので、Git Bash の
  `~/.bashrc` に変えた
- 2026-11-14: スクリプトのファイルは、はじめ `exec "${CLAUDE_HOOKS_BIN:-$HOME/.claude/bin/claude-hooks}" script <名前> "$@"`
  だけの sh としていた。バイナリが無いと sh の `exec: not found` だけで止まり、何を入れるかが分からない（claude-sync は
  スクリプトだけを入れる）。decide-next.sh は `local_review_covers` で入れ方を示していたが、呼ぶスクリプト
  （get-review-comments.sh、get-latest-review.sh）を移すと、その前で案内なしに止まった（`.claude/tests/scripts/gh-pr-review.sh`
  の「claude-hooks が無ければ、入れ直しを案内して止める」が落ちた）。decide-next.sh の確認の順を入れ替えても、次に移す
  スクリプトがまたその前に来るので、入れ方を示すのを sh の共通の部品（`lib/claude-hooks-script.sh`）に移した
- 2026-11-14: `claude_hooks_script` は、はじめ `.exe` の付いたパスも在るものとして数え、起動は拡張子の無いパスのまま exec
  していた。`.exe` だけがあり、`.exe` を補って起動しない sh では、在ると数えた後に `not found` で止まる（PR #148 の Copilot
  のレビュー。その版のテストは入れ方の文が出ないことしか見ず、起動の失敗でも通っていた）ので、`.exe` だけがあれば
  そのパスを起動するように変え、テストは起動して 0 で終えることを確かめる
- 2026-11-14: 案内に書くスクリプトのパス（gh-wait-review の「次の一手」の request-rereview.sh など）は、sh の版では
  自分の場所から辿り、デプロイ先の古い版を案内しないようにしていた。Rust に移すと自分の場所が分からないので、
  `lib/claude-hooks-script.sh` が呼んだ sh の場所を `CLAUDE_HOOKS_SCRIPT_DIR` で渡し、`script::claude_root` がそこから
  `<root>`（`~/.claude` か、リポジトリの `.claude`）を引く（渡されなければ `~/.claude`）。Windows の Git Bash の `pwd` は
  `/c/...` の形でネイティブの exe が読めないので、`pwd -W` の形で渡す
- 2026-11-14: worktree-add の .worktreelink の symlink は、sh の版では `ln -s` で作っていた。Git Bash の `ln -s` は既定で
  コピーを作るので、Windows では共有にならなかった。Rust に移した版は、Windows では `std::os::windows::fs` の
  symlink_dir / symlink_file で作る。開発者モードか管理者の権限が無ければ作れないので、作った symlink を消して、その旨を
  示して止める
