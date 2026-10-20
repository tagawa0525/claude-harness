---
status: accepted
date: 2026-10-21
requires: [ADR-0013, ADR-0014]
supersedes: []
superseded-by:
issues: []
---

# ADR-0045: 履歴の中の崩れた語を、prh の辞書で filter-repo に 1 回流して直し、番号は commit-refs rewrite-history で付け直す

## 背景

ADR-0014 の辞書（`modules/home/parts/ja-terms/prh.yml`）は、これから書く文書とコミットの本文の崩れた語を止める。
過去の文書、コードのコメント、コミットの本文には、辞書を足す前の語が残る。xlc では 2026-10-11 に履歴の語を直し、
記録の中のコミットの番号を付け直した。そのツールは xlc の `references/history-rewrite/`（git で追跡していない）にしか
なく、次の問題があった。

- 規則を `cb_lib.py` の `RULES` に手で書き直していて、辞書と食い違っていた（`RULES` の「効き」は辞書に無く、辞書が
  「エッジケース」に直す規則は `RULES` に無い）
- 対象のリポジトリとツールの置き場のパスが決め打ち
- 番号の付け直しは、filter-repo を前の回の commit-map で繰り返して収束させる（ADR-0013 が採らなかった案 A）。
  付け直しの規則も、`commit-refs`（ADR-0012）とは別に Python で書いていた
- 書き換えたものを生きているリポジトリに移す手順は、スクリプトに残っていない

ユーザーの指示（2026-10-21）: 「references にしかないツールはちゃんと昇格させておきましょう」。

## 決定の要因

- 規則の出典を 1 つにする。書き換えた履歴が、hook（ADR-0014）の検査を通る形になる
- 番号の付け直しは ADR-0013 の `commit-refs rewrite-history` に任せ、規則を 2 か所に書かない
- 書き換えた後の履歴が、元の履歴に語の変換だけを当てたものであることを、ref を移す前に確かめる
- 元のリポジトリは、書き換えて確かめ終わるまで変えない

## 検討した案

- 置き場
  - 案 A1: nixfiles の `modules/home/parts/history-rewrite/` に Python の CLI として置き、`package.nix` でビルドして
    PATH に入れる（`commit-refs` と同じ形）。手順はスキル `.claude/skills/history-rewrite/` に書く
  - 案 A2: xlc の `scripts/` に置く
- 規則の出典
  - 案 B1: prh.yml を読み、prh 6.0.0 と同じ意味で当てる。辞書に無い 1 回限りの置き換えは、同じ形のファイルを引数で
    足す
  - 案 B2: 規則をツールに書く（`cb_lib.py` の形）
  - 案 B3: textlint の `--fix` を各ブロブに当てる
- 番号の付け直し
  - 案 C1: 2 段にする。第 1 段は filter-repo の file-info-callback と message-callback で語だけを 1 回で直す。第 2 段は
    同じリポジトリで `commit-refs rewrite-history .git/filter-repo/commit-map`（ADR-0013）
  - 案 C2: 語と番号を同じ callback で直し、filter-repo を収束まで繰り返す（xlc のツール）
  - 案 C3: `commit-refs rewrite-history` に辞書の適用を足す（Rust）
- 直す範囲
  - 案 D1: テキストのブロブ（先頭 8000 バイトに NUL が無く、UTF-8 として読めるもの）の全文と、コミットとタグの本文
  - 案 D2: textlint-rule-prh と同じく、Markdown の Str ノードだけ

## 決定と理由

採用: 案 A1、B1、C1、D1。

置き場（A1）: 辞書は nixfiles のもので、ツールはどのリポジトリでも使う（A2 は xlc に固有でないものを xlc に置く）。
ja-terms の辞書と fix-markdown-lint.py の両方を store のパスで参照できる（git.nix の textlint と同じ形）。第 1 段は
filter-repo の Python の callback なので、ツールも Python で書く。依存（git-filter-repo、pyyaml、regex）は
`python3.withPackages` で入れる。ADR-0012 の「依存を増やさない」は `commit-refs` の crate の話で、ここには当てない。

辞書（B1）: B2 は辞書と食い違う。B3 はブロブごとに node を起こし、Markdown のコードを直さない（下の D1）。

- 既定の辞書は ja-terms の prh.yml（hook と同じもの）。`--dictionary` で置き換え、`--extra-rules` で規則を足す
  （どちらも prh の形で、重ねて渡せる。規則は渡した順に並べ、prh の `Engine.merge` のように同じ expected の規則を
  外さない）
- 一致の意味は prh 6.0.0（`lib/engine.js`、`rule.js`、`paragraph.js`、`changeset/`）に揃える:
  - 文章を `\n{2,}` で段落に分け、段落ごとに、すべての規則の一致を元の文字列の上で集める（規則を順に当てない）
  - 重なった一致は、同じ開始なら長い方、違う開始なら先の方を採る
  - expected の `$1`〜`$99` は捕捉した群に置き換える
  - 一致した文字列が expected と同じなら直さない
