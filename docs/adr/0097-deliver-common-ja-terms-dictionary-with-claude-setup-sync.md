---
type: ADR
status: proposed
date: 2026-11-19
requires: [ADR-0045, ADR-0084, ADR-0085, ADR-0093, ADR-0096]
supersedes: []
superseded-by:
issues: []
---

# ADR-0097: ja-terms の共通の辞書を、ビルドに埋めず、claude-setup sync がホームの下に配るファイルから読む

## 背景

崩れた日本語の語の共通の辞書 `crates/ja-terms/prh.yml` は、ja-terms がビルドに埋めている
（`crates/ja-terms/src/lib.rs` の `pub const DICTIONARY: &str = include_str!("../prh.yml")` と `Dictionary::common`。
2026-11-19 の main の `24d56747`）。ja-terms の 4 つのサブコマンド（`check`、`message`、`commit-msg`、`staged`）と
history-rewrite は、`--dictionary` が無ければこの辞書を当て、リポジトリの辞書（`.ja-terms/prh.yml`）があればその後ろに
重ねる（ADR-0096）。

埋めているので、辞書に規則を 1 つ足すと、マージの後に rebuild（ja-terms、claude-hooks、history-rewrite の作り直しと
テスト）を待たないと効かない。Windows では、clone を更新して `cargo install` を再実行するまで効かない。このリポジトリには、
ファイルをホームの下に配る仕組みが既にある: `claude-setup sync` の `HOME_FILES`（`crates/claude-setup/src/sync.rs`）が、
ソースのリポジトリのファイルをホームの下の決まったパスに置く（ADR-0093。今は markdownlint の設定の 1 行）。rebuild の
activation（`nix/claude-code.nix` の `home.activation.claudeCodeSetup`）も、flake のソース（store）を渡して同じ
`claude-setup sync` を呼ぶ。`claude-sync`（引数なし）は、`~/nix/claude-harness` の clone を `git pull --ff-only` で
最新にしてから同期する。

共通の辞書を埋め込みのままにするか、ホームの下にファイルとして配るかを相談し、このリポジトリの配る仕組みで配る形に
変える方を進めると決めた（ユーザー 2026-11-19）。規則を足してから効くまでが、マージと rebuild から、マージと
`claude-sync` になる。

共通の辞書を当てる経路（ADR-0096 の背景と同じ。2026-11-19 の main で確かめた）:

- `ja-terms staged`（グローバルの pre-commit）、`ja-terms commit-msg`（commit-msg の git hook と claude-hooks の git commit
  と git merge のルール）、`ja-terms message`（claude-hooks の git merge のルール）、`ja-terms check`（/retro の
  `memory-index.sh`）。どれも `Dictionary::for_dir` か `Dictionary::layered` から `Dictionary::common` を呼ぶ
- `history-rewrite` の `run`、`transform`、`sync-worktrees`: `--dictionary` が無ければ `Transformer::new` が
  `Dictionary::common` を呼ぶ。`run` は語の指定を `DIR/words` に記録し、`sync-worktrees` は記録の指定で同じ変換を
  作り直す（ADR-0045）。記録には、共通の辞書は書かれない（埋めた辞書は、ビルドが同じなら同じ）
- `dictionary-preview.sh` は `--dictionary` を渡すので、共通の辞書を読まない

ホームに配った辞書が無い環境（2026-11-19 に、テストと CI の設定を読んで数えた）:

- ja-terms と history-rewrite の `cargo test`（Linux と Windows の CI、Nix のパッケージの checkPhase、手元）。既定の辞書を
  当てるテスト（ja-terms の `tests/check.rs` の 1 件、`tests/message.rs`、`tests/repo_dictionary.rs`、`tests/staged.rs`、
  history-rewrite の `tests/transform.rs`、`run.rs`、`sync.rs`）が、今は埋めた辞書を使う
- claude-hooks の Nix のパッケージの installCheckPhase と Windows の CI の `cargo-test` の、`tests/merge-ja-terms.sh` と
  `tests/commit-message.sh`（本物の ja-terms を呼ぶ）
