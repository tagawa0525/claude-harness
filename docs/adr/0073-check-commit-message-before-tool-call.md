---
status: accepted
date: 2026-11-06
requires: [ADR-0033, ADR-0050]
supersedes: []
superseded-by:
issues: ["#591", "#445", "#489", "#619", "#620"]
---

# ADR-0073: git commit の本文を、ツール呼び出しの前に commit-msg と同じスクリプトで検査する

## 背景

git の commit-msg（`modules/home/parts/git.nix`）は、Claude Code のセッションのコミットのうち、件名が Conventional Commits で
ないもの、72 文字を超えるもの、本文に崩れた日本語の語（ADR-0014）があるものを拒む。git は commit-msg を pre-commit の後に
呼ぶので、拒む前に pre-commit の検査が一度走る（マージを終える `git commit` では、pre-commit が pre-merge-commit の
品質チェックに回し、数分かかる。#445）。

手戻りは pre-commit の時間だけではない。`git commit -q … <<'EOF' … EOF` の後に改行か `;` で `git log -1` や
run-checks.sh をつなぐと、commit が拒まれてもツールの終了コードは 0 になり、後段が古い HEAD のまま走る。2026-11-06 00:18 JST
（xlc の docs/m4-next-order）に、拒まれた commit の後で run-checks.sh が古い HEAD b4a62c5fb で `ALL_OK` を出した。ADR-0069 の
背景の 2026-10-30 の事故（拒まれた commit の後の amend が main のマージコミットを書き換えた）も同じ入口である。xlc の会話の
記録で、commit-msg の「件名が 72 文字を超えています」を含むツールの結果は、2026-10-21〜11-05 に 242 件ある（#591 の数え方）。

ADR-0050 と同じく、git の hook の順は変えられず、pre-commit からは `-m` の本文が読めない。ADR-0050 は `git merge -m/-F`
の本文の語だけを、ツール呼び出しの前に commit-msg と同じ ja-terms-message で検査し、マージを終える `git commit`（#445）と、
同じ呼び出しの前段が書く `-F` のファイル（#489）を残していた。

ユーザーは、ツール呼び出しの前に `git commit -m/-F` の件名を検査する hook を作ると決めた（ユーザー
2026-11-06）。

## 検討した案

- 案 A: commit-msg の規則（型の一覧、72 文字、Merge / fixup! / squash! / Revert を形式の対象外にする、語の検査）を
  hook の本体から 1 つのスクリプト（commit-msg-check）に切り出し、commit-msg の hook と claude-hooks の新しいルールが同じ
  store のパスで呼ぶ。ルールは commit-msg に渡るのと同じメッセージを一時ファイルに書いて渡し、終了コードで判定する。
  検査するかどうかの判定（CLAUDECODE、upstream のある fork、プロジェクトローカルの commit-msg）は hook に残し、ルールは
  ADR-0050 の判定（`commit_msg_skips`）を使う
- 案 B: 規則を Rust で書き直し、ルールが件名を自分で判定する。規則が commit-msg と Rust の 2 か所になり、片方を変えると
  黙って食い違う（通したコミットが commit-msg に拒まれるか、通るコミットを止める）。ADR-0050 の最初の版も語の範囲を Rust で
  書き直し、ローカルのレビューで同じ理由から範囲ごとスクリプトに移した
- 案 C: commit-msg の hook のファイルそのものを、ルールが一時ファイルを渡して呼ぶ。hook はプロジェクトローカルの commit-msg に
  `exec` で委ね、CLAUDECODE と fork も自分で見るので、ルールがそのままでは呼べない（ローカルの hook の副作用を走らせうる）。
  textlint の失敗と規約の違反が同じ終了コード 1 で、ルールが「判定できない」（ADR-0033）と「止める」を分けられない
- #445 は、`git commit` のメッセージを検査するこのルールが MERGE_HEAD の有無によらず扱う（commit-msg の検査を丸ごと
  呼ぶので、語も件名も 1 度に見る）。別に MERGE_HEAD のときだけ語を見るルールを足すと、同じ本文の取り出しが 2 つになる
- #489 の扱い:
  - 案 D: 同じ呼び出しの前段が `-F` のファイルを引数に取るかリダイレクトの先にするとき、書いた後の中身を実行の前に
    読めないので、判定できないとして止め、呼び出しを分けるか本文を直接渡すよう示す。`git merge` のルールも同じにする
  - 案 E: 判定できないとして notice で通し、commit-msg に任せる（ADR-0050 の「読めないときは通す」と同じ向き）。#489 の
    3 周目の指摘で挙がった案で、ゲートを緩める。通すと、commit-msg が後で拒んだときに後段が古い HEAD のまま走るという
    このルールの目的を果たさない
  - 案 F: 今の `git merge` の案内（前段の引数に -F の値と同じ文字列があるときだけ、拒んだ文言に「呼び出しを分ける」を
    足す。#488）を `git commit` にも使う。リダイレクトで書く形（`cat > msg.txt <<'EOF' … && git commit -F msg.txt`）は
    案内が出ず、前の試行で残ったファイルの中身で判定して通しうる

