---
status: accepted
date: 2026-11-06
requires: [ADR-0014, ADR-0050]
supersedes: []
superseded-by:
issues: ["#592"]
---

# ADR-0074: 自動メモリの本文の日本語の語を、/retro の memory-index.sh で ja-terms の辞書で検査する

## 背景

Claude Code の自動メモリ（`~/.claude/projects/<プロジェクト>/memory/*.md`）は git で追跡しないので、崩れた日本語の語の
検査（ADR-0014 の pre-commit と commit-msg）が掛からない。メモリは毎回のセッションに読み込まれ、そこから依頼、コミットの
本文、文書にコピーされるので、メモリに入った崩れた語は、コピーした先のコミットの段で止まる（#592）。

- 2026-11-06 01:04 JST、xlc の親のセッションのコミットの本文が commit-msg に止まった。出どころは、親が handoff のメモリに
  言い換えて書いた語だった
- 同じ日に xlc のメモリ（19 ファイル）を `ja-terms-message` に通すと、8 ファイルで 18 件あった（#592 の本文。/retro の
  担当が数えた）
- xlc のメモリには「memory を書いたら同じ辞書で検査する」とあるが、検査のコマンドが PATH に無く（`ja-terms-message` は
  commit-msg の hook と claude-hooks が store のパスで呼ぶだけ）、守られていなかった

ユーザーは、#592 の案のうち /retro の memory-index.sh に辞書の検査の行を足す案を問われ、「メモリに対する辞書検査はぜひやってください」（ユーザー 2026-11-06）と答えた。

## 検討した案

いつ検査するか:

- 案 A: /retro の `memory-index.sh` が、メモリの各ファイルを辞書で検査して `TERMS: <file>:<行> <誤り> => <正しい語>` を
  出す。/retro の手順 4 は memory-index.sh の食い違いを確認せずに直すので、区切りごとに直る
- 案 B: Write と Edit でメモリのディレクトリに書く呼び出しを、PostToolUse の hook で検査する
- 案 C: A と B の両方

検査の入口:

- 案 1: `ja-terms-message`（コミットのメッセージの検査）に、各ファイルの前に件名の行を足して渡す
- 案 2: ファイルの全体を検査する入口 `ja-terms-files` を ja-terms（`textlint.nix`）に足す

スクリプトから呼ぶ方法:

- 案 i: `home.packages` で PATH に出す（`staged-checks` の形）
- 案 ii: claude-code.nix でスクリプトに store のパスを埋める（ADR-0050 の package.nix の形）
- 案 iii: claude-code.nix が `~/.claude/bin/ja-terms-files` の固定パスに置き、スクリプトはそのパスを呼ぶ（環境変数
  `JA_TERMS_FILES` で差し替える。`~/.claude/bin/claude-hooks` と `.claude/scripts/lib/local-review.sh` の形）

## 決定と理由

採用: 案 A、案 2、案 iii。

- 案 A は、書き方によらず /retro の区切りで拾える。案 B は、Bash でメモリを書き換える形（xlc の親は handoff のメモリを
  python の置き換えで書き直すことが多い）を捕まえられない（#592）
- 案 1 は、`ja-terms-message` がコミットのメッセージの規則（件名が Merge、fixup!、squash!、Revert なら件名を除く、
  scissors の行から後を除く）を掛けるので、それを外すために件名をこしらえて渡すことになる。メッセージの規則を変えると
  メモリの検査が黙ってずれる。案 2 は、pre-commit（`staged.sh`）と同じく、textlint の設定（辞書、拡張子とパーサー）を
  そのまま使い、ファイルの全体を見る。Markdown のコードスパン、フェンス、フロントマターは、pre-commit と同じく textlint の
  パーサーが外す
- `ja-terms-files` は、textlint が引数をグロブとして読む（ADR-0014）ので、`staged.sh` と同じく番号の名前のファイルに
  書き出して 1 回の textlint で検査し、出力のパスを引数のとおりに戻す。ファイルごとに `--stdin` で呼ぶと、textlint の
  起動（0.3 秒ほど。ADR-0050）がファイルの数だけ掛かる。textlint は知らない拡張子のファイルを検査せずに通すので、
  対象の拡張子（`md` と `textlint.nix` の `codeExtensions`）に無いファイルと読めないファイルは、検査できないとして
  終了コード 2 で止める
- 案 ii は、スキルのスクリプトを claude-sync が store を経ずにそのままコピーする（`modules/home/scripts/claude-sync.sh`）
  ので、埋める場所が無い。スクリプトを Nix で組む形に変えると、claude-sync で即座に反映する運用から外れる
- 案 i と案 iii の違いは、呼ぶ側が PATH に頼るかと、名前を全体の PATH に出すか。`ja-terms-files` を呼ぶのは Claude Code の
  スクリプトだけで、`~/.claude/bin` には同じく Claude Code のためにだけ置く `claude-hooks` がある。テストは
  `JA_TERMS_FILES` で偽物か nix build したものに差し替える（`CLAUDE_HOOKS_BIN` と同じ）
- memory-index.sh は、辞書の検査が無い（rebuild の前）か失敗したら、ほかの行を出した後に `ERROR:` を出して exit 1 に
  する。黙って 0 件にすると、検査していないメモリを検査したと読む。崩れた語があると言う（終了コード 1）のに指摘の行を
  読めないときも、出力の形が変わったとして同じく止める

### 帰結

- /retro を回すたびに、メモリの崩れた語が `TERMS:` の行で出て、手順 4 で直る。回す合間にメモリから依頼やコミットに
  コピーされた語は、今までどおりコミットの段で止まる
- フロントマター（`description` など）の中の語は検査しない（pre-commit の Markdown と同じ）。メモリの索引 `MEMORY.md` の
  1 行の説明は本文なので検査する
- `ja-terms-files` は rebuild で `~/.claude/bin` に入る。claude-sync だけで memory-index.sh を入れ替えると、rebuild まで
  memory-index.sh は `ERROR:` で止まる（ほかの行は出る）
- 辞書と textlint の設定を変えると、pre-commit、commit-msg、claude-hooks、memory-index.sh が同じものを使う（`textlint.nix`）

### 確認

- `modules/home/parts/tests/ja-terms.sh`: nix build した `~/.claude/bin/ja-terms-files`（home-manager の home-files）が、
  崩れた語を引数のパス（空白とグロブの文字を含む）と行で出し、コードスパンの中を数えず、最初の行が Merge でも全体を見て、
  拡張子の無いファイルと無いファイルを 2 で止めること。memory-index.sh がそれを使って `TERMS:` の行を出し、コードスパン、
  フェンス、フロントマターの中を数えず、崩れた語が無ければ出さないこと
- `.claude/tests/scripts/retro.sh`: 偽の `ja-terms-files` で、memory-index.sh がすべての .md を 1 回で渡し、指摘を
  `TERMS:` の行にし、検査が無い、失敗した、指摘を読めないときに exit 1 で止まること
