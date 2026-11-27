---
type: ADR
status: accepted
date: 2026-11-27
requires: [ADR-0084, ADR-0086, ADR-0093, ADR-0094]
supersedes: []
superseded-by:
issues: []
description: git のグローバルの attributes に、すべてのパスの text=auto eol=lf と、拡張子 bat と cmd の eol=crlf を置く。正は config/git/attributes で、Windows は claude-setup git-attributes、Linux は home-manager の programs.git.attributes が置き、利用者のファイルと core.attributesFile は奪わない
---

# ADR-0105: git のグローバルの attributes に、改行を LF にそろえる規則を、Windows は claude-setup、Linux は home-manager の programs.git.attributes で置く

## 背景

`.gitattributes` を持たないリポジトリでは、改行の扱いが機械の設定で決まる。この Windows の PC の git は、system の
gitconfig（`C:/Program Files/Git/etc/gitconfig`）が `core.autocrlf=false` で、global に `core.eol` と `core.attributesFile`
は無い（2026-11-27、`git config --show-origin`）。この設定では、Windows のエディタや Python の `write_text` が CRLF で書いた
ファイルが、CRLF のまま index に入る。NC、lineage-grid、claude-harness は、それぞれの `.gitattributes` に
`* text=auto eol=lf` を書いて防いでいる（2026-11-27）が、新しいリポジトリと他人のリポジトリには無い。

利用者は、改行の既定を CRLF でなく LF にしたいと求め、グローバルの attributes を claude-setup と Nix で配る案に同意した
（ユーザー 2026-11-27 17:07、18:57 JST）。

git は `core.attributesFile` が無いとき、`$XDG_CONFIG_HOME/git/attributes`（`XDG_CONFIG_HOME` が無いか空なら
`$HOME/.config/git/attributes`）を読み、これは `.gitattributes` と `$GIT_DIR/info/attributes` より優先度が低い
（gitattributes(5)、git 2.56.0、r995 の man で 2026-11-27 に確かめた）。

ハーネスは、git の hook を Windows では `claude-setup git-hooks` が、Linux では `nix/git-hooks.nix` が置き（ADR-0086）、
markdownlint の設定は両方で `claude-setup sync` が置く（ADR-0093）。

## 決定の要因

- `.gitattributes` の無いリポジトリでも、index に入る改行を LF にする。リポジトリの `.gitattributes` が書いた規則には
  逆らわない
- Windows で CRLF でなければ動かない `*.bat` と `*.cmd` は、作業ツリーで CRLF にする
- 中身の正を 1 つにし、Windows と Linux で規則がずれない
- 利用者が自分で置いた attributes（ファイルか `core.attributesFile`）を黙って奪わない（`claude-setup git-hooks` が
  `core.hooksPath` と hook のファイルで行っている扱いと同じ）

## 検討した案

### 案 A: 正を `config/git/attributes` に置き、Windows は `claude-setup git-attributes`、Linux は `programs.git.attributes` で置く（採用）

- Windows: `claude-setup git-attributes --from <ファイル>` が、git が読む場所にそのファイルを置く。`scripts/install-windows.sh`
  が `git-hooks` と並べて呼ぶ
- Linux: `nix/git-attributes.nix` がファイルを行に分けて `programs.git.attributes` に入れ、home-manager が
  `~/.config/git/attributes` に置く
- 長所: 正が 1 ファイル。Linux は home-manager の宣言のままで、既存のファイルとの衝突は home-manager の検査
  （checkLinkTargets）が扱う。利用者が `programs.git.attributes` に足した行とも衝突せずに 1 つのファイルにまとまる
- 短所: 仕組みが Windows と Linux で 2 つ

### 案 B: `claude-setup sync` の、ホームの下に置くファイル（ADR-0093 の markdownlint の設定と同じ）に足す

- 長所: 仕組みが 1 つで、Linux も activation の sync が置く
- 短所: sync はソースを正として上書きする（ADR-0093）ので、利用者の attributes を黙って消す。上書きしない規則を足すと、
  sync の 1 つのファイルだけが別の扱いになる。Linux で利用者が `programs.git.attributes` を使うと、home-manager の symlink と
  sync の書くファイルが同じ場所を取り合う

### 案 C: グローバルの `core.autocrlf=input` か `core.eol=lf` にする

