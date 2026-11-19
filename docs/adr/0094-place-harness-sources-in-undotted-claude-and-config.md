---
type: ADR
status: accepted
date: 2026-11-17
requires: [ADR-0084, ADR-0093]
supersedes: [ADR-0080]
superseded-by:
issues: ["#268"]
---

# ADR-0094: ハーネスの配る元は dot なしの claude/ と config/ に置き、このリポジトリのセッションは ~/.claude だけを読む

## 背景

ハーネスが PC 全体に配るもの（グローバルの指示、スキル、スクリプト）の元はリポジトリの `.claude/` に、markdownlint の
グローバルの設定の元は `.config/` にあり、`claude-setup sync` が `~/.claude` と `~/.config` に配る（ADR-0084、ADR-0093）。

この置き方に 2 つの問題がある（#268）。

- どちらも隠しディレクトリで、リポジトリに入って `ls` しても出ない。このリポジトリの中身の大半（追跡しているファイルの
  うち `.claude/` に 94、`.config/` に 1。2026-11-17、ff999551）が、見えない場所にある
- Claude Code は `./.claude` をプロジェクトの設定として、`~/.claude` をユーザーの設定として読むので、このリポジトリの
  セッションには同じ中身が 2 か所から届く。CLAUDE.md は、プロジェクトの設定（`.claude/settings.json`）の
  `claudeMdExcludes` でユーザーの側を除いて 1 回にしている（ADR-0080）。スキルは 2 か所にあるままで、一覧に出るのは 1 回
  （ADR-0080 の確認）。重複を消すための設定が 1 つ余分にあり、その設定は `~/.claude` に配ってはならないもの（配ると
  どのリポジトリでも指示が読まれなくなる）として、同期の対象から名前で外している

ADR-0080 は、ディレクトリの名前を変える案（案 D）を、パスを書いているファイルのすべてに変更が及び、指示の重複 1 つに
見合わないとして採らなかった。そのときの要因は指示の重複だけで、隠しディレクトリの見えにくさは要因に無かった。

## 決定の要因

- リポジトリに入ったとき、配る元が `ls` に出ること
- このリポジトリのセッションが、指示とスキルを 1 か所からだけ読み、そのための設定を持たないこと
- ほかのリポジトリのセッションが読むもの（`~/.claude`、`~/.config/markdownlint/config`）が変わらないこと

## 検討した案

### 案 A: `.claude/` を `claude/` に、`.config/` を `config/` に改名する（採用）

`claude-setup sync` は `claude/` を `~/.claude` に、`config/markdownlint/config` を `~/.config/markdownlint/config` に配る。
`.claude/settings.json` は消す。

- 長所: 配る元が見える。このリポジトリに `.claude` が無くなり、セッションは `~/.claude` だけを読む。`claudeMdExcludes` と、
  それを同期から外す決まりが要らなくなる
- 短所: リポジトリの側のパスを書いているコード、テスト、文書を直す（ADR を除いて 130 行ほど）。確定した ADR の凍結する
  部分には `.claude/` が残る
- 短所: このリポジトリのセッションは、作業中のブランチの指示とスキルでなく、配られた版（`~/.claude`）を読む

### 案 B: 改名せず、今のまま（ADR-0080）

- 長所: 変更が無い。worktree のセッションが、そのブランチの指示を読む
- 短所: 隠しディレクトリのまま。`claudeMdExcludes` の設定と、ホームが `/home/<ユーザー>` でない環境で 2 回読みに戻る
  制約が残る

### 案 C: `.claude/` だけを改名し、`.config/` は残す

- 長所: 置き先と同じ相対パスで持つ（ADR-0093 の要因）ことが、markdownlint の設定では保たれる
- 短所: 隠しディレクトリが 1 つ残り、配る元の名前の付け方が 2 通りになる

### 案 D: 改名した上で、リポジトリの先頭に `claude/CLAUDE.md` を取り込む CLAUDE.md を置く

`~/.claude/CLAUDE.md` の無い環境（クラウドのセッションなど）と worktree のセッションに、そのブランチの指示を読ませる。

- 長所: ADR-0080 の要因（`~/.claude/CLAUDE.md` の無い環境でも指示が読まれる）を保つ
- 短所: `~/.claude/CLAUDE.md` のある環境では、同じ指示を 2 回読む。1 回にするには `claudeMdExcludes` を置く `.claude/` が
  要り、この決定で消すものが戻る

## 決定と理由

案 A。

- 配る元は `claude/`（CLAUDE.md、commands、skills、scripts。README と tests は配らない）と `config/`（markdownlint の設定）
- `claude-setup sync` は、ソースの `claude/` を `~/.claude` に、`config/markdownlint/config` を
  `~/.config/markdownlint/config` に置く。ソースのパスと置き先のパスは、同期のコードが対で持つ（ADR-0093 の「同じ相対パス」は
  やめる。ADR-0093 の「## 変更」）。`.claude/` を持つ古い版のソースは読まない（古い名前を探す分岐を残すと、名前が 2 つ
  ある状態が続く）
