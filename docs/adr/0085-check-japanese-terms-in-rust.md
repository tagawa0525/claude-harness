---
status: proposed
date: 2026-11-13
requires: [ADR-0014, ADR-0045, ADR-0047, ADR-0058, ADR-0082, ADR-0084]
supersedes: []
superseded-by:
issues: ["#101"]
---

# ADR-0085: 崩れた日本語の語の検査を、prh の辞書を読む Rust の実装 1 つにし、Windows と Linux の両方で使う

## 背景

崩れた日本語の語の検査（ADR-0014）は、辞書 `modules/home/parts/ja-terms/prh.yml` を textlint 15.7.1 と
textlint-rule-prh 6.0.0（Node.js）で当てている（2026-11-13、`700b1460` の nixpkgs の版）。呼ぶ所は次のとおり。

- git hook: pre-commit の `ja-terms-staged`（`staged.sh`）、commit-msg の `commit-msg-check`（`commit-msg-check.sh`。中で
  `ja-terms-message`）
- PreToolUse hook: `claude-hooks` の git merge と git commit の本文のルール（ADR-0050、ADR-0073）が、Nix の
  `makeWrapper` で渡した store のパスの `ja-terms-message` と `commit-msg-check` を子プロセスで呼ぶ
- /retro: `memory-index.sh` が `~/.claude/bin/ja-terms-files`（`files.sh`）を呼ぶ（ADR-0074）
- 履歴の書き換え: `history-rewrite` の `prh_rules.py`（378 行）が、辞書を Python で読み、prh 6.0.0 と同じ意味で当てる
  （ADR-0045）。JavaScript の正規表現を Python の `regex` の書き方に直し、直せない書き方は止める

ADR-0082 は、この検査を Windows で Node.js のまま動かすか、辞書を Rust で読むかを #101 の段階 3 で決めるとした。
ADR-0084 は、Nix の包む store のパスと `ja-terms-files` を Windows でどう用意するかを同じ段階に回した。関わる事実:

- 辞書の 104 の規則の pattern は JavaScript の正規表現（prh は u と m のフラグで読む）で、先読みと後読み（`(?=…)`、
  `(?!…)`、`(?<=…)`、`(?<!…)`）を使う。後読みには、長さの決まらないもの（`(?<!走らせ|走り|流し|測り)`、
  `(?<=(?<![一-龥])負の\n?)` など）がある。Rust の `regex` crate は先読みと後読みを扱わず、`fancy-regex` は長さの
  決まった後読みしか扱わない。`prh_rules.py` が Python の `re` でなく `regex` を使うのも、`re` が長さの決まった後読み
  しか読まないため
- textlint-rule-prh 6.0.0 は、Markdown を textlint の AST にし、Str のノードの原文（`getSource`。エスケープや改行を
  そのまま含む）に prh を当てる。Str でないコードスパンとコードブロックに加え、Link、BlockQuote、Emphasis（`*…*`。
  Strong の `**…**` は見る）の下は見ない。Header は見る（`textlint-rule-prh.js` の `createIgnoreNodeTypes` と既定の
  options。ADR-0058）。コードとテキストのファイル（`.rs`、`.sh` など）は、`@textlint/text` が 1 行ずつ Paragraph と Str
  にして当てる
- `commit-msg-check.sh`（58 行）と `message.sh`（29 行）は、件名の形と長さの検査と、検査する本文の切り出し
  （件名を除くか、scissors の行から後を除くか）で、textlint を呼ぶ前の処理は sh と sed で書いてある
- textlint の起動は重く、`commit-msg-check.sh` は軽い検査の後に呼んでいる
- Windows の機械に Node.js を入れずに、辞書を Rust で読む（2026-11-13 のユーザーの決定）

## 決定の要因

- Windows と Linux で、同じ文に同じ判定をすること（ADR-0082）
- 辞書の意味（prh 6.0.0）と、Markdown のどこを見るか（ADR-0058）を変えないこと
- 同じ辞書を当てる実装の数を減らすこと（今は textlint と `prh_rules.py` の 2 つ）
- 依存を増やさないこと（ADR-0047 の数え方）

## 検討した案

- 案 A: Windows にも Node.js を入れ、textlint と textlint-rule-prh を版を固定して npm で入れる。Linux は今のまま
- 案 B: 辞書を読む Rust の実装を Windows でだけ使い、Linux は textlint のまま
- 案 C: 辞書を読む Rust の実装を作り、Windows と Linux の両方で textlint と `prh_rules.py` を置き換える

## 決定と理由

採用: 案 C。