- `nix/tests/` の、本物の git hook でコミットするテスト（HOME を一時のディレクトリにするものと、利用者の HOME で走るもの）
- Windows の CI の `setup` は、docs/windows.md の手順どおり `claude-setup sync` を流してから hook を確かめるので、辞書がある
- claude-hooks の `cargo test` と `claude/tests/` の sh のテストは、偽の ja-terms を使うか、ja-terms を呼ばない
  （Linux で `CLAUDE_HOOKS_JA_TERMS` の無い claude-hooks は検査しない）

機械の上で ja-terms が動くのは、rebuild（activation が sync を呼ぶ）か、Windows の手順（`cargo install` の後に
`claude-setup sync`）を済ませた後なので、配った辞書が無いのは、どちらの手順も流していない機械だけになる。

## 決定の要因

- 規則を足してから効くまでを、ビルドし直さずに済ませること（ユーザー 2026-11-19）
- 同じ機械のどの経路（pre-commit、commit-msg、claude-hooks のルール、/retro の memory-index、history-rewrite）も、同じ
  共通の辞書を読むこと。Windows と Linux で同じ判定になること（ADR-0085 の要因）
- 配った辞書と ja-terms の版がずれたとき（`claude-sync` だけを流して辞書が新しい、またはその逆）に、黙って誤った判定を
  しないこと
- 読めない辞書を黙って飛ばさないこと（ADR-0096 の要因）
- history-rewrite の `run` から `sync-worktrees` までの間に共通の辞書が変わっても、同じ変換を当てること（ADR-0045）
- 新しい配る経路を作らないこと（ADR-0093 の要因の「ハーネスの配備の仕組みに乗せる」）

## 検討した案

配り方:

- 案 A: `claude-setup sync` の `HOME_FILES` に 1 行足し、`~/.config/ja-terms/prh.yml` に置く（markdownlint の設定と同じ経路。
  Windows にも届く）
- 案 B: `~/.claude/` の下（`PLACES` の 1 つ）に置く
- 案 C: 埋め込みのまま（rebuild を待つ）

ソースの置き場:

- 案 S1: 今の `crates/ja-terms/prh.yml` から動かさずに配る
- 案 S2: `config/ja-terms/prh.yml` に移す（配るファイルの置き場が `config/` に揃う）

読む場所の差し替え（テスト、CI、Nix のビルドの中のように、ホームに辞書の無い環境で辞書を渡す方法）:

- 案 E1: 専用の環境変数 `JA_TERMS_COMMON_DICTIONARY` に辞書のパスを渡す。無ければホーム（`HOME`、Windows では
  `USERPROFILE`。`claude-setup sync` と同じ `std::env::home_dir`）の下の `.config/ja-terms/prh.yml`
- 案 E2: `XDG_CONFIG_HOME` を見る（`$XDG_CONFIG_HOME/ja-terms/prh.yml`、無ければ `~/.config/ja-terms/prh.yml`）
- 案 E3: 差し替えを持たず、テストが HOME（Windows では USERPROFILE も）を一時のディレクトリにして辞書を置く
- 案 E4: `--dictionary` で渡す

辞書が無いときと読めないとき:

- 案 M1: 理由と修正方法（`claude-sync` を流す）を出し、検査できなかったとして止める
- 案 M2: 埋めた辞書を予備に残し、配った辞書が無ければそれを当てる
- 案 M3: 共通の辞書を当てずに通す

history-rewrite の記録:

- 案 R1: `run` が、読んだ共通の辞書の中身を作業ディレクトリ（`DIR/common-prh.yml`）にコピーし、それを `--dictionary` として
  `DIR/words` に記録する。`sync-worktrees` は記録のコピーを読む
- 案 R2: 記録に共通の辞書のパス（`~/.config/ja-terms/prh.yml`）を書く
- 案 R3: 記録に共通の辞書の中身のハッシュを書き、`sync-worktrees` が今の辞書と比べて違えば止める

## 決定と理由

採用: 案 A、案 S1、案 E1、案 M1、案 R1。