- pattern は prh と同じく、`/…/` を JavaScript の正規表現（u フラグと m フラグ）、ほかを固定の文字列として読む。
  Python の `regex` に書き直すのは、prh の辞書が長さの違う選択肢を lookbehind に使い（PR #413 の
  `(?<=[にがのをはもでな、]|^)抜け`）、標準の re ではそれを読めないため
- `^`、`$`、`.` は JavaScript の行の終わり（`\n`、`\r`、U+2028、U+2029）で読み替える。当たる範囲が Python と違う
  書き方（`\d`、`\w`、`\s`、`\b`、i フラグ、空の文字クラス）と、書き直していない書き方（後方参照、名前つきの群、Unicode の
  性質、prh の `options`、`regexpMustEmpty`、`imports`、pattern の無い規則）は、推測せずに止める
- 辞書を読むとき、どの規則も specs を確かめる（prh と同じ）。書き直しの誤りは、ここで止まる

2 段（C1）: C2 は ADR-0013 が案 A として採らなかった方式で、付け直しの規則も 2 か所になる。C3 は lookbehind を読む
ために fancy-regex のような crate が要り、JavaScript の正規表現の意味の書き直しを Rust で書くことになる。

- 第 1 段は番号に触らない（`--preserve-commit-hashes`）。番号は第 2 段の `commit-refs` だけが付け直すので、規則は
  `commit_refs::Map`（ADR-0012）の 1 か所に残る。番号の置き換えは同じ桁数なので、第 1 段で揃えた表は崩れない
- 第 1 段は空になったコミットも消さない（`--prune-empty=never`、`--prune-degenerate=never`）。語を手で直しただけの
  コミットは、変換の後に親と同じ中身になる。消すと元のコミットとの 1 対 1 の照合ができず、文書がそのコミットを指して
  いると、第 2 段が「消えたコミットを指す」で止まる
- 第 1 段の後に、元のすべてのコミットについて、書き換えた後のものと突き合わせる（照合）。作者とコミッターと符号化、
  親の対応、本文（元の本文に変換を当てたもの）、すべてのファイル（テキストは変換を当てたもの、ほかは元のまま。
  symlinkとサブモジュールは元のまま）、ブランチとタグの行き先（注釈つきのタグは、元のタグに変換を当てた
  オブジェクト）が合わなければ止める。番号の変わらないコミットも確かめる（変換が中身を変えるはずなのに
  filter-repo が当てなかったものを見る）。元のオブジェクトはミラーから alternates で読み、書き換えたリポジトリに
  ref を足さない
- 照合の期待は、filter-repo 2.47.0 が fast-export に渡す設定と同じ形にする: タグの本文の署名を外す
  （`--signed-tags=strip`）、tagger の無いタグに仮の tagger を付ける（`--fake-missing-tagger`）。本文は元の符号化の
  まま渡す（`--preserve-commit-encoding`。UTF-8 に直すと encoding の見出しが消える）。第 2 段がすぐに全体を
  書き直すので、第 1 段では repack しない（`--no-gc`）
- 第 2 段の照合と ref の移し方は ADR-0013 のとおり。`commit-refs` が止まれば、ツールも止まる

直す範囲（D1）: 直したいのは、コードのコメントとコミットの本文の語でもある。ja-terms の pre-commit も、コードと
テキストはファイルの全体を見る（`@textlint/text`）。D1 は Markdown のコードブロックとインラインのコードも直すので、
語そのものを例に挙げる文書（辞書と、辞書のテスト）は、除くパス（`--exclude`、既定は無し）で外す。語を論じる文書でも、
その語が直す対象の造語なら外さずに直す（xlc の ADR が論じる、確率テーブルを指す造語。ユーザーの判断、2026-10-21）。Markdown と
本文の表は、直した行を含む表だけを fix-markdown-lint.py の `format_table` で揃え直す（run-checks の Markdown の
自動修正と同じ形にし、書き換えで崩れた表を残さない）。

textlint との違い: textlint-rule-prh は、Markdown の Str ノード（インラインのコードと見出しの外の文）ごとに prh を呼ぶ。
このツールは全文を段落に分けて呼ぶので、次が違う。

- prh の `prh:disable` の指示は当てない。prh は指示のある段落の一致を外すが、その段落は textlint ではノードで、全文では
  空行の間になり、同じ範囲にならない。全文の段落で当てた版は、インラインのコードに指示の名前を書いたこの ADR の
  段落を丸ごと外し、textlint が見つける語を残した（2026-10-21、PR #413 の辞書で、この ADR と `prh_rules.py` の
  4 か所）。xlc と nixfiles の履歴に、指示を使った文書は無い（2026-10-21 に `git log --all -S'prh:disable'` で確かめた）。
  残したい語は、除くパスで外す
- lookbehind と `^` が見る前の文字が、ノードの切れ目（インラインのコードの直後など）で違う。textlint ではノードの
  先頭で `^` が当たり、このツールでは当たらない。この差で残った語は、書き換えた後の hook が見つける（xlc と
  nixfiles の履歴で起きるかは確かめていない）

