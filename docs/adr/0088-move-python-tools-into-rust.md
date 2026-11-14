---
status: proposed
date: 2026-11-14
requires: [ADR-0045, ADR-0082, ADR-0084, ADR-0085, ADR-0087]
supersedes: []
superseded-by:
issues: ["#101"]
---

# ADR-0088: Python のツール（history-rewrite、usage-report、kept-sentences）を Rust に移し、filter-repo と Python を依存から外す

## 背景

Issue #101 の段階 7 で、Python のツールを Rust に移す（ADR-0082）。対象は次の 4 本（2026-11-14、`aaa93149`、`wc -l`）。

- `modules/home/parts/history-rewrite/history_rewrite.py`（1,119 行）と `prh_rules.py`（378 行）: 履歴の語を辞書で直し、
  番号を `commit-refs rewrite-history` で付け直す CLI（ADR-0045）。`package.nix` が `python3.withPackages`
  （git-filter-repo、pyyaml、regex）のラッパーにし、PATH に `history-rewrite` として入れる。スキル `history-rewrite` は
  `history-rewrite run`、`transform`、`sync-worktrees` を名前で呼ぶ。第 1 段は git-filter-repo の Python の callback
  （file-info-callback、commit と tag の callback）で、テキストのブロブと本文に辞書を当てる。テストは
  `test_history_rewrite.py`（1,080 行。多くはラッパーの CLI を子プロセスで呼び、照合の拒否の 2 件は `Verifier` を直に呼ぶ）と
  `test_prh_rules.py`（274 行）
- 隣の `fix-markdown-lint.py`（347 行）: history-rewrite が表の揃え直しのために Python のモジュールとして読む。中身は
  `claude-hooks script fix-markdown-lint`（`claude-hooks/src/script/fix_markdown_lint.rs`）に移し済みで、.py は段階 7 で
  消す（ADR-0087）。それまで `.claude/tests/scripts/language-checks.sh` が両方の表の桁を突き合わせる
- `.claude/scripts/usage-report.py`（386 行）: 会話の記録から区間の費用を出す。/retro の文が `~/.claude/scripts/usage-report.py` の
  パスで呼ぶ（`skills/retro/SKILL.md` の allowed-tools と本文）
- `.claude/scripts/kept-sentences.py`（146 行）: 縮めた規則の文書の、元の版に無い文を出す。/delegate の文が
  `~/.claude/scripts/kept-sentences.py` のパスで呼ぶ（`skills/delegate/SKILL.md` の allowed-tools と
  `references/working.md`）。`.claude/README.md` の表も名前を挙げる

関わる事実:

- 辞書を prh と同じ意味で当てる Rust の実装は ja-terms のライブラリにあり、履歴の書き換えの mode（全文に段落ごとに当てる）は
  `Dictionary::apply` で持つ（ADR-0085。`prh_rules.py` の置き換えは段階 7 に残していた）。今の `Dictionary::load` は
  ファイルを 1 つ読み、history-rewrite は `--dictionary` と `--extra-rules` で複数の辞書の規則を順に並べる
- `commit-refs rewrite-history`（Rust）は、`git fast-export --no-data` の流れを読み、`git cat-file --batch` でブロブを読み、
  `git fast-import` に渡して履歴を書き換える。タグの署名の外し方、cat-file の読み方、対応表の形は、history_rewrite.py と
  「言語が違うので共有できない」として対にして保っている（history_rewrite.py の `SIGNATURE_MARKERS` の上の注記）
- `fix_markdown_lint.rs` は `regex` と標準ライブラリだけを使い、claude-hooks のほかのモジュールを呼ばない
- history-rewrite の `--exclude` は Python の `PurePosixPath.full_match` の書き方（`**` がディレクトリをまたぐ）
- usage-report が扱う日時は、時差つきの ISO 8601（記録の timestamp の `Z`、`--since` と `--until`、`git log --format=%cI`）と、
  JST（+09:00 の固定の時差）の表示と今日の区切りだけ

決めること: history-rewrite の第 1 段の書き換えの方式、置き場と依存（表の揃え直しの関数と、commit-refs と対にしている部品）、
usage-report と kept-sentences の置き場と呼ばれ方、テストの移し方。

## 決定の要因

- Windows で Python を入れずに動かす（ADR-0082）。filter-repo は Python のプログラムなので、callback を Rust にしても
  filter-repo を使う限り Python が要る
- 同じ判定の実装を 1 つにする（辞書の当て方、表の揃え方、署名の外し方）
- Windows で入れるものを増やさない（ADR-0084 の手順は crate ごとに `cargo install` する）
- スキルの文と hook のルールが名前で呼ぶものは、名前を変えるなら同じ PR で揃える（ADR-0087）

## 検討した案

history-rewrite の第 1 段:

