---
status: accepted
date: 2026-11-06
requires: []
supersedes: []
superseded-by:
issues: ["#603"]
---

# ADR-0076: Claude Code の自動メモリを、応答のたびに ~/.claude の git の別のブランチ memory-backup にコミットする

## 背景

Claude Code の自動メモリ（`~/.claude/projects/<プロジェクト>/memory/`）には履歴が無い。`~/.claude` は git の
リポジトリ（ブランチ main、リモートなし）で、手で管理する `~/.claude/.gitignore` は `projects/**` を除外して
`!projects/*/memory/**` でメモリだけを追跡する形にしてあるが、最後のコミットは 2026-08-23 の 533b677 で、
`git status --porcelain` は 99 件、xlc のメモリ（`projects/-home-tagawa-github-xlc/memory/`）は一度もコミットされて
いない（2026-11-06 に確かめた）。

2026-11-06 05:28 ごろ、xlc のセッションが Python の 1 行（書き込みで開いてから読む形）で xlc のメモリの索引
`MEMORY.md` を空にした。セッションの先頭に読み込まれていたので復元できたが、長い引き継ぎのファイルを壊すと会話の
記録から拾うしかない。2026-10-28 にも別のメモリで同じ型の失敗があった（#603）。

親のセッションは、このリポジトリでメモリを定期的にコミットする仕組み（claude-sync か、セッションの終わりの hook）を
決めることを推し（2026-11-06 05:32 JST の報告）、ユーザーは「4点とも推奨に従います。」（ユーザー 2026-11-06）と答えた。

同じ日に、main / master への直接のコミットを、GitHub のリモートが無いリポジトリでも止める変更（#595、PR #602）が
進んでいる。git の層（グローバルの pre-commit の hook）は `CLAUDECODE=1` のときに、ブランチが main / master なら
止める。`~/.claude` はリモートが無く main にいる。

## 決定の要因

- 壊したときに、壊す前の版を戻せる粒度
- ほかの hook と応答の時間への影響
- 失敗が黙って起きないこと（hook の失敗は利用者に見えにくい）
- main への直接のコミットを止めるゲート（#595）と両立し、例外を足さないこと
- 依存の少なさ

## 検討した案

いつコミットするか:

- 案 A: SessionEnd の hook。セッションが終わったときだけ走る。xlc の親のセッションのように数日続くセッションでは、その
  間の変更が 1 つにまとまる。既定の時間の上限は 1.5 秒で、`systemMessage` は捨てられる（Claude Code の hooks の文書、
  SessionEnd の節。2026-11-06 に読んだ）ので、失敗が利用者に見えない
- 案 B: claude-sync にコミットを足す。claude-sync は手で呼ぶか rebuild の activation で走るだけで、粒度は数日になる。
  nixfiles の `.claude` を `~/.claude` に同期する役目と関係が無い
- 案 C: systemd のユーザーの timer で数分ごとにコミットする。Claude Code に依らないが、粒度は間隔で決まり、失敗は
  journal と `systemctl --user --failed` にしか出ない。ホストごとに unit が要る
- 案 D: Stop の hook。主のエージェントの応答が終わるたびに走る。同期の hook の 0 と 2 以外の終了コードは止めない失敗
  として、標準エラーの 1 行目が通知に出る（2 は応答を続けさせる）。利用者が中断した応答では走らない（同じ文書の
  Stop の節）
- 案 E: PostToolUse の hook で、Write と Edit がメモリに書いた直後にコミットする。今回の失敗は Bash の Python が書いた
  ので拾えない。Bash の全部の呼び出しに掛けると、メモリに触らない呼び出しにも時間が掛かる

どこにコミットするか:

- 案 1: `~/.claude` の main に `git commit -- <メモリのパス>` でコミットする。#595 の git の層は `CLAUDECODE=1` で
  main へのコミットを止める。hook のプロセスは親の環境を継ぐ（hooks の文書）が、Claude Code が自身の環境に
  `CLAUDECODE=1` を置くかは確かめていない。置いていれば git の層に `~/.claude` の例外が要り、PreToolUse の層との一致
  （`main-commit-gates.sh`）を崩す。ほかに、commit-msg の検査、機密情報の検査、ステージしたファイルの日本語の語の検査
  （ADR-0014）がメモリの本文に掛かり、崩れた語のあるメモリの退避が止まる。利用者が main でステージした変更とも混ざる
- 案 2: 一時の index にメモリだけを入れ、`git write-tree`、`git commit-tree`、`git update-ref` で別のブランチ
  `memory-backup` を進める。HEAD、作業ツリーの index、main に触らず、git の hook は走らない