書き換える場所: ツールは `--source` のリポジトリに書き込まない。`--workdir` にミラー（`original.git`、書き換える前の
控え）と、ミラーのブランチとタグを持つ作業ツリーつきのリポジトリ（`rewritten`）を作り、そこで 2 段を実行する。
`commit-refs rewrite-history` が、作業ツリーを持ち、ほかの worktree の無いリポジトリを前提にするため（ADR-0013 の
「始める前に止める」）。元の番号から最後の番号への対応表を `--workdir` の `commit-map` に書く。

生きているリポジトリへの反映は、worktree ごとに判断が要る（未コミットの変更、チェックアウト中のブランチ）ので
スキルに書く。`rewritten` から fetch でオブジェクトを取り、`git update-ref --stdin` の 1 つのトランザクションで、
各ブランチとタグを「元の番号のままなら」の条件つきで移す。古いオブジェクトの掃除（`reflog expire` と `gc`）は
取り消せないので、issue にしてユーザーが決める。

履歴からパスを消す処理（2026-10-11 の xlc の `examples/*openmc-*.msgpack`）は入れない。その回だけのもので、
必要なら filter-repo を先に別に実行し、その commit-map で `commit-refs rewrite-history` を実行すれば済む（ADR-0013）。

### 帰結

- 良い点: 規則の出典が prh.yml の 1 つになり、書き換えた履歴は hook の辞書の検査を通る
- 良い点: 番号の付け直しは 1 回で終わり、規則は commit-refs の 1 か所にある
- 良い点: 照合が通らなければ止まり、元のリポジトリは変わらない
- 悪い点: prh の JavaScript の正規表現を Python に書き直すので、書き直していない書き方を辞書に足すと、このツールが止まる
  （止まれば、その書き方の読み替えを足す）
- 悪い点: textlint は Markdown のコードを見ないが、このツールは直す。語を例に挙げる文書は、除くパスで外す
- 悪い点: UTF-8 として読めないテキストのブロブは直さない（数を `NOT_UTF8:` に出す）。UTF-8 として読めない本文が
  あると止まる

### 確認

- `modules/home/parts/history-rewrite/test_prh_rules.py`: ja-terms の辞書と specs を読む、長さの違う lookbehind と `^`、
  `\r` の後の `^`、`.` が行の終わりに当たらない、lookahead の `\n?`、固定の文字列、patterns、`$N`、expected と同じ
  一致を直さない、元の文字列の上で集める、重なりの扱い、`prh:disable` を文として読む、前の段落を lookbehind が
  見ない、扱わない書き方で止まる
- `modules/home/parts/history-rewrite/test_history_rewrite.py`: 一時的なリポジトリ（2 本のブランチ、注釈つきと軽量の
  タグ、番号を書いた文書と本文、表、コードのコメント、バイナリ、symlink、除くパス、語を手で直しただけの
  コミット、署名つきのタグと tagger の無いタグ、encoding の見出しのあるコミット、UTF-8 でないテキスト、コードフェンスの
  中の表、CRLF の表）を書き換え、語が直る、表が揃う、番号が最後の番号になる、すべての元のコミットが対応する、元のリポジトリが
  変わらない、同じ辞書の textlint が書き換える前は語を見つけ書き換えた後は見つけない、変換と違う履歴を照合が止める。transform は
  無いファイルで止まり、symlink を変えない
- テストは nixfiles の今の prh.yml（main の 8d6261ea、100 の規則）を読み、どの規則の specs も通る。PR #413 の時点の
  prh.yml（35 の規則）も、specs を通って読めることを手で確かめた（2026-10-21）
- nixfiles の clone（2026-10-21、main の 8d6261ea の 2634 コミット、同じ時点の辞書。辞書と辞書のテストを除くパスに
  渡した）で、語の段と照合と第 2 段が 17.6 秒で通った（1443 のブロブと 278 の本文を直した）。このブランチの
  `history-rewrite transform` を同じ辞書で ADR とスキルに当てた結果も、textlint（prh）の指摘が 0 になった

## 補足

xlc の `references/history-rewrite/`（`callback2.py`、`cb_lib.py`、`iterate3.sh`、`msgcallback.py`、`verify2.py`）が元に
なったツール。照合の方式（元の全コミットと 1 対 1 で、メタデータ、親、本文、すべてのファイル）は `verify2.py` を引き継いだ。

実装の途中で試して変えた版:

- **段落の区切りを prh の正規表現のまま当てる版**: prh の `/([^]*?)\n{2,}/g` を Python の re で
  `([\s\S]*?)\n{2,}` として当てると、区切りの無い長い末尾で開始の位置ごとに読み直し、長さの 2 乗の時間がかかった。
  nixfiles の履歴の 3656 のブロブで、語の変換だけで 271 秒（cProfile で `_paragraphs` が 270.6 秒）。区切りの
  `\n{2,}` だけを探して切る形にした（同じ段落になる）