- `.claude/settings.json`（`claudeMdExcludes`）は消す。このリポジトリが追跡するプロジェクトの設定は、このリポジトリだけの
  規則（直下の `CLAUDE.md`）とスキル（`.claude/skills/`）だけで、配る元（`claude/`）と同じ中身は置かない。
  `.gitignore` の `.claude/settings.local.json` は残す（Claude Code は、このリポジトリのセッションで許可を選ぶと、
  `.claude/` が無くてもこのファイルを作る。2026-11-17、Claude Code 2.1.326 で、main の作業ツリーに作られた）
- 変更の後のレビューの要否とレビューのレベルを決める「規則の文書」の判定（`check-merge-review`、`review-level.sh`）、
  `/retro` の `prose-size.sh` のスキルの判定は、リポジトリの先頭の `claude/` を `.claude/` と同じに数える（ほかの
  リポジトリの `.claude/` はそのまま数える）。`guard-sbatch` が通すテストは `claude/tests/scripts/slurm-run.sh`、
  `post-merge-cleanup.sh` が `claude-sync` を知らせる変更は `claude/` と `config/`（古い版の clone のために `.claude/` も見る）
- 案 B は、要因の 1 つ目を満たさない。ADR-0080 が改名を採らなかった理由（変更の範囲が指示の重複 1 つに見合わない）は、
  要因が 2 つになったので当たらない
- 案 C は、`config/` の中身が 1 ファイルで、対で持つコードは 1 行で済む。名前の付け方を揃える方を取る
- 案 D は、消す設定が戻る。`~/.claude/CLAUDE.md` の無い環境でこのリポジトリを開く使い方は、今は無い

### 帰結

- 良い点: 配る元が `ls` に出る。このリポジトリが追跡するプロジェクトの設定は、配る中身と重ならない、このリポジトリ
  だけの規則とスキルだけになる（「## 変更」の 2026-11-19 の項）
- 悪い点: このリポジトリの worktree で指示やスキルを直しても、そのセッションには効かない。試すときは
  `claude-sync <worktree>` で `~/.claude` に入れる（ほかのセッションにも効く。試し終えたら引数なしの `claude-sync` で main の
  版に戻す）。ADR-0080 の「指示を直すブランチでは、同期しなくてもその worktree のセッションに効く」は無くなる
- 悪い点: `~/.claude/CLAUDE.md` の無い環境でこのリポジトリを開くと、グローバルの指示が 1 つも読まれない
- 悪い点: 確定した ADR の凍結する部分と、マージの前のコミットの本文には `.claude/` と `.config/` が残る。この ADR より前の
  記録の、リポジトリの側の `.claude/<パス>` は `claude/<パス>`、`.config/<パス>` は `config/<パス>` と読む（`~/.claude` と
  `~/.config` は置き先で、変わらない）
- 配ってある版の `claude-setup`（と `claude-sync`）は、この変更より後のソースを読めない（`.claude` が無いので
  「ソースが見つかりません」で止まり、何も書かない）。マージの後、取り込む側（nixfiles）で lock を進めて rebuild するか、
  Windows では `cargo install` し直すまで、引数なしの `claude-sync` は使えない
- 取り込む側（nixfiles）の `harness-contract.sh` は、ハーネスのスクリプトを `.claude/scripts` から読む。lock を進める前に、
  `claude/scripts` を読むように直す

### 確認

- `modules/home/parts/claude-setup/tests/sync.rs`: ソースの `claude/` を `~/.claude` に、`config/markdownlint/config` を
  `~/.config/markdownlint/config` に配る。`.claude/` だけを持つソースでは止まる
- `modules/home/parts/claude-hooks/tests/script_prose_size.rs`、`script_review_level.rs`、
  `modules/home/parts/claude-hooks/src/rules/check_merge_review.rs` と `guard_sbatch.rs` のテスト: `claude/` の下の判定
- `claude/tests/scripts/git-workflow.sh`: `claude/` か `config/` が変わる pull の後に `claude-sync` を知らせる
- このリポジトリのセッションが指示を 1 回だけ読むことの自動のテストは無い（確かめるには Claude Code を起動して API を呼ぶ
  必要がある）。疑うときは、main の作業ツリーで `claude -p` に見出し「# グローバル開発指針」の回数を聞く

## 変更

- 2026-11-19: 追跡するプロジェクトの設定。前は「このリポジトリは、追跡するプロジェクトの設定を持たない。」。配る元を
  `claude/` に移したので、`.claude/` と直下の `CLAUDE.md` は、配る中身と重ならずにこのリポジトリのセッションだけが読む
  置き場として使える。配備と導入の手順をスキル（`.claude/skills/harness-deploy`、`harness-install`）に、このリポジトリだけの
  規則を直下の `CLAUDE.md` に置く（ユーザー 2026-11-19 の指示）。決定の要因の「指示とスキルを 1 か所からだけ読み」は、
  配る中身について保つ: このリポジトリだけの規則とスキルは配る元（`claude/`）に置かないので、同じ中身が 2 か所から届く
  ことは無い。`.claude/` と直下の `CLAUDE.md` に足してよいのは、配る元に無い、このリポジトリだけのものに限る