- 案 3: `~/.claude` と別のリポジトリ（`~/.claude/memory-backup.git` など）にコミットする。ゲートとは関係なくなるが、
  ユーザーが答えた推奨は「このリポジトリで」で、`~/.claude/.gitignore` もメモリを追跡する形にしてある

## 決定と理由

案 D と案 2 を採る。Stop の hook（`~/.claude/bin/claude-memory-backup`、`modules/home/scripts/claude-memory-backup.sh`）が、
応答のたびに `~/.claude/projects/*/memory/` の下の全部を一時の index に入れ、ツリーが `memory-backup` の先端と違うときだけ、
先端を親にしたコミットを作ってブランチを進める。

- 粒度: 応答ごとの版が残るので、ある応答がメモリを壊しても、その前の応答の終わりの版が残る。今回の形（索引を壊した
  応答の後に気づく）はこれで戻せる。案 A と案 B は数日、案 C は間隔の粒度になる
- 時間: 実際のメモリ（17 プロジェクト、130 ファイル、624 KB。2026-11-06）のコピーで、1 回 0.02 秒以下（変更の
  ある回とない回の両方。`time` で測定）。応答ごとに実行しても待ちにならない
- 失敗を見せる: 失敗したら、1 行目に「claude-memory-backup: メモリを … コミットできませんでした」と書き、続けて git の
  エラーを出して終了コード 1 で終わる。Claude Code は通知に 1 行目を出す。async の hook にしないのは、async の hook の
  `systemMessage` は利用者に見えず、次のターンで Claude にだけ渡る（hooks の文書の「How async hooks execute」）ため
- ゲートとの両立: main にも HEAD の指すブランチにもコミットしないので、#595 の 2 つの層のどちらにも掛からず、例外は
  要らない。git の層はコミットの先のブランチが main / master のときに止めるもので、`memory-backup` は対象でない。
  PreToolUse の層は Bash ツールの呼び出しを見るもので、hook のプロセスは通らない
- 同時に終わった応答: 複数のセッションの Stop の hook が同時に走っても変更を失わないよう、`.git/memory-backup.lock` を
  `flock` で取り、親を読んでからブランチを進めるまでを 1 つずつにする。`update-ref` にも古い値として親を渡す
- 依存: git、jq（hook の入力の `session_id` と `cwd` をコミットの本文に入れる）、flock（util-linux）、coreutils。どれも
  既に使っている
- `~/.claude` が git のリポジトリでないホストでは、home-manager の activation が `git init` する（退避の hook が
  応答のたびに失敗を出し続けないため）
- `~/.claude/.gitignore` は nixfiles で管理しない。退避は `git add -f` でメモリのディレクトリだけを入れるので
  `.gitignore` に依らず、`.gitignore` は利用者が main で何を追跡するかの設定のまま残す

### 帰結

- 戻し方: `git -C ~/.claude log --stat memory-backup -- projects/<プロジェクト>/memory/<ファイル>` で版を探し、
  `git -C ~/.claude show <コミット>:projects/<プロジェクト>/memory/<ファイル>` で中身を出す
- 同じ応答の中で書いて壊したものは、その応答の前の版しか残らない。利用者が中断した応答の変更は、次の応答の終わりに
  まとめて入る。サブエージェントが書いたメモリは、親の次の応答の終わりに入る
- 機密情報の検査（ADR-0015）は退避のコミットに掛からない。`~/.claude` にリモートは無く、`memory-backup` を push する
  手順も無い
- `~/.claude` の main と、手で管理する `.gitignore` は今のまま。main へのコミットは利用者が行う
- コミットはメモリが変わった応答ごとに増える。オブジェクトは小さく、git の自動の gc がまとめる

### 確認

- `modules/home/parts/tests/claude-memory-backup.sh`: メモリだけがツリーに入る、同じ中身では足さない、消したファイルが
  前のコミットから戻せる、main の HEAD とステージした変更が変わらない、どれも失敗する git の hook と `CLAUDECODE=1` の
  もとでも成功する、同時の 6 本がどれも失敗しない（ロックを外すと 5 本が `update-ref` の古い値の不一致で落ちる）、
  リポジトリでないディレクトリでは 1 行目に理由を出して失敗する
- activation の settings.json の更新を `~/.claude/settings.json` のコピーで 2 回実行し、Stop の登録が 1 件だけで、
  ほかの設定が変わらないことを確かめた（2026-11-06）
