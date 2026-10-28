---
name: git-merge
description: featureブランチをローカルでmainにマージ。GitHubを経由しないリポジトリ用。チェック・レビュー・rebase・マージコミット・後片付けまで行う。
# model 未指定: レビュー指摘への対応、コンフリクト解決、マージコミットメッセージの判断が必要なためセッションモデルを継承する
argument-hint: [branch]
allowed-tools:
  - Bash(~/.claude/scripts/git-merge-state.sh*)
  - Bash(~/.claude/scripts/post-merge-cleanup.sh*)
  - Bash(~/.claude/scripts/review-level.sh*)
  - Bash(~/.claude/skills/language-checks/scripts/run-checks.sh*)
  - Bash(design-records -C*)
  - Bash(git status*)
  - Bash(git branch*)
  - Bash(git log*)
  - Bash(git diff*)
  - Bash(git range-diff*)
  - Bash(git merge-base*)
  - Bash(git rev-parse*)
  - Bash(git symbolic-ref*)
  - Bash(git show*)
  - Bash(git switch*)
  - Bash(git rebase*)
  - Bash(git merge*)
  - Bash(git rev-list*)
  - Bash(git worktree*)
---

# Git Merge Command

GitHub リモートを持たないリポジトリで、feature ブランチをローカルの main にマージする。
GitHub での PR の流れ（CI → 自動レビュー → `gh pr merge --merge`）を、ローカルの手順に置き換えたもの。

## 現在の状態

!`~/.claude/scripts/git-merge-state.sh $ARGUMENTS`
!`git log --oneline --graph --all -20`

$ARGUMENTS のブランチ（省略時は現在のブランチ）が対象。上が `ERROR:` で終わっていれば、その指示に従う
（main 自身、GitHub リモートあり、未コミットの変更は、ここで止まる）。

対象の `TARGET_WORKTREE` と main の `DEFAULT_WORKTREE` は、そのブランチをチェックアウトしている場所。
別の worktree にあるブランチへは switch できないので、ブランチの操作は `git -C [TARGET_WORKTREE]`、
マージは `git -C [DEFAULT_WORKTREE]` で行う。

`TARGET_WORKTREE` が none（対象がどこにもチェックアウトされていない。worktree が 1 つだけのときなど）なら、
`git -C [DEFAULT_WORKTREE] switch [branch]` で main の worktree に取り出し、以降の `[TARGET_WORKTREE]` はそこを指す。
マージの前に `git -C [DEFAULT_WORKTREE] switch main` で main に戻す。`DEFAULT_WORKTREE` も none なら今の worktree を使う。

## 止まる条件

次は hook が止める。通らないときは理由に従って直す（文言や書き方を変えて迂回しない）:

- 新しい ADR を 2 件以上加えるブランチ。決定ごとにブランチを分ける（/topic-triage）
- マージ結果が品質チェックを通らない

## 判断が要る手順

### main がまだない（`DEFAULT: none`）

`ROOT_COMMITS` が 1 つで、初期化だけの内容であることを `git show` で確認してから、ルートコミットを指す main を作る:

```bash
git branch main [root-commit]
```

ルートコミットに初期化以外の変更が含まれていれば、作らずにユーザーに確認する。

### 積み重なり（`STACKED_ON` が none でない）

対象は、まだマージしていない別のブランチの上に積まれている。土台のブランチから順にこの手順でマージし、
対象は後で rebase する。

### rebase（`BEHIND` が 0 でない）

rebase の前に、古い土台（`git merge-base main [branch]`）と旧先端（`git rev-parse [branch]`）を控える（「レビュー」の rebase 後の判断で
`git range-diff <古い土台>..<旧先端> main..[branch]` と `git log <古い土台>..main` に使う。rebase の後では取れない）。同じ範囲を触る
ブランチは並行に進めず、マージしてから次を始める（並行の数が多いほど main が進み、rebase とレビューのやり直しが増える。ユーザー 2026-10-21）。

```bash
git -C [TARGET_WORKTREE] rebase main
```

コンフリクトしたら内容と解決案を示し、解決して `git rebase --continue` するか
`git rebase --abort` するかを決める。

