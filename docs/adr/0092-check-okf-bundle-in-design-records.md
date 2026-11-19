---
type: ADR
status: accepted
date: 2026-11-17
requires: [ADR-0009]
supersedes: []
superseded-by:
issues: []
---

# ADR-0092: docs/ を OKF のバンドルとして design-records check で検査する

## 背景

OKF（Open Knowledge Format）v0.2 は、マークダウンと YAML のフロントマターだけで知識を表す形式で、概念の文書に `type` を
必須とし、各ディレクトリの `index.md` で内容を一覧にする（SPEC §3、§4.1、§8）。NC リポジトリの `docs/` はこの形で書いている
（仕様の原典は https://github.com/GoogleCloudPlatform/open-knowledge-format の `SPEC.md`、コミット 8a8ff87b、
2026-11-19 取得）。このリポジトリの `docs/`（`adr/` の 76 件と `windows.md`）には `type` も
`index.md` も無く、将来の `docs/issues/` や `docs/plans/` を同じ形で書く決まりも無い。

ADR と issue の形を検査するのは CLI の `design-records`（ADR-0009）で、プロジェクトの pre-commit が呼ぶ。OKF の形も
ここで検査すれば、同じ hook で違反を止められる。

ADR の `status` は `proposed`、`accepted` など 7 値で、OKF の `status`（`draft`、`stable`、`deprecated`）と値が違う。
OKF は未知のキーと値を許す（SPEC §11）。

## 決定の要因

- `type` と `index.md` が揃っていないことを、コミットのたびに機械的に見つける
- 自分以外のプロジェクトや、OKF を使わないプロジェクトの `design-records check` を壊さない
- 既存の ADR の意味（`status` の値）を変えない
- 新しい文書を足すときの手作業を増やさない

## 検討した案

- 案 1: `design-records check` に OKF の検査を足す。`docs/index.md` のフロントマターに `okf_version` を持つリポジトリだけを
  対象にする（オプトイン）
- 案 2: OKF の検査だけをする別の CLI を作る
- 案 3: 案 1 のうえで、ADR の `status` の値を OKF の値に置き換える（`accepted` → `stable` など）
- 案 4: 案 1 のうえで、ADR の `status` のキー名を別名に改名する（別のトピックとして後で行う）

## 決定と理由

案 1 を採る。

- `docs/index.md` に `okf_version` があるリポジトリだけを対象にする。ほかのリポジトリの `docs/` は今までどおり見ない
- 検査の規則は OKF v0.3 に従う（節の番号は v0.3 のもの。仕様の原典は
  https://github.com/open-knowledge-format/open-knowledge-format の `SPEC.md`、コミット 527e81af、2026-11-19 取得）。
  `okf_version` は、規則を知っている `"0.2"` と `"0.3"` を通し、ほかの値を違反にする。`"0.2"` のバンドルには、ルートの
  `index.md` の規則だけを v0.2 のものにする
- 対象の `docs/` 以下の、追跡している `.md` のうち、名前が `.` で始まるファイルとディレクトリの下のものを除いた全部を
  1 つのバンドルとして（§3.2）、次を違反（error）にする
  - `index.md` と `log.md` 以外の文書は、フロントマターを読め、空でない `type` を持つ。OKF v0.3 は `type` の無い `.md` を
    概念でないファイルとして許し、検査には注意を勧めるだけ（§3.2）だが、このリポジトリの決まりとして違反にする
  - ADR は `type: ADR`、`docs/issues/` の issue は `type: Issue`
  - 文書か下位のディレクトリを持つディレクトリに `index.md` がある
  - 下位のディレクトリの `index.md` はフロントマターを持たない。バンドルのルートの `index.md` のフロントマターは、
    `okf_version`、`title`、`description`、`license` と拡張のキー（§8.1）。`"0.2"` のバンドルでは `okf_version` だけ
  - `index.md` の `* [題](パス) - 説明` の行のリンク先が実在し、同じディレクトリの文書と下位のディレクトリがすべて載っている。
    題は Markdown のリンクの文字列として、バックスラッシュのエスケープとコードスパンの中を飛ばし、対になった角括弧を含めて読む（§8）。
    Markdown のリンクとして読めない行（対にならない `[`、説明まで続くバッククォート）は、前と同じく題の中の最初の `](` で切って読む
    リンク先はパーセントエンコードを復号し、先頭が `/` ならバンドルのルートから、ほかは索引のディレクトリから解く（§6）。
    バンドルのルートの外と、`.` で始まるパスの下に解けるリンクは、URL と同じく外部のリンクとして見ない
  - `log.md` はフロントマターを持たず（§9）、日付の見出しが `YYYY-MM-DD`
- 索引の題と説明が、リンク先のフロントマターの `title` と `description` と違うときは注意（warning）にする。ADR は
  フロントマターに `title` を持たないので、題は最初の 1 段目の見出しと比べる。題は両方を `design-records index` の書く形に
  揃えて比べる（表示が同じなら合う）。索引の行の題がリンクを含むときも注意にする（Markdown はリンクの文字列の中のリンクを
  許さず、その行はリンクにならない）
- `design-records index` が、索引に載っていない文書と下位のディレクトリの行を足す（説明は文書の `description` から取る。
  リンク先はパーセントエンコードする。題は、文書の題のリンクをその文字列に置き換え（画像は残す）、残る角括弧、閉じない
  バッククォート、末尾のバックスラッシュをエスケープして書く。既にある行とルートのフロントマターは直さない、§8.1）。`index.md` が無ければ作る
- `design-records new` が作る ADR のテンプレートに `type: ADR` を足す。`type` は ADR の任意のキーで、確定した後も直して
  よいキーにする（既存の ADR に足すため）
