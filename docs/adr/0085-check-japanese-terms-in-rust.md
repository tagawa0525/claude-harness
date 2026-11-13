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
textlint-rule-prh 6.0.0（Node.js）で当てている（2026-11-13、flake.lock の nixpkgs `4fe25e12eb9f` の版）。呼ぶ所は次のとおり。

- git hook: pre-commit の `ja-terms-staged`（`staged.sh`）、commit-msg の `commit-msg-check`（`commit-msg-check.sh`。中で
  `ja-terms-message`）
- PreToolUse hook: `claude-hooks` の git merge と git commit の本文のルール（ADR-0050、ADR-0073）が、Nix の
  `makeWrapper` で渡した store のパスの `ja-terms-message` と `commit-msg-check` を子プロセスで呼ぶ
- /retro: `memory-index.sh` が `~/.claude/bin/ja-terms-files`（`files.sh`）を呼ぶ（ADR-0074）
- 履歴の書き換え: `history-rewrite` の `prh_rules.py`（378 行）が、辞書を Python で読み、prh と同じ意味で当てる
  （ADR-0045）。JavaScript の正規表現を Python の `regex` の書き方に直し、直せない書き方は止める

ADR-0082 は、この検査を Windows で Node.js のまま動かすか、辞書を Rust で読むかを #101 の段階 3 で決めるとした。
ADR-0084 は、Nix の包む store のパスと `ja-terms-files` を Windows でどう用意するかを同じ段階に回した。関わる事実:

- 辞書の 104 の規則の pattern は、`/…/` で囲んだ JavaScript の正規表現（prh は u と m のフラグで読む）か、固定の
  文字列（`端のケース` など 56 の行。prh は正規表現の特殊な文字を逃がしてから読む）。正規表現は先読みと後読み（`(?=…)`、
  `(?!…)`、`(?<=…)`、`(?<!…)`）を使う。後読みには、長さの決まらないもの（`(?<!走らせ|走り|流し|測り)`、
  `(?<=(?<![一-龥])負の\n?)` など）がある。Rust の `regex` crate は先読みと後読みを扱わず、`fancy-regex` は長さの
  決まった後読みしか扱わない。`prh_rules.py` が Python の `re` でなく `regex` を使うのも、`re` が長さの決まった後読み
  しか読まないため
- textlint-rule-prh 6.0.0 は、Markdown を textlint の AST にし、Str のノードの原文（`getSource`。エスケープや改行を
  そのまま含む）に prh を当てる。Str でないコードスパンとコードブロックに加え、Link、BlockQuote、Emphasis（`*…*`。
  Strong の `**…**` は見る）の下は見ない。Header は見る（`textlint-rule-prh.js` の `createIgnoreNodeTypes` と既定の
  options）。textlint 15.7.1 で確かめた見ない範囲は、ほかにブロックの HTML、画像の代替文字列、先頭の `---` で囲んだ
  YAML front matter（ADR-0058 の帰結）。コードとテキストのファイル（`.rs`、`.sh` など）は、`@textlint/text` が 1 行ずつ Paragraph と Str
  にして当てる
- `commit-msg-check.sh`（58 行）と `message.sh`（29 行）は、辞書の検査のほかに、Merge、fixup!、squash!、Revert の件名の
  扱い、Conventional Commits の件名の形、件名の 72 文字の上限と 50 文字の警告、検査する本文の切り出し（件名を除くか、
  scissors の行から後を除くか）、終了コード（0 = 通す、1 = 規約に反する、2 = 語の検査が失敗した）を持つ。sh と sed で
  書いてある
- `prh_rules.py` は、Markdown を区別せず、テキストのブロブの全文とコミットの本文に、段落ごとに規則を当てる
  （ADR-0045 の案 D1）
- textlint の起動は重く、`commit-msg-check.sh` は軽い検査の後に呼んでいる
- Windows の機械に Node.js を入れずに、辞書を Rust で読む（2026-11-13 のユーザーの決定）

