---
status: accepted
date: 2026-10-22
requires: [ADR-0014, ADR-0033]
supersedes: []
superseded-by:
issues: []
---

# ADR-0050: git merge の本文の日本語の語を、ツール呼び出しの前に commit-msg と同じ textlint で検査する

## 背景

GitHub を使わないリポジトリ（xlc）は、feature branch を `git merge --no-ff -m "<本文>"` で main に入れる。git は
マージコミットの hook を pre-merge-commit → commit-msg の順に呼ぶ。pre-merge-commit は品質チェック（run-checks
--merge）で数分かかり、commit-msg は本文の崩れた日本語の語を textlint と prh の辞書（ADR-0014）で数秒で検査する。
本文に辞書で崩れた語（辞書が別の語に言い換える語）があると、数分の品質チェックを待ってから
commit-msg で拒まれ、直してもう一度数分待つ。2026-10-22 の xlc のマージで 2 回起きた。

git の hook の順は変えられない。pre-merge-commit には本文が渡らず（`-m` の値は commit-msg まで読めない）、
pre-merge-commit の中で先に検査することもできない。

ユーザーは /retro で挙がった案のうち、claude-hooks（Claude のツール呼び出しの前に走る PreToolUse の hook）で
`git merge` の `-m` / `-F` の本文を先に検査する案を採った（2026-10-22）。

## 検討した案

- 案 A: claude-hooks に、`git merge` の `-m` / `-F` の本文を、commit-msg と同じ検査に渡して、崩れた語が
  あれば「誤り => 正しい語」と修正方法を示して止めるルールを足す。検査の store のパスは、package.nix で
  バイナリに埋める
- 案 B: 品質チェックを pre-merge-commit から commit-msg に移し、語の検査の後に実行する。commit-msg は
  本文の検査の場所で、品質チェックをそこに置くと hook の役割が混ざる。`git merge` 以外のコミットでも commit-msg は
  呼ばれるので、品質チェックを実行する条件（マージのときだけ）を commit-msg で見分け直すことになる
- 案 C: 検査の CLI（本文を受け取って辞書で検査するコマンド）を公開し、/git-merge のスキルの手順で、マージの前に呼ぶ。
  手順を飛ばしたマージや、スキルを経ない `git merge` には効かない。hook のように機械で止まらない

## 決定と理由

案 A。

- ツール呼び出しの前に止まるので、品質チェックを待たない。textlint の起動と検査は、手元（r995、1 コア）で 0.5 秒ほど
  （本文 2 行で 0.52〜0.54 秒、3 回）。nix でビルドした claude-hooks のこのルールだけで、崩れた語のある本文は
  0.37〜0.39 秒、正しい本文は 0.34〜0.38 秒、本文の無い `git merge` は 4〜8 ミリ秒（各 3 回、2026-10-22）
- 判定は commit-msg と同じにする。commit-msg の語の検査（件名が Merge / fixup! / squash! / Revert なら件名を除く、
  scissors の行から後を除く、空白だけなら textlint を呼ばない、textlint の設定と引数）を 1 つのスクリプト
  （ja-terms-message。`modules/home/parts/ja-terms/message.sh`、`textlint.nix`）にまとめ、commit-msg とこのルールが
  同じものを呼ぶ。commit-msg が呼ばれない形（`--ff-only`、`--squash`、グローバルの core.hooksPath を使わない
  リポジトリ）と、commit-msg が検査を飛ばすリポジトリ（`upstream` リモートのある fork、ローカルの commit-msg に
  置き換わるリポジトリ）では、このルールも検査しない。commit-msg より厳しく止めると、git が通すマージまで止める
- 最初の版は、検査する範囲（件名を除く、scissors）を Rust で書き直し、textlint と設定だけを共有した。ローカルの
  レビューで、commit-msg の範囲を変えるとこのルールが黙ってずれる（通したマージが数分後に拒まれるか、通るマージを
  止める）と指摘され、範囲ごとスクリプトに移した
- `-m` の値は、ヒアドキュメント（`-m "$(cat <<'EOF' … EOF)"`）なら本文を取り出し、複数の `-m` は git と同じく
  空行で連ねる。`-F` のファイルは、コマンドの行き先（`cd` と `git -C`）からの相対で読む。gh pr create の本文を
  読む `body_text`（pre-pr-create-check）は、最初の 1 つの値の原文（クォートと `$(cat <<'EOF'` を含む）を返し、
  `-m<値>` のまとまりも読まないので、git のオプションの読み方（git_args）に値を返す関数を足して使う
- 既存の pre-git-merge-check（既定ブランチへのマージで ADR の数を見る）には入れず、別のルールにする。対象が違い
  （commit-msg は既定ブランチ以外へのマージでも検査する）、pre-git-merge-check のエスケープ（ALLOW_MULTI_TOPIC）が
  語の検査まで外すことになるため
- 検査を起動できない、検査に失敗した、`-F -`（標準入力）や読めないファイルで本文が分からないときは、判定できなかった
  理由を利用者に見せて通す（ADR-0033）。commit-msg が同じ検査で止めるので、検査は抜けない
- エスケープは設けない。語を置き換えれば通る

## 帰結

- 本文の語の誤りは、数分の品質チェックの前に、0.5 秒ほどで止まる
- 検査の store のパスはビルド時に埋めるので、cargo で直接ビルドしたバイナリ（`.claude/tests/hooks.sh` の既定）には
  無く、このルールは判定できない理由を出して通す。nix でビルドしたもの（配置するもの）だけが検査する
- 辞書、textlint の設定、検査する範囲を変えると、commit-msg と claude-hooks の両方が同じものを使う（textlint.nix と
  message.sh が 1 か所）
- `--no-ff` の無い `git merge -m` は、fast-forward になれば commit-msg を呼ばないが、このルールは fast-forward に
  なるかを見ずに検査する（崩れた語を置き換えれば通るので、止めすぎの害は小さい）。git の整え方のうち、先頭の空行と
  `--cleanup=strip` の # の行は合わせ、設定の `commit.cleanup` は見ない
- git がメッセージに足す行（`--log` や `merge.log` の、マージするコミットの件名の一覧）は、このルールには見えない。
  その件名に崩れた語があれば、今までどおり commit-msg が数分後に拒む。xlc と全体の設定は `merge.log` を使っていない
  （2026-10-22）

## 確認

- ルールの判定（本文の取り出し、検査しない形、検査の終了コードの扱い）は、cargo のユニットテストが偽の
  ja-terms-message で確かめる
- 埋めた ja-terms-message と辞書で、崩れた語を止め、正しい語と Merge の件名を通すことは、package.nix の
  installCheckPhase がビルドしたバイナリで確かめる（`nix build .#claude-hooks`、flake の checks）
- commit-msg の判定が変わらないことは、`modules/home/parts/tests/ja-terms.sh` と `commit-msg-conventions.sh` で確かめる
