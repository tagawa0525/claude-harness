---
type: ADR
status: accepted
date: 2026-11-29
requires: [ADR-0025, ADR-0042, ADR-0109]
supersedes: []
superseded-by:
issues: ["#428", "#441"]
description: claude-hooks の PreToolUse のルール block-secret-read が、設定の permissions.deny の Read の規則（~/ か // で始まるもの）を秘密の置き場の一覧として実行時に読み、Bash のコマンドの引数と入力のリダイレクトのパスを照らして拒む。一覧は静的な deny だけに持つ。照らすのはモデルが普通に書く形だけで、置き場の名前を書くだけで中身を出さないコマンドは通す
---

# ADR-0110: 秘密の置き場の Bash での読み取りを、設定の Read の deny を一覧として hook で照らして拒む

## 背景

ADR-0109 で、Read、Grep、Glob をどのパスでも確認なしにし、分かっている秘密の置き場（`~/.ssh` など）の Read を、
配る設定（`claude-settings.json`）の `permissions.deny` で拒んだ。配る設定は `Bash(cat:*)`、`Bash(grep:*)`、
`Bash(find:*)`、`Bash(xargs:*)`、`Bash(curl:*)` を確認なしにしていて、Bash で置き場を読むと、中身が人の確認を経ずに
会話と外へ送るコマンドに渡りうる（#428）。

2026-11-29 に i7（Windows、Git Bash）で、今の deny が Bash のどの書き方に効くかを確かめた（#428 のコメント。中身は
出さず件数だけを見た）。`ls ~/.ssh/`、`ls "$HOME/.ssh"`、`cd ~ && ls .ssh` は deny で拒まれ、`ls "$(echo ~)/.ssh"` と
`echo ~/.ssh | xargs ls` は通った。Claude Code の権限判定は、変数と cd の後の相対のパスまでは解くが、`$(...)` の置換と
xargs が標準入力から足すパスを見ず、`bash -c` の中に再帰しない（ADR-0025 の背景の公開ソースの調査と同じ）。

hook の範囲は ADR-0042 が決めている: 普通に書く形で守らせ、作為的な抜け道は塞がない。hook は境界ではない。前の担当が
「境界として塞ぐ」方向で ADR-0042 と食い違って止まったのを受け、ユーザーは ADR-0042 に合わせ、普通に書く形だけを照らし、
読めないシェルの形は止める側に倒さず、hook を外への持ち出しの境界として扱わないと決めた（ユーザー 2026-11-29 の判断の
要約）。境界としての防ぎ方（`Bash(curl:*)` の確認なしの許可、sandbox）は #441 に分けた。

## 決定の要因

- 秘密の置き場の一覧を 1 か所に持つ（#428 の案）。Windows の置き場（`~/AppData/Roaming/` の下）も同じ一覧で扱う
- Grep と Glob の結果から置き場を除けるのは、Claude Code の静的な deny だけ（hook は呼び出しの入力しか見ず、`~` を
  起点にした Grep の結果を絞れない）
- ADR-0042 の「止める側の誤判定は直す」: 置き場の名前を書くだけで中身を出さない普通のコマンドを止めない
- 拒否の文言は PC 全体に配る hook のもので、なぜ拒むかをその場に書く

## 検討した案

一覧の置き場:

- 案 A: 一覧を静的な deny（`permissions.deny` の `Read(...)`）だけに持ち、hook が実行時に設定の場所（`CLAUDE_CONFIG_DIR`、
  無ければ `~/.claude`）の `settings.json` を読んで、同じ規則で Bash を照らす
- 案 B: 一覧を claude-hooks（Rust）に持ち、Read、Grep、Glob も hook で照らし、静的な deny を消す。hook は Grep と Glob の
  結果から置き場の中身を除けないので、`~` を起点にした Grep が `~/.ssh` の中身を返す（ADR-0109 の確認の 3 つ目が崩れる）
- 案 C: 一覧を claude-hooks と静的な deny の両方に持ち、食い違いをテストで見つける。claude-hooks の nix のビルドの
  ソース（`crates/claude-hooks/package.nix` の fileset。根は `crates/`）に `claude-settings.json` が入らず、crate の
  テストから読めない。根を広げると、設定を変えるたびに claude-hooks を作り直してテストが走る
- 案 D: 一覧を claude-hooks に持ち、`claude-setup` が静的な deny を作る。配る仕組みを変えることになり、#428 の範囲を超える

止める範囲（置き場の名前を書くだけのコマンドをどうするか）:

- 案 E: すべてのコマンドの引数を照らす。`ssh -i ~/.ssh/id_ed25519 host`、`ssh-keygen -f ~/.ssh/id_new`、
  `ssh-add`、`mkdir -p ~/.ssh`、`chmod 700 ~/.ssh`、`test -f ~/.ssh/id_ed25519` のような、鍵を使う・作る・置き場を
  用意するだけのコマンドと、`echo .env >> .gitignore`、`git rm --cached .env`、`grep -rn .env docs/`、
  `find . -name .env` のような、名前を語や式として書くだけのコマンドを止める
- 案 F: 案 E から、置き場の名前を書くだけで中身を出さないプログラム（`ssh`、`ssh-add`、`ssh-keygen`、`mkdir`、`chmod`、
  `test`、`[`、`git`）と、名前を出すだけの `echo` と `printf`（xargs に渡すときは照らす）、`grep` と `rg` の探す語、
  `find` の式を除く
- 案 G: 読むプログラムの表（`cat`、`head`、`grep`、`find` など）の引数だけを照らす。表に無い読み方（`base64`、`tar`、
  `curl -T`、`xxd`）を見落とし、`echo … | xargs ls` も別に扱う必要がある