- 長所: ファイルが要らない
- 短所: `core.autocrlf=input` は `*.bat` と `*.cmd` も LF にする。`core.eol` は `text` の属性が付いたファイルにしか効かず、
  `.gitattributes` の無いリポジトリでは何もしない

### 案 D: 何もせず、リポジトリごとに `.gitattributes` を書く

- 長所: 実装が無い
- 短所: 新しいリポジトリと他人のリポジトリで、書き忘れると CRLF が index に入る

## 決定と理由

案 A。

- 中身（`config/git/attributes`）: `* text=auto eol=lf`、`*.bat text eol=crlf`、`*.cmd text eol=crlf`。`text=auto` は
  バイナリと判定したファイルを変えない
- 置く場所: git が既定で読む `$XDG_CONFIG_HOME/git/attributes`（無いか空なら `~/.config/git/attributes`）。
  `core.attributesFile` は書かない（書かなくても git が読み、利用者の gitconfig を増やさない）
- Windows（`claude-setup git-attributes`）は、次のときに止まり、何も書かない
  - global か system の `core.attributesFile`（include した設定を含む。`GIT_CONFIG_NOSYSTEM` のときは system を見ない）が、
    置く場所と違う場所を指す。git はそこを読み、置く場所は読まないので、置いても効かず、そこを書き換えると利用者の規則を
    奪う
  - 置く場所に、ハーネスの置いたものでないファイルか symlink がある。ハーネスの置いたものは、1 行目が配る中身の 1 行目と
    同じファイルで、上書きして新しい中身にする（中身が同じなら何もしない）
- Linux（`nix/git-attributes.nix`）は、配る行を `lib.mkBefore` で `programs.git.attributes` の先に置く。git は、同じパスに
  当たる行のうち後の行を採るので、利用者が足した行が勝つ。`core.attributesFile` が置く場所と違う場所を指すと、
  assertion で評価を止める。置く場所の既存のファイルは、home-manager が扱う（既定は止まり、
  `home-manager.backupFileExtension` があれば退避して警告する）。git hook と同じ扱いで、ハーネスの側では足さない

### 帰結

- `.gitattributes` の無いリポジトリで、CRLF のファイルを add すると index は LF になる。既に CRLF で index にあるファイルは、
  git が変えない（`git status` に差分は出ない。2026-11-27、Git for Windows で、CRLF をコミットしたリポジトリに後から
  attributes を置いて確かめた）
- `.gitattributes` があっても、そこに `*` の行が無いリポジトリでは、その規則に当たらないファイルにこの規則が効く
- Linux でも、CRLF で書いたファイルの add が LF になり、`*.bat` と `*.cmd` は CRLF で取り出される。改行の既定が LF の
  Linux では、ほかに変わる点は無い
- Linux で `home-manager.backupFileExtension` を設定した構成（nixfiles）は、利用者が手で置いた `~/.config/git/attributes` を
  `.backup` に退避して置き換え、activation の警告だけを出す。止まらないので、退避したファイルの規則は自分で
  `programs.git.attributes` に移す
- 利用者が自分の規則を足すときは、Windows ではハーネスのファイルを直さず（次の設置で上書きされる）、リポジトリの
  `.gitattributes` か `$GIT_DIR/info/attributes` に書く
- 配る中身の 1 行目は変えない（変えると、Windows の前の版のファイルを、ハーネスのものと見分けられなくなる）

### 確認

- `crates/claude-setup/tests/git_attributes.rs`: 置く場所（`XDG_CONFIG_HOME` を含む）、2 度の実行、前の版の上書き、
  利用者のファイルと symlink と `core.attributesFile`（global、system、`GIT_CONFIG_NOSYSTEM`）で止まること、置いた後の
  `.gitattributes` の無いリポジトリで CRLF のファイルが LF で index に入り、`*.bat` が CRLF で取り出されること
- `nix/tests/git-attributes.sh`: home-manager の構成が置くファイルが `config/git/attributes` と同じこと、home-manager の
  checkLinkTargets が利用者のファイルで止まること、`core.attributesFile` を別の場所にすると評価が止まること、置いた
  ファイルで CRLF のファイルが LF で index に入ること
- Windows の CI（`setup` の Check the global git attributes）: `scripts/install-windows.sh` が置いたファイルが
  `config/git/attributes` と同じで、runner の `core.autocrlf=true` のもとでも、`.gitattributes` の無いリポジトリのテキストを
  LF で index に入れて LF で取り出し、`*.bat` を CRLF で取り出すこと