`remap.commitRefs` が true のリポジトリでは、rebase と `commit --amend` の後に post-rewrite の hook が文書の中の
コミットの hash を付け替える。付け替えは作業ツリーに残るだけなので、hook の出力に従って片付けてから次の「番号の重複」に進む
（後の手順で amend したときも同じ。残したままマージすると、ブランチの文書は main の履歴に無い rebase 前の hash を指したまま main に入る）:

- `📝 置き換えはコミットしていません`: `git -C [TARGET_WORKTREE] diff` で旧と新の hash の置き換えだけであることを確かめ、
  コミットする（件名は `chore: remap the commit refs after the rebase` のように、きっかけを書く）
- `❌ コミットの番号を付け直せませんでした`: 対応は二度と渡されないので、原因を直してから、出力の修正方法の
  `commit-refs remap <保存した対応>` を `cd [TARGET_WORKTREE] &&` を付けて実行し（今のディレクトリの文書を書き換える）、
  上と同じく確かめてコミットする。途中まで書き換えて止まったこともある（ERROR の最後の行に書き換えたファイルが出る）ので、
  原因を直す前に `git -C [TARGET_WORKTREE] diff` で、書き換え済みの分を見ておく

### 番号の重複（rebase の後）

設計の記録（docs/adr か docs/issues）のあるリポジトリでは、rebase の直後に番号の重複を確かめる。main にも
このブランチにも同じ番号の記録が入っていると、`number NNNN is also used by ...` が出る:

```bash
design-records -C [TARGET_WORKTREE] check
```

重複があれば、このブランチで足した側を振り直す（ADR は `design-records -C [TARGET_WORKTREE] renumber NNNN`、
issue は `design-records -C [TARGET_WORKTREE] renumber --kind issue NNNN`）。新しい番号の予約、ファイル名と参照の付け替え、コミットまでを行う。
できたコミットを `git show` で見て、付け替えた参照が振り直した記録を指していることを確かめてから、チェックに進む。

### チェック（CI の代わり）

main との分岐点からの変更を検査する。マージ時にも hook が走るが、ここで先に直しておく:

```bash
~/.claude/skills/language-checks/scripts/run-checks.sh -C [TARGET_WORKTREE] --base main
```

失敗したら原因を直してコミットし、もう一度チェックする。通ってからマージする。

全体の `cargo test` などで前景の上限（600 秒）を超えうるので、チェックは `run_in_background` で回し、終わりの通知を待つ。
`git commit … && run-checks.sh` のように commit とチェックを 1 つの前景のコマンドにつながない。

### レビュー（自動レビューの代わり）

レベルと対象はスクリプトが決める。worktree で作業していると、引数なしの `/code-review` は呼び出し元の cwd
（main の worktree など）の差分を見て「指摘なし」と返し、ブランチ名は cwd のリポジトリで解くので、対象は
ブランチをチェックアウトしている worktree のパス（`WORKTREE`）で渡す:

```bash
~/.claude/scripts/review-level.sh [branch]
```

差分が空だとスクリプトが止まる。出力の `LEVEL`（docs/ だけなら medium、他は high）と `WORKTREE` で
`/code-review <LEVEL> <WORKTREE>` を回し、指摘に対応してからマージする（`WORKTREE` は手順 1 の `TARGET_WORKTREE` と同じ）。
レビューが対象を見つけられなかった・実行できなかったときの返答は、指摘ゼロでなく未実施として扱う（`[]` が指摘ゼロだと
言えるのは、返答が差分を読んだと示しているときだけ）:

- 直す前に、指摘が依拠する事実を一次情報で確かめる（確かめ方は /gh-pr-review の「3.1 前提を一次情報で検証する」）。
  前提が誤っていれば、確かめた事実を対応しない理由にする。レビューも誤る（2026-10-24 に、数値の符号についての指摘を
  確かめずに取り込み、後で元のデータを見ると指摘が誤っていた）
- 直すのは検証できる誤りだけ（バグ、コードや事実と食い違う記述、壊れた参照）。直した結果、ADR の決定・帰結・確認に
  書いた内容が変わるなら、同じコミットで ADR も合わせる（確定した ADR はマージの後に直せない。/adr の「確定後の運用」の、
  マージ前の項）。
  修正したら、コミットして rebase の要否とチェックからやり直す