誤検出の比べ方: i7 のセッションの記録（`~/.claude/projects/` の下の 4 プロジェクト、2026-11-29 に集めた Bash の呼び出し
9110 件）のうち、置き場の語（`.ssh`、`.aws`、`.env`、`gnupg`、`credentials`、`netrc` など）を含む 40 件を、案 F を実装した
ルールに通した。止めたのは 13 件で、すべて `~/.ssh` の下（`config`、`known_hosts`、`*.pub`、一覧）を読むものだった
（ADR-0109 の deny も、置き場の全体を Read で拒む）。残りの 27 件に、案 E だけが止める形（置き場のパスを引数に取る
`ssh`、`ssh-keygen`、`mkdir`、`echo` など）は無く、記録の数では案 E と案 F は分かれなかった。そこで、上の例の形
（新しい機械に鍵を作って置く、`.env` を `.gitignore` に足す、`.env` を探す）がモデルの普通に書く形で、案 E がそれを止める
ことを、比べる軸にした。

## 決定と理由

採用: 一覧は「案 A」、止める範囲は「案 F」。

- PreToolUse のルール `block-secret-read`（Bash だけ）が、設定の `permissions.deny` のうち `~/` か `//` で始まる
  `Read(...)` の規則を、Bash の呼び出しのたびに読む。規則の書き方は Claude Code のもの（`**` は 0 個以上の要素、`*` は
  要素の中の 0 文字以上）で、末尾の `/**` は置き場のディレクトリそのものも含む（Claude Code も `ls ~/.ssh/` を拒む）。
  ほかの書き方（`./x`、`/x`）は見ない
- 照らすのは、コマンドの引数（ADR-0025 のとおりラッパーをほどき、`bash -c` などの文字列の中と `$(...)` の中の
  コマンドも含む）と、入力のリダイレクト（`<`）の読み元。パスは、同じ呼び出しで字面のまま決まる変数と、hook の環境の
  `HOME`、`USERPROFILE`、`APPDATA`、`LOCALAPPDATA` で展開し、前の `cd` と `env -C` の先からの相対で解く。値の決まらない
  展開（`"$(echo ~)/.ssh"`）は任意のディレクトリとみなし、後ろの字面で照らす。Windows では大文字と小文字を区別せず、
  Git Bash のドライブのパス（`/c/x`）を `C:/x` と読む
- 置き場の名前を書くだけで中身を出さないコマンドは照らさない（案 F の除外）。ssh と `sudo -i` の先のコマンドは
  照らさない（接続先や別の利用者のファイルを読む）
- エスケープは置かない。静的な deny と同じく、中身が要るときはユーザーが手で渡す
- 設定のファイルが無ければ何も止めない。読めないか JSON でなければ、止めずに理由を利用者に見せる（`systemMessage`）。
  hook は境界ではなく（ADR-0042）、Bash をすべて止めると設定を直す手段も止まる

案 B は静的な deny の役目（Grep と Glob の結果を絞る）を失う。案 C は nix のビルドの形を変えるか、一覧の食い違いを
見つけられない。案 D は配る仕組みの変更で、範囲が大きい。案 A なら、ADR-0109 の帰結の「一覧に足す」がそのまま Bash に
効き、Windows の置き場も同じ行で扱う。

案 E は、上の普通の形を止める誤判定で、ADR-0042 が直すとするもの。案 G は、照らす形がプログラムの表の広さで決まり、
表に無い読み方を見落とす。案 F の除外は、名前を書くだけで中身を出さないことが分かっているものだけにした。

### 帰結

- 普通に書く形（`cat`、`grep`、`find`、xargs、変数、`$(...)`、cd の後の相対のパス、`bash -c` の中）で置き場を読む
  コマンドは、Claude Code の権限判定が見ない形も hook が拒む
- 作為的な書き方（パスを組み立てる python や perl の本文、`eval` の文字列を変数から作る、symlink を通す）は照らさない。
  置き場の親を再帰で読む形（`grep -r x ~`）も照らさない。ADR-0042 のとおり、その穴はモデルの判断と人のレビューに
  任せ、外への持ち出しの境界としての防ぎ方は #441 で扱う
- ADR-0109 の帰結の「deny は Read の規則なので、Bash の `cat ~/.ssh/id_ed25519` は塞がない」は、この ADR で塞ぐ
- hook が読むのは利用者の設定（`settings.json`）だけで、プロジェクトの `.claude/settings.json` と `settings.local.json` の
  deny は照らさない。配る一覧は利用者の設定に入る（`claude-setup settings --static`）
- 一覧の規則は `~/` か `//` で書く。ほかの書き方で足した規則は Read だけに効き、Bash には効かない
- Bash では、Read の deny と違って大文字と小文字を変えた書き方（Windows）も拒む
- `.env.example` も Bash で読めない（ADR-0109 の帰結と同じ）

### 確認

- `crates/claude-hooks/src/rules/block_secret_read.rs` の単体のテストが、普通に書く形で置き場を読むコマンドを拒み、
  置き場の外と、名前を書くだけのコマンドを通すこと、Windows の置き場と `$APPDATA` の形、Windows の大文字と小文字と
  Git Bash のパスを確かめる
- `crates/claude-hooks/tests/rule_block_secret_read.rs` が、`CLAUDE_CONFIG_DIR` の `settings.json` から一覧を読むこと、
  設定が無ければ止めず、読めなければ止めずに理由を見せることを確かめる
- 配備の後、Linux と Windows のそれぞれで、`ls "$(echo ~)/.ssh" | wc -l` と `echo ~/.ssh | xargs ls | wc -l` が
  block-secret-read で拒まれることを見る
