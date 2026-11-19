---
type: ADR
status: proposed
date: 2026-11-19
requires: [ADR-0014, ADR-0045, ADR-0050, ADR-0074, ADR-0085]
supersedes: []
superseded-by:
issues: ["#316"]
---

# ADR-0096: 崩れた語の辞書を、共通の辞書と、リポジトリの .ja-terms/prh.yml の 2 つに分けて重ねる

## 背景

崩れた日本語の語の辞書は `crates/ja-terms/prh.yml` の 1 つで（114 の規則。2026-11-19 の main の `1cb8d265`）、ja-terms が
ビルドに埋め、どのリポジトリでも同じ規則を当てる（ADR-0014、ADR-0085）。xlc の文書で nominal の訳語を業界で通じる語に直して
辞書で止めたいが、その語は xlc でしか使わないので、辞書を全リポジトリに共通の分とリポジトリごとの分に分け、今の共通の辞書に
ある xlc でしか使わない語も xlc の辞書に移すと決めた（ユーザー 2026-11-19）。

辞書を当てる経路（2026-11-19 の main で `ja-terms`、`ja_terms`、`DICTIONARY` を grep して数えた）:

- `ja-terms staged`: グローバルの pre-commit（`claude-hooks git-hook pre-commit`）がリポジトリの直下で呼ぶ。ステージした中身と、
  ステージした `.textlintignore` を読む
- `ja-terms commit-msg`: commit-msg の git hook がリポジトリの直下で、claude-hooks の git commit と git merge のルール
  （ADR-0050、ADR-0073）がコマンドの行き先のディレクトリで呼ぶ
- `ja-terms message`: claude-hooks の git merge のルール（ADR-0058）が、コマンドの行き先のディレクトリで呼ぶ
- `ja-terms check`: /retro の `memory-index.sh`（ADR-0074）が、呼んだセッションの今のディレクトリで、リポジトリの外のメモリの
  ファイルを渡して呼ぶ。`dictionary-preview.sh` は一時のディレクトリで、`--dictionary` を渡して呼ぶ
- `history-rewrite` の `run`、`transform`、`sync-worktrees`（ADR-0045）: ja-terms のライブラリを呼ぶ。`--dictionary` が無ければ
  埋めた辞書。`--dictionary` と `--extra-rules` は渡した順に並べる（同じ expected の規則を外さない）

ADR-0085 の時点で呼ぶ所に挙がっていた `~/.claude/bin/ja-terms-files` は、今は無い（`ja-terms check` に置き換わった）。

prh と textlint の慣習（textlint-rule-prh 6.0.0 と、それが使う prh 5.4.4 の `lib/index.js`、`lib/engine.js`、
`lib/textlint-rule-prh.js`。2026-11-19 に nix store の textlint-rule-prh-6.0.0 で読んだ）:

- 辞書のファイルの名前は `prh.yml`。prh の CLI は、`--rules` が無ければ対象のディレクトリから根まで上って `prh.yml` を探す
  （`getRuleFilePath`）
- 辞書は `imports` でほかの辞書を読み込める。読み込んだ側の規則が先に並び、読み込まれた辞書の規則のうち、同じ expected の
  規則が既にあるものは外す（`Engine.merge`）
- textlint-rule-prh の `rulePaths` は複数の辞書を並べ、前の辞書から順に `Engine.merge` でつなぐ（前の辞書が勝つ）

ja-terms は辞書の `version` と `rules` の外の項目（`imports`、`targets` など）を読まずに止める（`crates/ja-terms/src/lib.rs` の
`parse_rules`）。追跡しているファイルに `prh.yml` を持つリポジトリは、この PC の `~/github` と `~/nix` の下では
claude-harness（`crates/ja-terms/prh.yml`）だけだった（2026-11-19、`git ls-files`）。

## 決定の要因

- リポジトリに固有の語を、そのリポジトリでだけ止めること。辞書の無いリポジトリでは今と同じに動くこと
- 同じリポジトリのどの経路（pre-commit、commit-msg、claude-hooks、/retro、履歴の書き換え）も同じ辞書の組を当てること。
  worktree では、その worktree の辞書を当てること