## 決定と理由

案 A と案 D を採る。

- 規則は commit-msg-check（`modules/home/parts/ja-terms/commit-msg-check.sh`。`textlint.nix` の `commitMsgCheck`）の
  1 か所に置く。commit-msg の hook は、ローカルの commit-msg、CLAUDECODE、upstream のある fork を見た後に、これを
  `exec` する。終了コードは 0 が通す、1 が規約に反する、2 が語の検査（textlint）の失敗。git は 0 でないどの値でも
  コミットを止めるので、hook の振る舞いは変わらない。ルールは 1 で止め、2 と起動の失敗は判定できなかった理由を出して
  通す（ADR-0033）
- ルール（check-commit-message）は、commit-msg に渡るのと同じメッセージを組み立てる（builtin/commit.c）。`-m` は値の後に
  改行を足し、2 つ目からは前に空行を挟む。`--no-message` はそれまでの `-m` を消す。`-F -`（と `/dev/stdin`、
  `/dev/fd/0`、`/proc/self/fd/0`）は標準入力で、同じコマンドのヒアドキュメント（`git commit -F - <<'EOF'`）から本文を
  取る（hook のプロセスの標準入力は hook の入力なので読まない）。ほかの `-F` はコマンドの行き先からの相対のファイル。git は
  commit-msg の前に、`--cleanup=verbatim` でなければ行末の空白と先頭と末尾の空行を除き、続く空行を 1 つにする
  （`#` の行は `--cleanup=strip` でも commit-msg の後で除かれ、commit-msg には残る）。書いたまま commit-msg に渡す `git merge`（ADR-0050）と
  違う。git 2.56 で、ローカルの commit-msg に渡ったファイルを見て確かめた（2026-11-06）
- `-m` の値の読み方（単引用符、`$(cat <<'EOF' … EOF)` のヒアドキュメント、シェルが展開する値は判定できない）は ADR-0050 の
  ルールと同じ関数を使う。`-F -` のヒアドキュメントは、区切りをクォートしていなければ、`$` かバッククォートを含むときに
  判定できないとする
- 対象外は、commit-msg を呼ばない形（`-n` / `--no-verify`。後ろの `--verify` が打ち消す。`--dry-run`）、ほかのコミットから
  メッセージを作る形（`-c` / `-C`、`--fixup`、`--squash`）、git が止める形（`-m` と `-F` の両方）、本文の無い形（エディタ）と、
  commit-msg が検査しない所（リポジトリの外、グローバルの core.hooksPath を使わないリポジトリ、upstream のある fork、
  ローカルの commit-msg に置き換わるリポジトリ）
- 前段が `-F` のファイルを書きうる（そのコマンドの行き先からの相対で同じパスを、引数に取るか出力のリダイレクトの先に
  する）とき、`git commit` と `git merge` のルールはファイルを読まずに止める（案 D）。止めると前段も実行されないことと、
  呼び出しを分けるか、本文をヒアドキュメントで直接渡す方法を文言に示す。案 E は目的（後段が古い HEAD で走る前に止める）を
  果たさず、案 F は書いた後の中身を判定できない。読むだけの前段（`cat msg.txt`、`test -f msg.txt`）も止めるが、本文を
  直接渡せば通る
- 前段の引数とリダイレクトの先の `$NAME` と `${NAME}` は、同じ呼び出しの代入で値が字面のまま 1 つに決まれば、展開してから
  比べる（`S=<dir>; cat > $S/msg.txt <<'EOF' … EOF` の後の `git commit -F <dir>/msg.txt`）。単引用符の中の `$` は字面のまま。
  値が決まらない展開（同じ呼び出しに代入が無い、値が展開を含むか同じ名前に 2 つの値がある、`$(…)`、`${NAME:-x}`、`$1`）を
  含むものは、最後の決まらない展開より後ろの字面が `/` で始まり、`-F` のパスがその後ろのパスの要素で終われば、書きうる
  ものとして止める（`cat > $TMP/msg.txt` と `git commit -F /tmp/x/msg.txt`）。後ろに字面の無いもの（`git add $FILES`、
  `echo "$T"`）と、字面がパスの要素の途中から始まるもの（`cp notes $VERSION.txt` の `.txt`）は数えない。値が決まらないときに止める側へ倒すのは案 D と同じ向きで、決まらないものを全部止めずに後ろの字面で絞るのは、
  ADR-0025 が実行するものの決まらないコマンドを、文字列に対象の語があるときだけ止めるのと同じ理由（`git add $FILES` の
  ような、-F のファイルと関係の無い前段で止める偽陽性を増やさない）
- 拒否の文言には、commit-msg-check の出力（❌ の行）と、先に止める理由（このまま実行すると pre-commit の後で commit-msg が
  拒み、同じコマンドの後段が古い HEAD のまま走る。その例）と、修正方法（❌ の行のとおりに直す）を書く。PC 全体で読む文言
  なので、ADR と xlc の番号は書かない