- claude-hooks の、普通に書く形（`cd <worktree> &&`、`env` や `timeout` の前置き、`bash -c '…'`、`nix develop -c` など、普通の手順で書くコマンド。
  珍しい git の設定や全体オプションで行き先を変える、pipefail を途中で外す、のような作為的な書き方は含まない）でない抜け道の指摘は直さず、無いものとして打ち切りを決める
  （hook は境界ではなく、抜け道を 1 つ塞ぐと次の反例が出て周が終わらない。抜け道はモデルの判断と人のレビューに任せる）。
  普通の形を誤って止める指摘は直す
- 直したら、もう一度回す。指摘の内容で打ち切る（レビューは周ごとに新しい候補を出すので、指摘ゼロを待たない）:
  - 精度（物理、数値、正しさ。記録と出力の一致）と速度（性能のリグレッション）に関わる指摘が出る限り、直して次の周を回す
  - ある周の指摘が文言・構成・置き場・将来の汎用化だけになったら、そこで打ち切る。その指摘は 1 つの issue にまとめ、
    対応しない理由を書く
  - 文書と数値だけの変更は、事実の誤りに絞った 1 周で終わる
  - 2026-10-14 に 13〜15 周回ったレビューが、文言の指摘で止まらず使用量を圧迫した
- /delegate の担当が報告でレビューの周と最後の周を回した HEAD の hash を示したブランチでは、親が回すかを下の手順で決める。
  担当のレビューを親が毎回繰り返すのは使用量の無駄だが、報告を読んだだけで済ませると、レビューを受けていない挙動の修正を
  見落とす（2026-10-28 に、担当が「確かめていないこと」に並べただけの修正を、親が読んで済ませた）。手順:
  - 担当が最後の周の HEAD を示していない、または hash が今のブランチの祖先でない（rebase で書き換わった場合は
    `git range-diff <古い土台>..<旧先端> main..[branch]` で対応するコミットに読み替える。読み替えられなければ）ときは回す
  - 示された HEAD（rebase で読み替えたときは読み替えたコミット）より後の変更を `git -C [TARGET_WORKTREE] diff <最後の周の HEAD> HEAD`
    で読み、挙動（コード、計算、hook の判定、テストの比べる範囲、スキルと hook の文書、記録の数値）を変える変更があれば回す。
    件名やファイル名だけで判断しない（`docs(…)` の件名でもテストの比べる範囲に触れ、記録のファイルでも数値を直すことがある）。
    挙動を変えない文書だけ（docs/ の説明、記録の本文など）なら回さず、その判断をマージの本文に書く
  - 回すときはブランチ全体を、`review-level.sh [branch]` の `LEVEL` と `WORKTREE` で回す（積み重なったブランチの `--base <土台のブランチ>`
    は下の項のとおり付ける）。`/code-review` は PR、ブランチ、パスを対象にして、ブランチ全体の差分を見る。最後の周の HEAD からの
    範囲は対象にならないので、`--base <最後の周の HEAD>` で LEVEL を決めると、見る範囲と LEVEL を決めた範囲がずれる
  - 担当の周が、そのブランチの初めてのレビューにあたる（初めてのレビューを省かない原則は変わらない）。ただし報告の周が
    指摘に対応して終わっているか（打ち切りの理由が書かれているか）は、報告から確かめる
  - 下の rebase 後の判断は、レビュー済みのコミットを回し直すかの規則で、上の判断（最後の周より後に挙動を変えるコミットがあるか）とは別