- `claude-setup sync` は、`crates/ja-terms/prh.yml` を `~/.config/ja-terms/prh.yml` に置く（`HOME_FILES` に 1 行足す。中身が
  違えば上書きし、ソースが無ければ何もしない。ADR-0093 と同じ規則）。rebuild の activation と `claude-sync` と Windows の
  手順の `claude-setup sync` が置く
- ja-terms は、共通の辞書を環境変数 `JA_TERMS_COMMON_DICTIONARY`（空でなければ）のパスから、無ければホームの下の
  `.config/ja-terms/prh.yml` から読む。ビルドに埋めない（`DICTIONARY` を消す）。history-rewrite も同じライブラリの関数で
  読む。環境変数は、テストとビルドの中で辞書を渡すためのもので、利用者の機械では置かない
- 辞書が無い、読めない（UTF-8 でない、YAML の誤り、扱わない項目、specs の失敗）、ホームが分からないときは、検査
  できなかったとして止める（ja-terms は終了コード 2、history-rewrite は 1）。辞書が無いときの理由には、辞書のパスと、
  `claude-sync`（Windows では `claude-setup sync`）で配ることを書く
- `--dictionary` を渡したときは、今のとおり渡した辞書だけを当て、共通の辞書を読まない
- `history-rewrite run` は、`--dictionary` が無ければ、共通の辞書を 1 度だけ読み、その中身で変換を作り、同じ中身を
  `DIR/common-prh.yml` に書いて、`--dictionary DIR/common-prh.yml` として `DIR/words` に記録する。`sync-worktrees` は記録の
  コピーを当てる。辞書を渡さずに `sync-worktrees` を呼んだ指定は、記録のコピーを渡したものとみなして比べる（ADR-0096 の
  リポジトリの辞書と同じ扱い）
- テストとビルドの中では、共通の辞書のソース（`crates/ja-terms/prh.yml`）を `JA_TERMS_COMMON_DICTIONARY` で渡す
  （Rust のテストは子プロセスに、claude-hooks の Nix のパッケージと Windows の CI はシェルのテストに、`nix/tests/` は
  hook を起動する前に）

理由:

- 案 A は、新しい経路を作らない。activation、`claude-sync`、Windows の手順の 3 つが既に `claude-setup sync` を呼ぶので、
  どの機械にも同じ中身が届く。案 B の `~/.claude/` は Claude Code の設定の置き場で（ADR-0093 の要因）、ja-terms の辞書は
  Claude Code のものでない。案 C は、規則を足すたびに rebuild を待つ（決定の要因に反する）
- 案 S1 は、crate のテスト（`CARGO_MANIFEST_DIR` の `prh.yml`）と Nix のパッケージの fileset（ja-terms は crate の直下を
  src にする。history-rewrite は `../ja-terms/prh.yml` を含める）がそのまま辞書を読める。並行して共通の辞書から xlc の
  規則を消す PR（#316）とも衝突しない。案 S2 は、配るファイルの置き場が `config/` に揃うが、ja-terms と history-rewrite の
  パッケージの fileset の root を変え、テストのパスも変える。辞書は配るファイルであると同時に ja-terms のテストの入力
  （specs、`lib.rs` の needle のテスト）なので、crate の中に置く方が読み手に近い
- 案 E1 は、`claude-setup sync` が置く場所（`home_dir` の下の固定のパス）と ja-terms が読む場所が、環境変数を置かない限り
  必ず一致する。案 E2 は、`XDG_CONFIG_HOME` を `~/.config` 以外にした機械で、sync が置く場所と ja-terms が読む場所がずれる
  （sync も見るようにすると、markdownlint の設定と扱いが分かれる）。Windows には `XDG_CONFIG_HOME` の慣習が無い。案 E3 は、
  Windows で `home_dir` が `HOME` でなく `USERPROFILE` を見るので、テストが OS ごとに違う変数を差し替える。シェルの
  テスト（claude-hooks の `merge-ja-terms.sh`）が HOME を差し替えると、git の設定の置き場も動く。案 E4 は、`--dictionary`
  が「渡した辞書だけを当て、リポジトリの辞書を重ねない」意味（ADR-0096）なので、共通の辞書の差し替えに使うと、
  リポジトリの辞書を重ねる経路をテストできない