- エスケープは設けない。メッセージを直せば通る
- tree-sitter-bash の木は、並び（`a && b <<EOF`）の後ろのヒアドキュメントとリダイレクトを並びに付け、ヒアドキュメントの
  演算子の後ろのリダイレクト（`cat <<'EOF' > msg.txt`）をヒアドキュメントの中に置き、`<<` かその前のリダイレクトの直前の
  1 語の `-` を落とし、`<<-` の本文の範囲を誤る。コマンドの読み取り（`shell.rs`）で、並びの最後のコマンドからリダイレクトに
  たどり（ヒアドキュメントの中のリダイレクトも含める）、`-` を引数に戻し、本文は演算子の行の次の行から区切りの行までを原文から切り出す（同じ行に前のヒアドキュメントが
  あれば決めない）

### 帰結

- `git commit -m/-F` の件名と語の誤りは、pre-commit の前に止まり、同じ呼び出しの後段は走らない
- commit-msg の規則を変えると、commit-msg と claude-hooks の両方が同じスクリプトを使う。claude-hooks のパッケージは
  makeBinaryWrapper で `CLAUDE_HOOKS_COMMIT_MSG_CHECK` に store のパスを固定する（ADR-0050 の ja-terms-message と同じ）。
  cargo で直接ビルドしたバイナリ（`.claude/tests/hooks.sh` の既定）には無く、判定できない理由を出して通す
- `git commit` のたびに commit-msg-check（textlint を含む）を 1 度呼ぶ。ADR-0050 の測定では ja-terms-message が 0.3〜0.5 秒
- `-F -` に同じコマンドのヒアドキュメントが無い形（`printf … | git commit -F -`）、`<<<`、`< file` は判定できない理由を出して
  通し、commit-msg に任せる
- `git merge` のルールは、前段が `-F` のファイルを書きうる形で、書く前の中身の語を示す代わりに、判定できないとして止める。
  PR #488 の案内の文言（前段の引数に同じ文字列があるときだけ「呼び出しを分ける」を足す）はこれに置き換わる
- `commit_msg_skips` は、リポジトリの外で `git config --get core.hooksPath` がグローバルの値を返すため、リポジトリの外を
  先に除く。`git merge` のルールも、リポジトリの外では検査しない
- 前段のパスの展開は、代入の位置とスコープを見ない（後ろの代入、サブシェルや `bash -c` の文字列の中の代入、
  `S=x cmd` の前置きの代入も数える）。実際の値と食い違えば、止める側にも止めない側にも誤りうる（例:
  `(S=/a); cat > $S/msg.txt; git commit -F /msg.txt` は、実際には `$S` が空で `/msg.txt` を書くが、`/a/msg.txt` と読んで
  止めない）。
  引用符の外の値の単語分割とグロブも見ない。`-F` の値そのものに `$` があるもの（`git commit -F $S/msg.txt`）は、今も
  展開せず判定できない理由を出して通す（#620）

### 確認

- メッセージの組み立て、対象外の形、判定できない理由、前段が書くファイルの扱いは、cargo のユニットテスト
  （`src/rules/check_commit_message.rs`、`check_merge_body_ja_terms.rs`）が偽の検査で確かめる。ヒアドキュメントと
  リダイレクトの読み取りは `src/shell.rs` のテストが確かめる。前段のパスの変数の展開と、決まらない展開の後ろの字面での
  比較は `src/shell.rs` の `writer_before_*` のテストと、両ルールの前段が書くファイルのテストが確かめる
- 包んだ commit-msg-check で、xlc の記録にある 73 文字の件名（`-F -` のヒアドキュメントの形と `-m "$(cat <<'EOF' … EOF)"`
  の形）と Conventional Commits でない件名を止め、72 文字の件名と、Merge / fixup! / squash! / Revert の長い件名を通すことは、
  package.nix の installCheckPhase（`tests/commit-message.sh`）が確かめる（`nix build .#claude-hooks`、flake の checks）
- commit-msg の判定が切り出しの前後で変わらないことは、`modules/home/parts/tests/commit-msg-conventions.sh`（72 文字の境界、
  対象外の件名を含む）、`ja-terms.sh`、`local-hooks-worktree.sh` で確かめる

## 変更

- 2026-11-06: 決定と理由に、前段の引数とリダイレクトの先の変数を展開してから比べる項を足した。前は「前段が `-F` の
  ファイルを書きうる（そのコマンドの行き先からの相対で同じパスを、引数に取るか出力のリダイレクトの先にする）とき」の
  「同じパス」を、書いたままの字面で比べていた。xlc のセッションの `S=<dir>; cat > $S/msg.txt <<'EOF' … ; git commit -F
  <dir>/msg.txt` と `sed -i … $S/msg.txt; git commit -F <dir>/msg.txt` を止めず、commit-msg が後で拒んだ（#619）