## 決定の要因

- Windows と Linux で、同じ文に同じ判定をすること（ADR-0082）
- 辞書の意味（textlint-rule-prh 6.0.0 が使う prh 5.4.4）と、Markdown のどこを見るか（ADR-0058）を変えないこと
- 同じ辞書を当てる実装の数を減らすこと（今は textlint と `prh_rules.py` の 2 つ）
- 依存を増やさないこと（ADR-0047 の数え方）

## 検討した案

- 案 A: Windows にも Node.js を入れ、textlint と textlint-rule-prh を版を固定して npm で入れる。Linux は今のまま
- 案 B: 辞書を読む Rust の実装を Windows でだけ使い、Linux は textlint のまま
- 案 C: 辞書を読む Rust の実装を作り、Windows と Linux の両方で textlint と `prh_rules.py` を置き換える

## 決定と理由

採用: 案 C。

- 新しい crate（名前は実装の PR で決める）に、辞書を読み、文に当てるライブラリと CLI を置く
  - 辞書: `prh.yml` を読む。`/…/フラグ` の pattern は、ECMAScript の正規表現のエンジンの `regress` で、prh と同じ u と
    m のフラグで読む（書き直さない）。固定の文字列は、prh と同じく特殊な文字を逃がしてから読む（`prh_rules.py` と同じ）。
    specs を読み込むときに確かめる
  - 当て方: prh 5.4.4 の意味（段落ごとの一致、expected の `$1`、重なった一致の選び方。`prh_rules.py` の冒頭の注記）
  - 当てる範囲は 2 つの mode に分ける。検査の mode は textlint と同じ: Markdown は `pulldown-cmark` で、textlint の Str の
    ノードに当たる原文の範囲を当て（見ない範囲は上の背景の、ノードの種類と ADR-0058 の一覧）、コードとテキストの
    ファイルは 1 行ずつ当てる。履歴の書き換えの mode は `prh_rules.py` と同じ: 全文に段落ごとに当てる（ADR-0045 の
    案 D1。コードの中の語と、行をまたぐ一致も直す）
  - コミットのメッセージの規約: `commit-msg-check.sh` と `message.sh` の規約（上の背景の、件名の扱い、形、長さ、本文の
    切り出し、終了コード）を、同じ判定と同じ終了コードのまま CLI とライブラリに移す
- 置き換えるもの: `ja-terms-message`、`commit-msg-check`、`ja-terms-staged`、`ja-terms-files`、`prh_rules.py`。
  `ja-terms-staged` と `ja-terms-files` の振る舞い（`.textlintignore` の除外、見る拡張子、出力の形）も同じに保つ。
  git hook（#101 の段階 4）と `history-rewrite`（段階 7）も同じライブラリか CLI を使う
- `claude-hooks` からの呼び方: 今のとおり子プロセスで CLI を呼ぶ。辞書を `claude-hooks` のビルドに埋めると、辞書を
  変えるたびに `claude-hooks` の作り直しとテストが走るため（`claude-hooks/package.nix` の注記）。Linux は今のとおり
  `makeWrapper` の環境変数で store のパスを渡し、環境変数が無い Windows は PATH（`cargo install` の置き場）で探す
- 移す順序: まず Rust の実装を作り、textlint と同じ入力に同じ結果を返すかを比べる。比べる入力は、辞書の specs と、
  このリポジトリの追跡している Markdown とコードのファイルと、コミットのメッセージの履歴。specs と、ファイルから
  選んだ固定の入力は `nix flake check` のテストにする（`nix flake check` の sandbox は `.git` を持たず、Linux の
  GitHub Actions も無いので、履歴との比較は実装の PR で手元で流し、結果を PR に書く）。結果が揃ってから、呼ぶ所を
  1 つずつ付け替え、textlint を外す。外した後は、比べたときの textlint の結果を期待値のファイルに残し、テストは
  それと比べる