- 案 1: Rust で `git fast-export --no-data` の流れを読み、直したブロブと本文を `git fast-import` に渡す（filter-repo の
  2.47.0 が fast-export と fast-import に渡す設定と同じにする）
- 案 2: filter-repo を子プロセスで呼び、変換だけを Rust の CLI にする（filter-repo の `--blob-callback` などに、Rust の
  CLI を起動する Python の 1 行を渡す）
- 案 3: `commit-refs rewrite-history` に辞書の適用を足し、1 段にする（ADR-0045 の案 C3）

history-rewrite の置き場:

- 案 A: 新しい crate `history-rewrite`（`modules/home/parts/history-rewrite/`。Python の版と同じ場所と名前）
- 案 B: `claude-hooks` のサブコマンドにし、PATH の `history-rewrite` はラッパーのスクリプトにする

表の揃え直しの関数:

- 案 ア: `fix_markdown_lint.rs` を新しいライブラリの crate（`fix-markdown-lint`）に出し、claude-hooks と history-rewrite の
  両方が path で依存する
- 案 イ: claude-hooks にライブラリの target を足し、history-rewrite が claude-hooks に依存する
- 案 ウ: history-rewrite が表の関数をコピーして持つ

commit-refs と対にしている部品（署名の外し方、cat-file の読み方）:

- 案 甲: commit-refs のライブラリ（`commit_refs`）に出し、history-rewrite が path で依存する
- 案 乙: 今のとおり対にして、それぞれに持つ

`--exclude` の書き方:

- 案 い: `globset`（ripgrep の crate。Rust の glob の事実上の標準）を、`/` をまたがない `*` の設定で使う
- 案 ろ: `full_match` と同じ照合を手で書く

usage-report と kept-sentences:

- 案 C: `claude-hooks script <名前>` に移し、`lib/claude-hooks-script.sh` を読んで呼ぶだけの `<名前>.sh` を足し、スキルの文と
  allowed-tools を `.sh` に付け替えて .py を消す（ADR-0087 の `fix-markdown-lint` の案 D と同じ形）
- 案 D: 新しい crate にする
- 案 E: .py の名前を残し、中身を `claude-hooks script` を呼ぶだけの Python にする

usage-report の日時:

- 案 a: `chrono` か `jiff` の crate を足す
- 案 b: 時差つきの ISO 8601 の読み取りと、固定の時差の暦の変換を手で書く

## 決定と理由

採用: 案 1、案 A、案 ア、案 甲、案 い、案 C、案 b。

- history-rewrite は Rust の crate（案 A）にし、CLI の名前、サブコマンド、引数、出力の形、終了コードを Python の版と
  同じにする。辞書は ja-terms のライブラリ（`Dictionary::apply`）で当て、複数の辞書の規則を渡した順に並べる読み方を
  ja-terms に足す。第 2 段と worktree の番号の付け直しは、今のとおり `commit-refs rewrite-history` と `commit-refs remap` を
  子プロセスで呼ぶ
- 第 1 段（案 1）: fast-export と fast-import を直に扱う。fast-export の設定は filter-repo 2.47.0 の `fep_cmd` に、
  ブロブを流さない `--no-data` を足したもの（ブロブは cat-file で読み、直したものだけを流れに書く）。filter-repo の
  `--prune-empty=never`、`--prune-degenerate=never`、`--preserve-commit-hashes`、`--preserve-commit-encoding`、`--no-gc` に
  当たる振る舞い（コミットを消さない、本文の番号に触らない、本文の符号化を変えない、repack しない）は、流れを
  書き換えずに渡すことで同じになる。対応表（`old new`）は fast-export の `original-oid` と fast-import の
  `--export-marks` から作り、照合（ADR-0045）は今の Python の版と同じ項目を確かめる
- 表の揃え直し（案 ア）: `fix_markdown_lint.rs` をライブラリの crate `fix-markdown-lint`（`modules/home/parts/fix-markdown-lint/`）に
  移す。`claude-hooks script fix-markdown-lint` と `staged-checks` はそれを呼ぶ。history-rewrite は fence の判定、表の行の
  判定、`format_table` を呼ぶ
- 署名の外し方と cat-file の読み方（案 甲）は commit-refs のライブラリに移し、commit-refs の `rewrite-history` と
  history-rewrite が同じ関数を呼ぶ
- `--exclude`（案 い）: `globset` で、`*` と `?` が `/` をまたがない設定（`literal_separator`）にする
- usage-report と kept-sentences（案 C）: `claude-hooks script usage-report` と `claude-hooks script kept-sentences` に移し、
  `~/.claude/scripts/usage-report.sh` と `kept-sentences.sh` を呼ぶだけの sh にする。スキルの文と allowed-tools と
  `.claude/README.md` を付け替え、.py を消す。`.claude/tests/scripts/` の sh のテストは、呼ぶだけの sh を呼ぶ配備の
  確認として残す（ADR-0087 の「テスト」）