- 新しい crate（名前は実装の PR で決める）に、辞書を読み、文に当てるライブラリと CLI を置く
  - 辞書: `prh.yml` を読み、pattern を ECMAScript の正規表現のエンジンの `regress` で、prh と同じ u と m のフラグで
    読む（書き直さない）。specs を読み込むときに確かめる
  - 当て方: prh 6.0.0 の意味（段落ごとの一致、expected の `$1`、重なった一致の選び方。`prh_rules.py` の冒頭の注記）
  - Markdown: `pulldown-cmark` で、textlint の Str のノードに当たる原文の範囲を当てる（見ない範囲は textlint と同じ。
    上の背景）。コードとテキストのファイルは 1 行ずつ当てる
- 置き換えるもの: `ja-terms-message`、`commit-msg-check`、`ja-terms-staged`、`ja-terms-files`、`prh_rules.py`。
  `claude-hooks` は子プロセスを呼ばずにライブラリの関数を呼ぶので、store のパスを渡す `makeWrapper` が要らなくなる。
  git hook（#101 の段階 4）と `history-rewrite`（段階 7）も同じライブラリか CLI を使う
- 移す順序: まず Rust の実装を作り、textlint と同じ入力に同じ結果を返すかを比べる。比べる入力は、辞書の specs と、
  このリポジトリの追跡している Markdown とコードのファイルと、コミットのメッセージの履歴。specs と、ファイルから
  選んだ固定の入力は `nix flake check` のテストにする（`nix flake check` の sandbox は `.git` を持たず、Linux の
  GitHub Actions も無いので、履歴との比較は実装の PR で手元で流し、結果を PR に書く）。結果が揃ってから、呼ぶ所を
  1 つずつ付け替え、textlint を外す

理由:

- 案 C は、ADR-0014 が採らなかった案（辞書の形式と照合を自分で決めて保守する。既存のツールがあるので作らない）に
  近いが、辞書の形式は prh のまま変えない。ADR-0014 の後、ADR-0045 で prh の照合を `prh_rules.py` に自分で持ち、
  Windows で Node.js を使わないと決まった今は、照合を自分で持つ費用はすでに払っている
- 案 C は、同じ辞書を当てる実装が 1 つになる。今の textlint と `prh_rules.py` の 2 つは、同じ辞書を別の正規表現の
  エンジンで当てていて、揃っているかを確かめる手段が無い。段階 7 で `prh_rules.py` を Rust に移すときに Rust の
  実装がどのみち要るので、それを検査にも使う
- 案 C は、PreToolUse hook が Node.js を起動せずに済む。`claude-hooks` の中で関数を呼ぶので、Windows でも store の
  パスを解く問題（ADR-0084 が段階 3 に回したもの）が無くなる
- 案 A は、Windows の依存に Node.js と npm の版の管理が残り、段階 7 の Rust の実装と textlint の 2 つが残る
- 案 B は、Windows と Linux で別の実装が判定するので、同じ文に違う判定を返しうる（ADR-0082 の要因に反する）
- `regress` は ECMAScript 2018 の正規表現を実装し、長さの決まらない後読みも読むので、辞書の pattern を prh と同じ
  意味で書き直さずに読める（<https://github.com/ridiculousfish/regress> の README、2026-11-13 に閲覧）。事実上の標準と
  言える crate ではないが、`regex` か `fancy-regex` で読むには長さの決まらない後読みの規則を書き直す必要があり、
  辞書の書き方（JavaScript の正規表現）と prh の意味が分かれる。`pulldown-cmark` は Rust の CommonMark のパーサーとして
  事実上の標準

### 帰結

- 良い点: Windows と Linux の判定が同じ実装になり、Node.js と Python が検査の依存から外れる
- 良い点: PreToolUse hook と commit-msg の hook が、textlint の起動を待たない
- 悪い点: textlint の Markdown の AST（remark）と `pulldown-cmark` は、エッジケースで文の切り方が違いうる。比べる
  テストで見つかった違いは、揃えるか、違いを記録してから付け替える
- 悪い点: prh の意味を自前で持つので、prh の新しい版の変更には自分で追いつく（今の `prh_rules.py` と同じ）
- ADR-0014 の決定（textlint と textlint-rule-prh で当てる）と ADR-0058（textlint の Markdown のパーサーで見る範囲を
  決める）は textlint を外す PR で、ADR-0045 の決定（`prh_rules.py` を Python で書く）は `prh_rules.py` を外す PR で、
  「## 変更」を書く
- 実装はまだ決定に追いついていない。進みは #101 に書く

### 確認

- 比べるテストが、specs と固定の入力で textlint と同じ結果を返す。履歴との比較の結果が、実装の PR にある（違いが
  あれば、その記録がある）
- 辞書の specs が、Rust の実装で通る
- Windows の CI（`windows.yml`）で、新しい crate と `claude-hooks` のテストが通る