理由:

- 案 C は、ADR-0014 が採らなかった案（辞書の形式と照合を自分で決めて保守する。既存のツールがあるので作らない）に
  近いが、辞書の形式は prh のまま変えない。ADR-0014 の後、ADR-0045 で prh の照合を `prh_rules.py` に自分で持ち、
  Windows で Node.js を使わないと決まった今は、照合を自分で持つ費用はすでに払っている
- 案 C は、同じ辞書を当てる実装が 1 つになる。今の textlint と `prh_rules.py` の 2 つは、同じ辞書を別の正規表現の
  エンジンで当てていて、揃っているかを確かめる手段が無い。段階 7 で `prh_rules.py` を Rust に移すときに Rust の
  実装がどのみち要るので、それを検査にも使う
- 案 C は、PreToolUse hook が Node.js を起動せずに済む。Windows では、検査の CLI を `claude-hooks` と同じく
  `cargo install` で入れれば PATH で見つかるので、store のパスの代わり（ADR-0084 が段階 3 に回したもの）を別に
  用意しなくてよい
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

## 補足

- 2026-11-13: 辞書を読み文に当てるライブラリ（`modules/home/parts/ja-terms/src/lib.rs`）を作る途中で、レビューで
  次の版を捨てた。
  - 複数の辞書の規則をただ並べた。textlint-rule-prh は複数の辞書を prh の `Engine.merge` でつなぎ、前の辞書と同じ
    expected の規則を外す。辞書は 1 つなので、1 つのファイルだけを読む
  - `diffs` で重なりを 2 度外した。textlint-rule-prh が報告するのは、prh の `ChangeSet` を作るときに 1 度外したもの。
    外し方は前の要素を外す前の並びで比べるので、0〜10、5〜7、8〜12 の一致では、8〜12 が 2 度目にだけ外れる
  - 引用符の無い `1:30` を、prh と同じく文字列と読むとした。prh 5.4.4 の js-yaml 3.14.1 は 60 進の数と読む
- 2026-11-13: prh が読まずに止める辞書のうち、Rust の実装が読むもの（js-yaml 3.14.1 が数や日付と読む引用符の無い値、
  u フラグを付けない V8 の `RegExp` が読めない pattern、空の from か to の spec、入れ子の並びの pattern、文字列の
  version）は揃えない。どれも今の辞書には無い。比べる間は textlint が先に止め、textlint を外した後は Rust の実装
  だけが辞書を読むので、揃えても判定は変わらない。今の辞書を当てた結果は、このリポジトリと nixfiles の追跡して
  いる 355 のファイルで `prh_rules.py` とすべて同じだった
- 2026-11-13: 検査の mode の Str の範囲（`modules/home/parts/ja-terms/src/strs.rs`）は、textlint 自身に答えさせて
  比べる。Str の原文の全体に一致する規則だけの辞書（`tests/textlint/whole.yml`）を当てると、textlint は Str ごとに
  開始の位置と原文を報告する。固定の入力の期待値は `tests/textlint/update.sh` がこれで作る。このリポジトリ、nixfiles、
  r995 の 24 のリポジトリの Markdown 1369 ファイル（189,592 の Str）、コミットのメッセージ 34041 件（157,828）、
  コードのファイル 2356（487,185 行）で、textlint の答えとすべて同じになった。違いの見つかった次の 5 つは、textlint の
  micromark 2.11.4 と remark の拡張が CommonMark 0.31 の pulldown-cmark と違う読み方で、揃えた。
  - lazy continuation を読まない。字下げか `>` の足りない段落の続きの行で、項目と引用を閉じて新しい段落にする
    （引用の後の lazy な行は引用の外で、検査する）
  - literal autolink（micromark-extension-gfm-autolink-literal 0.5.7）。`https://` は日本語の後でも始まり、パスは
    空白、`<`、行の終わりまで続く（日本語の句読点では終わらず、後のコードスパンも含む）
  - 1 つの `~` の取り消し線（micromark-extension-gfm-strikethrough 0.6.5）
  - front matter は文の先頭だけ（pulldown-cmark の metadata block は文の途中の `---`…`...` も読む）
  - ASCII でない記号を句読点に数えない（強調の区切りの判定。`C*×…*` を強調と読む）