- usage-report の日時（案 b）: `--since`、`--until`、記録の timestamp、`%cI` を、時差つきの ISO 8601 として読み、UNIX の時刻で
  比べる。表示と今日の区切りは +09:00 の固定の時差で暦に直す
- テスト: Python のテストは、同じ振る舞いを Rust の結合テスト（`history-rewrite/tests/`、claude-hooks の `tests/script_*.rs`）に
  移す。`test_prh_rules.py` の振る舞いは ja-terms の `apply` のテストに移す（ja-terms に既にあるものは足さない）。
  付け替える PR で、Python の `test_history_rewrite.py` を Rust の history-rewrite に向けて流し、通ることを PR に書く
- PR の順: kept-sentences、usage-report、fix-markdown-lint の crate への切り出し、commit-refs の部品の切り出し、
  history-rewrite の Rust の版（transform、run、sync-worktrees）、package.nix の付け替えと Python の版、`fix-markdown-lint.py`、
  `language-checks.sh` の .py との照合、`scripts/run-python-tests.sh` の history-rewrite の部分を消す PR。Rust の版の
  history-rewrite と fix-markdown-lint の crate は、Windows の CI（`windows.yml`）の matrix に足し、README の「Windows で使う」の
  `cargo install` に history-rewrite を足す

理由:

- 案 1: 案 2 は filter-repo を使うので Python が残り、移す目的にならない。案 3 は ADR-0045 が採らなかった案で、語の変換と
  番号の付け直しが 1 段になると、その間の照合（変換だけを当てた履歴との 1 対 1 の照合）ができない。
  fast-export と fast-import の読み書きは commit-refs が既に持つので、同じ書き方で足りる
- 案 A: スキルと PATH の名前をそのまま使え、Windows でも `cargo install` で `history-rewrite` が入る。案 B は、PATH の名前の
  ためにラッパーのスクリプトが要り（Windows では Git Bash の外から呼べない）、履歴の書き換えのテストと依存（ja-terms）が
  毎回の hook のバイナリに入る
- 案 ア: 案 イ は history-rewrite のビルドに tree-sitter などの hook の依存を持ち込む。案 ウ は表の揃え方が 2 つになり、
  今の Python と Rust の照合（ADR-0087）と同じ費用を続ける。ライブラリの crate は `cargo install` で入れるものを
  増やさない（path の依存として一緒にビルドされる）
- 案 甲: 2 つの実装が Rust になったので、対にして保つ理由（言語の違い）が無くなった。案 乙 は、片方を直して
  もう片方を忘れると、照合の期待と書き換えが食い違う
- 案 い: `globset` は事実上の標準で、`**` の扱いは `full_match` と同じ（パスの成分の全体の `**` がディレクトリをまたぐ）。
  案 ろ は、手で書く照合（文字クラス、エスケープ）を保守することになる
- 案 C: 呼び出し元は retro と delegate のスキルの文と README の表だけで、PreToolUse のルールは名前で見分けない
  （2026-11-14、`grep -rn 'usage-report\|kept-sentences' .claude modules`）。案 D は Windows で入れる crate を増やす。
  案 E は Windows で呼ぶのに `python3` が要る
- 案 b: 扱う形は時差つきの ISO 8601 と固定の時差だけで、時間帯の表（夏時間）を引かない。暦の変換は数十行で、
  crate を足すより保守の費用が小さい

### 帰結

- 良い点: Python が、ハーネスの配る CLI とスクリプトの依存から外れる（`scripts/run-python-tests.sh` が流すほかの Python の
  テストは対象の外）。Windows でも history-rewrite が使える
- 良い点: 表の揃え方、辞書の当て方、署名の外し方が、それぞれ 1 つの実装になる
- 悪い点: filter-repo の振る舞い（fast-export と fast-import の設定、仮の tagger など）を自分で持つ。git の版を上げて
  fast-export の出力が変わったときは、照合が止めるので、それを見て合わせる
- 悪い点: usage-report と kept-sentences の判定を変えるたびに、claude-hooks のビルドとテストが要る（ADR-0087 の帰結と同じ）
- ADR-0045 の決定（Python で書き、filter-repo の callback で第 1 段を流す）は、Python の版を消す PR で「## 変更」を書く。
  ADR-0087 の「二重の実装の期間」は、`fix-markdown-lint.py` を消す PR で帰結を直す

### 確認

- history-rewrite の Rust の結合テストが、`nix flake check` と Windows の CI で通る。付け替える PR で、Python の
  `test_history_rewrite.py` が Rust の版に対して通る
- 付け替えた後、`git ls-files '*.py'` に history-rewrite、usage-report、kept-sentences、fix-markdown-lint の .py が無く、
  `history-rewrite/package.nix` が Python を参照しない
- `~/.claude/scripts/usage-report.sh` と `kept-sentences.sh` が、`.claude/tests/scripts/` の今のテストの期待値で通る