- ADR の `status` は、この ADR では今のキーと 7 値のままにする。OKF の `status` は文書の鮮度の合図で、ADR の `status` は
  決定の進み具合を表す別のもの。値を OKF の 3 値に寄せると rejected と superseded の区別が消えるので採らず、キー名を別名に
  改名して OKF の `status` と衝突しないようにする（ユーザーの指示、2026-11-17）。改名は、`status` を読む CLI、hook、
  スクリプト、スキルと、ADR を持つほかのプロジェクトの全件に及ぶので、別のトピックとして issue に書き、別の PR で行う

案 2 は、ADR のパスと中身を読む処理とフロントマターの読み取りをもう 1 つの CLI に持たせ、pre-commit の hook も 2 つになる。
案 3 は、値の区別が消える。

### 帰結

- 既存の ADR 76 件に `type: ADR` を足す 1 つのコミットが要る（`type` は凍結する部分ではないので、`AMENDABLE_KEYS` に足す）
- `docs/issues/` の新しい issue には、手で `type: Issue` を足す（`design-records new --kind issue` の下書きは表題の行だけのまま。
  フロントマターの形は各リポジトリが決める、ADR-0018）

### 確認

- `crates/design-records/src/okf.rs` の単体テストと `tests/cli.rs` の `check` と `index` の試験（v0.3 の規則は
  `check_accepts_bundle_metadata_in_a_v0_3_root_index_and_index_keeps_it`、`check_rejects_an_okf_version_it_does_not_know`、
  `check_rejects_front_matter_in_a_log`、`check_and_index_leave_dot_paths_out_of_the_bundle`、
  `check_resolves_index_links_from_the_index_directory_and_slash_from_the_bundle_root`、
  `check_treats_index_links_outside_the_bundle_as_external`、`check_treats_index_links_to_dot_paths_as_outside_the_bundle`、
  `check_reads_an_okf_version_followed_by_a_yaml_comment`、`index_percent_encodes_link_targets_and_check_decodes_them`、
  `index_unwraps_a_link_in_a_title_and_check_reads_the_entry`、
  `index_escapes_brackets_and_backslashes_in_titles_and_check_reads_them`、
  `check_lists_an_entry_whose_title_has_a_link_but_warns_about_it`、
  `index_escapes_a_lone_backtick_so_that_a_description_cannot_close_it`、
  `check_still_lists_hand_written_entries_with_a_lone_bracket_backtick_or_image`）
- このリポジトリの `docs/` で `design-records check` が違反なしで通る

## 各案の長所と短所

- 案 1: 長所は、pre-commit の設定を変えずに効き、オプトインで他を壊さない。短所は、`design-records` の責務が ADR と issue の
  ほかに広がる
- 案 2: 長所は、責務が分かれる。短所は上のとおり、読み取りの重複と hook の追加
- 案 3: 長所は、OKF の値と揃う。短所は、既存の記録とスキルの全面的な書き換え
- 案 4: 長所は、決定の進み具合の区別を残して OKF の `status` と衝突しない。短所は、`status` を読む箇所の全部の書き換え（別の PR にする）

## 変更

- 2026-11-19: 検査の規則を OKF v0.3 に上げた（決定と理由の、規則の版と `okf_version` の値、`.` で始まるパス、`type` の
  決まり、`index.md` のフロントマター、リンクの解き方、`log.md`、`design-records index` の項）。前は「`index.md` の
  フロントマターは、バンドルのルートの `okf_version` だけ」で、`okf_version` の値を見ず、v0.2 の規則を当てていた。
  v0.3 に上げるのはユーザーの決定（2026-11-19）で、v0.3 の §13 の変更のうち検査に関わるものを入れた。`"0.2"` も通すのは、
  v0.2 のバンドルを持つほかのリポジトリ（NC のリポジトリ）の `design-records check` を止めないため。知らない版は規則が
  違いうるので、黙って通さず違反にする（§12 は consumer に拒まないよう求めるが、これは書く側の検査）。仕様の本文は
  リポジトリにコピーしない（原典をコミットで固定でき、コピーは片方だけが古くなる。検査が使うのは節の番号だけ）。v0.3 の
  §5.4 は、OKF の `status` の未知の値（ADR の 7 値）を `stable` でないものとし、`superseded_by` を足した。ADR の `status`
  のキーを改名する理由（#261）に加わるが、この変更では改名しない
- 2026-11-19: 索引の行の題の書き方と読み方を決めた（決定と理由の、索引の行、題と説明の注意、`design-records index` の項）。
  前は「`index.md` の `* [題](パス) - 説明` の行のリンク先が実在し、同じディレクトリの文書と下位のディレクトリがすべて
  載っている」で、題の形を決めておらず、index は文書の題をそのまま書き、check は題の中の最初の `](` で行を切っていた。
  題にリンクを含む文書（`# [a](http://x) の件`）で、index の書いた行 `* [[a](http://x) の件](0001-a.md)` を check が
  「索引に載っていない」とした（#330）。§8 の行は Markdown のリンクで、CommonMark はリンクの文字列の中のリンクを許さず、
  この行はほかの Markdown の読み手にもリンクにならないので、index が題のリンクを外して書く。check が入れ子の角括弧を読む
  だけの案は、index の行を check とだけ合わせ、ほかの読み手には壊れた行を残すので採らなかった。題のリンクを外さずに角括弧を
  すべてエスケープする案（`\[a\](http://x) の件`）は、表示が文書の題と違い、xlc のリポジトリが手で書いた行（リンクを
  外した題）とも違うので採らなかった。手で書いた普通の行（角括弧もエスケープも無い題）の読み方と比べ方は変わらず、
  Markdown のリンクとして読めない手書きの行も前の読み方で読む（読めなくすると、その文書を「索引に載っていない」とし、
  index が同じ文書の行を足し直す）