- 案 M1 は、ADR-0096 の「読めない辞書を黙って飛ばさない」と同じ向き。辞書の無い環境は、上の背景のとおりテストとビルドの
  中（辞書を渡す）と、rebuild か Windows の手順を流していない機械だけで、後者は ja-terms と一緒に辞書を配る手順を流せば
  直る。案 M2 は、辞書の出どころが 2 つになり、古い埋め込みで通ったことに気づけない（配り忘れた機械で、足した規則が
  黙って効かない）。案 M3 は、検査が黙って抜ける
- 版のずれ: 辞書が ja-terms より新しく、古い ja-terms の読めない書き方（扱わない項目、読めない正規表現）か、古い当て方で
  specs が合わない規則を持てば、ja-terms は辞書を読むときに specs を確かめるので（ADR-0085）、読めない辞書として止まる。
  specs が通る規則は、古い ja-terms でも辞書を書いた人の確かめた例のとおりに当たる。辞書が ja-terms より古い（rebuild が、
  `claude-sync` で入れた新しい辞書を、古い lock の store の辞書で上書きする）ときは、足した規則がまだ当たらないだけで、
  誤った指摘はしない（今の埋め込みで rebuild を待つ間と同じ。スキルと CLAUDE.md も同じ順で戻る）
- 案 R1 は、`run` の変換と `sync-worktrees` の変換が、間に `claude-sync` か rebuild があっても同じ辞書の中身になる。
  記録のコピーは、`run` が変換に使った中身そのもの（読み直さない）。案 R2 は、`run` の後に辞書が変わると、
  `sync-worktrees` が履歴と違う変換を worktree の変更に当てる。案 R3 は変わったことに気づけるが、止まった後に元の
  辞書を戻す手段が無い

### 帰結

- 良い点: 共通の辞書の規則を足すと、マージと `claude-sync` で効く（ja-terms、claude-hooks、history-rewrite をビルドし
  直さない）。Windows も、clone を更新して `claude-setup sync` を流せば効く
- 良い点: 共通の辞書を読む場所が 1 つ（`~/.config/ja-terms/prh.yml`）になり、どの経路もそこを読む
- 悪い点: 辞書を配っていない機械では、ja-terms の検査がすべて止まる（コミットが止まる）。理由と修正方法を出す
- 悪い点: テストとビルドの中で、`JA_TERMS_COMMON_DICTIONARY` を渡す所が増える（Rust のテストの子プロセス、claude-hooks の
  package.nix、Windows の CI、`nix/tests/`）
- 悪い点: rebuild は flake の lock の版の辞書を配るので、`claude-sync` で入れた新しい辞書を、lock の古い版で上書きする
  （スキルと CLAUDE.md と同じ）
- `HOME_FILES` から外しても、配った辞書は消えない（ADR-0093 の帰結と同じ）
- ADR-0085 と ADR-0096 の背景の「ビルドに埋めた共通の辞書」は、この ADR の前の事実になった（両方の「## 変更」に書く）

### 確認

- `crates/ja-terms/tests/common_dictionary.rs`: 4 つのサブコマンドが `JA_TERMS_COMMON_DICTIONARY` の辞書と、無ければ
  ホームの下の `.config/ja-terms/prh.yml` を読み、辞書が無い、YAML が読めない、specs が合わないときに 2 で終わり、
  無いときの理由に `claude-sync` を書き、`--dictionary` では共通の辞書を読まないこと
- `crates/claude-setup/tests/sync.rs`: `claude-setup sync` が `crates/ja-terms/prh.yml` を `~/.config/ja-terms/prh.yml` に置くこと
- `crates/history-rewrite/tests/`: `run` が共通の辞書を `DIR/common-prh.yml` にコピーして記録し、`run` の後に共通の辞書を
  変えても `sync-worktrees` がコピーの規則で直し、共通の辞書が無ければ止まること
- 一時のホームで、辞書に規則を 1 つ足して `claude-setup sync` だけを流すと、ビルドし直さない ja-terms がその規則を当てる
  こと（実装の PR の手順と出力）