- 読めない辞書を黙って飛ばさないこと
- prh の辞書の形と、辞書を重ねる意味を変えないこと（ADR-0085 の要因）
- 共通の辞書から語を移す間に、検査の抜ける期間を作らないこと

## 検討した案

置き場と名前:

- 案 A: リポジトリの直下の `prh.yml`（prh の CLI の既定の名前）
- 案 B: リポジトリの直下の `.ja-terms.yml`（ツールの名前の dot ファイル。`.textlintignore` と同じ並び）
- 案 C: リポジトリの直下の `.ja-terms/prh.yml`（ツールの名前のディレクトリに、prh の名前で置く）
- 案 D: 設定のファイル（textlint の `.textlintrc` の `rulePaths` のように、辞書のパスを並べる）を置き、そこから辞書を読む

見つけ方:

- 案 1: 決まったパスを、作業ツリーの直下（`git rev-parse --show-toplevel`）から読む
- 案 2: prh の CLI と同じく、今のディレクトリから根まで上って探す

重ね方:

- 案 X: リポジトリの辞書を先に、共通の辞書を後に、prh の `Engine.merge` でつなぐ（同じ expected の規則は、リポジトリの辞書の
  ものだけを残す。リポジトリの辞書が共通の辞書を prh の `imports` で読み込んだのと同じ）
- 案 Y: 共通の辞書を先に、リポジトリの辞書を後に `Engine.merge` でつなぐ（共通の辞書が勝つ）
- 案 Z: `Dictionary::append` で並べる（同じ expected の規則も両方を残す）

## 決定と理由

採用: 案 C、案 1、案 X。

- リポジトリの辞書は、作業ツリーの直下の `.ja-terms/prh.yml`。形は共通の辞書と同じ prh の辞書（`version` と `rules`）
- どのリポジトリの辞書を読むかは、ja-terms を実行する今のディレクトリを含む作業ツリー（`git rev-parse --show-toplevel`）で
  決める。worktree はそれぞれの直下のファイルを読む。作業ツリーの外（今のディレクトリから根までに `.git` が無く、`GIT_DIR` も
  無い）なら、共通の辞書だけを当てる
  - `ja-terms staged` は、ステージした `.ja-terms/prh.yml`（インデックスの版）を読む。ステージした `.textlintignore` を読むのと
    同じく、コミットに入る中身を、そのコミットに入る辞書で検査する
  - ほかの経路は作業ツリーのファイルを読む。claude-hooks の git commit と git merge のルールはコマンドの行き先のディレクトリで、
    git hook はリポジトリの直下で ja-terms を呼ぶので、コミットするリポジトリの辞書になる。/retro の `memory-index.sh` は、
    呼んだセッションの今のディレクトリのリポジトリの辞書を当てる（メモリはそのリポジトリのセッションが読み込み、そこから
    そのリポジトリのコミットにコピーされる）
  - `history-rewrite` は、`--dictionary` が無ければ、`run` が `--source` のリポジトリの、`transform` が今のディレクトリの
    リポジトリの辞書を読み、`run` の記録（`DIR/words`）に残す。`sync-worktrees` は記録の辞書を使う。`--repo-dictionary FILE` で
    明示もできる
- 重ね方は、リポジトリの辞書の規則を先に、共通の辞書の規則のうち、同じ expected の規則がリポジトリの辞書に無いものを後ろに
  並べる（prh の `Engine.merge`）
- `--dictionary` を渡したときは、渡した辞書だけを当て、リポジトリの辞書を重ねない（今と同じ）。`dictionary-preview.sh` は、渡した
  辞書の版どうしの差を見るためのもので、一時のディレクトリで ja-terms を呼ぶので、リポジトリの辞書を重ねない
- 読めない辞書（読めないファイル、YAML の誤り、扱わない項目、specs の失敗）と、作業ツリーの中で git が失敗したときは、検査
  できなかったとして止める（終了コード 2）