- 積み重なったブランチ（`STACKED_ON` が none でない）は、`review-level.sh` に `--base <土台のブランチ>` を付ける
- 対応しない指摘は、その理由をユーザーに伝える
- レビューを済ませたブランチを rebase した後は、変更の中身で回すかを決める（無駄なレビューは使用量を消す。初めてのレビューは
  省かない）。次の 2 つがそろえば回さない（チェックが通ることは「チェック」の段で済んでいる）: `git range-diff <古い土台>..<旧先端> main..[branch]` で
  自分のコミットが同一であること、rebase で入った main のコミット（`git log <古い土台>..main`）がブランチの変更と同じファイルにも
  関わる挙動にも触れないこと（ファイルは `git diff --name-only <古い土台> main` とブランチの変更ファイルの共通部分で、挙動はブランチが
  呼ぶ関数や型の名前を main の差分で検索して確かめる）。衝突を解いた、同じファイルや関わる挙動に触れた、のどちらかなら回す。回さないときは、マージの本文に range-diff と main の変更を比べた根拠を書く
  （ユーザーに判断を任されても、ユーザーが認めたとは書かず、「担当が range-diff と main の変更から判断した」と書く）

### マージコミットメッセージ

ブランチの内容を把握してから書く:

```bash
git log --format='%h %s%n%b' main..[branch]
git diff --stat main...[branch]
```

```text
Merge: [ブランチの変更を簡潔に要約]

## Why
[目的・背景を1-2文]

## What
- [主要な変更点を3-5項目]

## Impact
[影響するモジュール/機能]

Branch: [branch]
```

### マージ

必ずマージコミットを作る（GitHub での `--merge` と同じ履歴になる）。
`DEFAULT_WORKTREE` が none なら、今の worktree で `git switch main` してから行う。
main の worktree が feature ブランチのままだと、ブランチを自分自身にマージして何も起きない。
`git merge` は main の worktree から実行する（issue #372 の事故）。マージの直前に、
`git -C [DEFAULT_WORKTREE] symbolic-ref --short HEAD`（none なら今の worktree）が main であることを確かめ、
違えば止まる:

```bash
git -C [DEFAULT_WORKTREE] merge --no-ff [branch] -m "$(cat <<'EOF'
[マージコミットメッセージ]
EOF
)"
```

マージは hook（品質チェック、本文の語の検査）に止められることがある。成否は終了コードで判定するので、`git merge` の出力を
`tail` などにつながず（終了コードが消える）、後片付けとは別の呼び出しにする。通ったら、main の先頭がこのマージコミットであること
（`git -C [DEFAULT_WORKTREE] rev-parse HEAD^2`（none なら今の worktree）がブランチの先端と一致する）を確かめてから後片付けに進む。
マージは 1 本ずつ前景で行い（hook のチェックが数分かかるので、Bash の `timeout` を上限の 600000 ms にする）、git の書き込み（rebase、merge、commit）をバックグラウンドに重ねない。前景の上限（600 秒）を超えて
バックグラウンドに移ったコマンドがあるうちは、その終わりの通知まで同じリポジトリに git で書き込まない（2026-10-22 に、移った
マージの上に同じブランチの rebase とマージを重ねて競合し、main にインデックスの残骸が残った）。チェックの回し方は「チェック」の節。
拒まれると、マージはステージされたまま main の worktree に MERGE_HEAD が残る:

- 本文の語で拒まれたら、本文を直して `git -C [DEFAULT_WORKTREE] commit -m "…"` でそのマージを終える（品質チェックは
  もう一度走る）。`git merge` をやり直すと MERGE_HEAD があるため拒まれる
- 品質チェックで拒まれたら、`git -C [DEFAULT_WORKTREE] merge --abort` で取り消し、ブランチで直して「レビュー」の節のとおり
  rebase の要否とチェックからやり直す

## 後片付け

対象の worktree とブランチを削除する（未マージなら、何も消さずに止まる。変更が残っていれば失敗する。確認して対処する）:

```bash
~/.claude/scripts/post-merge-cleanup.sh [branch]
```

出力に `NOTE:` の行（rebuild か claude-sync が要る。判定は post-merge-cleanup.sh）があれば、完了メッセージでそのままユーザーに伝え、rebuild や claude-sync の実行はユーザーに任せる。

## 完了確認

```bash
git log --oneline --graph -10
```

```text
✅ [branch] を main にマージしました: [merge-commit-hash]

後片付け:
- worktree [path] を削除（該当する場合）
- ブランチ [branch] を削除
- [post-merge-cleanup の NOTE: の行]（出た場合）

次のステップ:
- 次の作業を始める → /git-branch
```