- 2026-11-13: lazy な行は、1 度の読みで見つけたものすべての前に空行を入れる方式を試して捨てた。前の行で項目が閉じると、
  後の行が別の項目の続きになり、lazy でなくなる（lsp-det の `docs/vision.ja.md` で、`1b.` の行の前に入れると、後の
  `2.` の行は新しい項目でなく段落の続きになり、その後の `2b.` の行は lazy でない）。最初の 1 行ずつ入れて読み直す
- 2026-11-13: 次は textlint と揃えず、違いとして残す（どれも比べた入力には無い）。
  - BOM で始まるテキストのファイル: textlint 15.7.1 の @textlint/text は、BOM を外した文の位置で、BOM を含む文から
    Str の原文を切るので、行が 1 文字ずれる（BOM の後の `日本語` と `abc` の 2 行の、2 行目を `bc` と改行と読む）。Rust の実装は BOM の後
    から行に分ける
  - コードスパン、強調、リンクを含むインラインの注（`^[…]`）: Rust の実装は、pulldown-cmark の 1 つの Text の続きの
    中でだけ注を探すので、注を分けない
  - 取り消し線の `~` と強調の `*` が交差する文（`a ~b *c~ d* e`）: pulldown-cmark は取り消し線を知らずに強調を組む
    ので、micromark が Str に残す部分を強調として外す
- 2026-11-13: 検査の mode の報告（`modules/home/parts/ja-terms/src/report.rs`、`pragma.rs`、`main.rs` の `ja-terms check`）は、
  報告を確かめる規則だけの辞書（`tests/textlint/report.yml`）を当てた textlint の `--format unix` の出力を、固定の入力の
  期待値にする（`tests/textlint/update.sh`）。Str の比較と同じ 37,766 のファイルとメッセージで、今の辞書の報告 2,882 件と、
  一致が多く重なりやすい規則の辞書（`する`、`/[ぁ-ん]{3}/`、`/。\n?/` など 8 つ）の報告 367,017 件が、textlint の
  報告とすべて同じになった。レビューで次の版を捨てた。
  - 拡張子 markdown も Markdown として読んだ。ja-terms-files は md だけを読み、markdown は止めるので、見る拡張子が変わる
  - `prh:disable:<pattern>` を regress の文字の単位で照合した。prh は u のフラグ無しで読むので、V8 は UTF-16 の単位で
    照合し、`^.$` はサロゲートペアの文字に当たらない。次に一致だけを UTF-16 の単位にした版も捨てた。pattern の中の
    サロゲートペアの文字（`prh:disable:𩸽`）が 1 つの文字のまま、2 つのサロゲートの一致に当たらない
  - 拡張子を `Path::extension` で読んだ。`.md` のように . で始まる名前に拡張子を返さず、ja-terms-files が md と読む
    ファイルを止める
  - 同じ位置と同じ文の報告を 1 つにまとめた（textlint の filter-duplicated-process.js）。Str の範囲は重ならず、1 つの
    Str の中の一致は重なりを外した後に同じ開始を持たないので、prh だけの報告では起きない
  - `prh:disable:<pattern>` の `\u{…}` は、textlint と揃えず違いとして残す（今の辞書にも比べた入力にも無い）。V8 は
    u のフラグが無いと Annex B のとおり `u` と `{…}` の文字（または繰り返し）と読むが、regress は Annex B の読み方を
    持たず、符号位置と読む