- 共通の辞書の規則を移すときは、先にリポジトリの辞書に同じ規則を足し、その後に共通の辞書から消す。案 X では同じ expected の
  規則はリポジトリの辞書のものだけが残るので、両方にある間も検査は 1 度で、抜ける期間も重なる報告も無い。xlc でしか使わない
  規則の一覧と、消す作業は #316 に書く（この ADR を実装する PR では共通の辞書を変えない）

理由:

- 案 C は、ja-terms のための辞書だと名前で分かる。案 A は、ほかの目的で `prh.yml` を置いたリポジトリ（textlint で使い、
  `imports` や `targets` を持つもの）でも、グローバルの pre-commit がそれを読み、扱わない項目で止まるか、そのリポジトリが
  ja-terms に当てるつもりの無い規則を当てる。グローバルの hook はどのリポジトリでも走るので、置いたことが辞書を当てる意思に
  なる名前にする。案 C は prh の名前を残すので、形が prh の辞書だと分かり、`ja-terms staged` の `**/prh.yml` の除外と、
  /history-rewrite の `--exclude` の例がそのまま当たる。案 B は形が名前に出ず、除外を足す必要がある
- 案 D は、辞書の数が 1 つの今は、読むファイルが 1 段増えるだけで要らない
- 案 1 は、リポジトリの外のファイルを読まない。案 2 は、作業ツリーの外（ホームのディレクトリ）の `prh.yml` や、
  claude-harness の `crates/ja-terms/` の下で呼んだときの共通の辞書そのものを、リポジトリの辞書と読む
- 案 X は prh の `imports` と同じ意味で、ADR-0085 の補足のとおり、複数の辞書を prh の `Engine.merge` でつなぐ（ただ並べない）。
  リポジトリの辞書が、そのリポジトリの文脈に合わない共通の規則を、同じ expected の規則で置き換えられる。案 Y では置き換え
  られない。案 Z は、移す間に同じ規則が両方にあると 2 度当たり、同じ語に違う直しを当てる規則が両方にあると、どちらを報告するか
  が規則の順で決まる
- 作業ツリーの外を共通の辞書だけにするのは、`dictionary-preview.sh` と、リポジトリの外のディレクトリで呼んだ /retro が今と
  同じに動くため。`.git` も `GIT_DIR` も無いことを、git の失敗の文言（言語の設定で変わる）でなく、ファイルの有無で見る

### 帰結

- 良い点: リポジトリに固有の語を、ほかのリポジトリの誤検知を気にせずに足せる。辞書の無いリポジトリは今と同じ
- 良い点: 共通の辞書から規則を移す間も、検査の抜けと重なりが無い
- 悪い点: ja-terms が、`--dictionary` の無い作業ツリーの中の呼び出しで、git を 1 度多く起動する
- 悪い点: リポジトリの辞書が共通の辞書と同じ expected の規則を持つと、共通の規則は当たらない。置き換えるつもりの無い同じ
  expected の規則を足すと、共通の規則が外れる（prh の `imports` と同じ）
- `dictionary-preview.sh` は、リポジトリの辞書と共通の辞書を重ねた結果を見せない（渡した 1 つの辞書の版どうしの差を見る）

### 確認

- ja-terms の `check`、`message`、`commit-msg`、`staged` が、`.ja-terms/prh.yml` のあるリポジトリでその規則を当て、作業ツリーの
  外と辞書の無いリポジトリで共通の辞書だけを当て、worktree でその worktree の辞書を当て、読めない辞書で 2 で終わること。
  同じ expected の規則は、リポジトリの辞書のものだけが当たること（`crates/ja-terms/tests/repo_dictionary.rs`）
- claude-hooks の git commit と git merge のルールが、行き先のリポジトリの辞書で止めること（`crates/claude-hooks/tests/merge-ja-terms.sh`）。
  /retro の `memory-index.sh` が、今のディレクトリのリポジトリの辞書で `TERMS:` を出すこと（`crates/claude-hooks/tests/script_memory_index.rs`）
- `history-rewrite` の `transform` と `run` が、リポジトリの辞書を当て、`run` の記録に残すこと（`crates/history-rewrite/tests/`）
