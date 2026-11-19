---
name: git-merge
description: featureブランチをローカルでmainにマージ。GitHubを経由しないリポジトリ用。
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

GitHub リモートの無いリポジトリで、feature ブランチをローカルの main にマージする。/delegate の担当のブランチは、親の許可の後に
担当がこの手順でマージする（/delegate の受ける側の手順 10）。

## 現在の状態

!`~/.claude/scripts/git-merge-state.sh $ARGUMENTS`
!`git log --oneline --graph --all -20`

$ARGUMENTS のブランチ（省略時は今のブランチ）が対象。`ERROR:` の行があれば、その指示に従って止まる。

ブランチの操作は `git -C [TARGET_WORKTREE]`、マージは `git -C [DEFAULT_WORKTREE]` で行う（別の worktree にあるブランチへは
switch できない）。`TARGET_WORKTREE` が none なら `git -C [DEFAULT_WORKTREE] switch [branch]` で取り出して以降はそこを指し、
マージの前に `git -C [DEFAULT_WORKTREE] switch main` で戻す。`DEFAULT_WORKTREE` も none なら今の worktree を使う。

## 止まる条件

次のマージは hook が止める。原因を直して通す（CLAUDE.md の Git の、手順と hook は必須ゲートの項）:

- 新しい ADR を 2 件以上加えるブランチ（決定ごとにブランチを分ける。/topic-triage）
- マージ結果が品質チェックを通らない
- 本文にレビューの記録（「マージコミットメッセージ」の節の `Reviewed:` と指摘ごとの行）が無いか、`Reviewed:` の hash が
  ブランチの先端の祖先でない

## 判断が要る手順

下の節は、当てはまるものを上から順に通る。「proposed の ADR」の節は、rebase が要らないブランチでも、チェックの前に必ず通る。

### main がまだない（`DEFAULT: none`）

`ROOT_COMMITS` が 1 つで初期化だけの内容であることを `git show` で確かめてから、`git branch main [root-commit]` で作る。
初期化以外の変更を含めば、作らずにユーザーに確認する。

### 積み重なり（`STACKED_ON` が none でない）

土台のブランチから順にこの手順でマージし、対象は後で rebase する。

### rebase（`BEHIND` が 0 でない）

rebase の前に、古い土台（`git merge-base main [branch]`）と旧先端（`git rev-parse [branch]`）を控える（「rebase の後」の
`git range-diff <古い土台>..<旧先端> main..[branch]` と `git log <古い土台>..main` に使う。rebase の後では取れない）。同じ範囲を
触るブランチは並行に進めず、マージしてから次を始める。

```bash
git -C [TARGET_WORKTREE] rebase main
```

コンフリクトしたら内容と解決案を示し、`git rebase --continue` か `git rebase --abort` かを決める。

`remap.commitRefs` が true のリポジトリでは、rebase と `commit --amend` の後に post-rewrite の hook が文書の中のコミットの hash を
付け替え、作業ツリーに残す。次に進む前に、hook の出力に従って片付ける（後の手順で amend したときも）。

### 番号の重複（rebase の後）

設計の記録（docs/adr か docs/issues）のあるリポジトリでは、rebase の直後に `design-records -C [TARGET_WORKTREE] check` を実行し、
重複があれば出力に従って振り直してから次に進む（`renumber` にも `-C [TARGET_WORKTREE]` を付ける）。

### proposed の ADR（どのブランチでも、チェックの前に）

`PROPOSED_ADRS` は、ブランチの feat と fix のコミットが `Refs:` で挙げた ADR のうち、先端で `status: proposed` のもの。
`STALE_CONFIRMATIONS` は、ブランチで変えた ADR の「確認」に挙げたファイルのうち、その ADR を最後に変えたコミットより後に
変わったもの（`ADR-NNNN:<パス>`）で、挙がった ADR は決定、帰結、確認がそのファイルの今の実装と合うかを確かめて直す。
ブランチのコミットを変えた後（rebase、番号の振り直し、コミットの追加や amend）は `~/.claude/scripts/git-merge-state.sh [branch]` を
再実行して取り直す（読み込んだときの none や行の無さで、この手順を飛ばさない）。一覧に加えて、次も見る:

- このブランチが足すか直した proposed の ADR（`git diff --name-status main...[branch] -- docs/adr`）
- `Refs:` の無いコミットや perf などの型のコミットだけが実装した main の ADR（一覧にも diff にも出ないので、実装から見る）
- 番号を振り直した ADR（一覧から漏れる。前のコミットの `Refs:` に残る古い番号は main の別の ADR を指しうるので、一覧の ADR が
  このブランチの実装したものかを本文で確かめてから直す）

どれも無ければ、ここで決めることは無い。

実装をマージするブランチは、実装した ADR を同じブランチで実装に合わせて直し、accepted にする（accepted の ADR の凍結する部分
（/adr の「確定後の運用」）は、マージの後には「## 変更」でしか直せない）。マージの前に、ADR ごとに次のどれかを決めてコミットする
（hook は止めない）:

- 決定の全部を実装した: 決定と帰結を実装に合わせ、「確認」に実際のテストと結果を書いて、`status: accepted` と `date:` をその日にする
- 一部を実装した: 残りを「帰結」と issue に書き、「確認」を書いて accepted にする。または残りを決めるまで proposed のまま残す
- 試して、採らないと決めた: `status: rejected` と `date:` をその日にし、理由（試した結果）を「帰結」に書き、その実装をブランチから
  外す
- 試しただけでまだ決めない、または決定だけを足し実装は後のブランチで行う: proposed のまま残す

proposed のまま残した ADR は、マージの本文の Impact に番号と理由を書く。この手順で直した ADR は、docs/ だけの変更でも
（/delegate の担当の最後の周の後でも、「レビュー」の挙動を変えない文書の扱いに当てず）レビューを受けさせる。後から ADR や
ブランチのコミットを変えたら、rebase の要否、この手順の一覧の取り直し、チェックからやり直す。

### チェック（CI の代わり）

```bash
~/.claude/skills/language-checks/scripts/run-checks.sh -C [TARGET_WORKTREE] --base main
```

前景の上限（600 秒）を超えうるので `run_in_background` で回して終わりの通知を待ち、`git commit … && run-checks.sh` のように
つながない。失敗したら直してコミットし、「proposed の ADR」の一覧を取り直してからやり直す。通ってからマージする。

### レビュー（自動レビューの代わり）

```bash
~/.claude/scripts/review-level.sh [branch]
```

出力の `LEVEL` と `WORKTREE`（`TARGET_WORKTREE` と同じ）で `/code-review <LEVEL> <WORKTREE>` を回し、指摘に対応してから
マージする（引数なしやブランチ名では cwd のリポジトリを見て「指摘なし」と返る）。積み重なったブランチ（`STACKED_ON` が none で
ない）は、`review-level.sh` に `--base <土台のブランチ>` を付ける。

親が回す周は Opus で回す。親が Skill で呼ばず（/code-review は呼んだ側のモデルの fork で走る）、Agent（`model: "opus"`。fork は
`model` を無視するので fork でない型にし、description は `~/.claude/skills/delegate/references/requesting.md` の
「Agent の `description`」の項のとおり）で起動した担当に「`/code-review <LEVEL> <WORKTREE> [焦点]` を Skill で呼び、background で
走ったら終わりの通知を待ってから、結果をそのまま返す」と頼む。対象を見つけられなかった、実行できなかった返答は、指摘ゼロでなく
未実施として扱う（`[]` は差分を読んだと示しているときだけ指摘ゼロ）。

- 直す前に、指摘が依拠する事実を一次情報で確かめる（/gh-pr-review の「3.1 前提を一次情報で検証する」）。誤っていれば、確かめた
  事実を直さない理由にする
- 直した結果、ADR の凍結する部分（下の (b) の一覧）の内容が変わるなら、同じコミットで ADR も合わせる。帰結と確認は (c) に
  当たるものだけ blocking として直し、ほかはマージの後に直せる（/adr の「確定後の運用」）。直したらコミットして、rebase の要否、
  「proposed の ADR」の一覧の取り直し、チェックからやり直す
- claude-hooks の、普通に書く形（`cd <worktree> &&`、`env` や `timeout` の前置き、`bash -c '…'`、`nix develop -c` など、普通の
  手順で書くコマンド。珍しい git の設定や全体オプションで行き先を変える、pipefail を途中で外す、のような作為的な書き方は
  含まない）でない抜け道の指摘は直さず、無いものとして打ち切りを決める（hook は境界でなく、塞ぐたびに次の反例が出て周が
  終わらない）。普通の形を誤って止める指摘は直す
- 対応しない指摘の書き方は、下の「直す指摘の基準」の「直さない（non-blocking）」と「行」の項に従う

#### 直す指摘の基準

/delegate の担当と、/gh-pr-create と /gh-pr-review のローカルの `/code-review` の周もここに従う。Copilot の指摘も直すかはここで
決める（処置と返信、文書だけの PR の分け方は /gh-pr-review の「3.2 処置を決める」、周の終わりは同じスキルの Step 6）。直すかは、
重大度の自己申告でなく、報告を読む側が確かめられる再現で決める。

- 直す（blocking）:
  - (a) 通常の入力と流れで到達する誤動作、誤った数値、データの破損、セキュリティの欠陥、ゲートの迂回で、落ちるテストか
    再現する手順を先に書けるもの。スキルと hook の文書の、手順のとおりに実行すると失敗する誤り（壊れたパス、無い `VERDICT` の
    名前）も、その実行を再現の手順として数える
  - (b) ADR の凍結する部分（背景、決定の要因、検討した案、決定と理由、各案の長所と短所、表題、フロントマターの `requires` と
    `supersedes`）の事実の誤りで、一次情報で確かめたもの（マージの後に直すには「## 変更」が要る）
  - (c) issue、計画、検証の記録の本文と、ADR の凍結しない部分（帰結、確認など）が、引いた先の一次情報（表、比較の JSON、ログ、
    sacct、コミット）と食い違う値・符号・事実を指す語（ジョブの状態など）、または、そのリポジトリの CLAUDE.md か記録の規則が
    求める出どころ（コミットと日付など）の抜け（規則が無いリポジトリでは数えない）。再現は、食い違いなら出典、抜けなら求める
    項目と抜けている箇所と見つけた引いた先。言い回しの指摘と、引いた先が見つからない抜けは non-blocking。引いた先そのもの
    （検証の記録の表、計算の値）の誤りは (a) の誤った数値。どちらが誤りかは、引いた先が別の一次情報から描かれたもの（記録の表は
    比較の JSON から）ならその元を開いて決め、元と食い違えば (a)。それでも決められないものは、どの周でも issue に書いてユーザーに
    判断を求め、答えが来るまでマージしない。スキルと hook の文書と ADR の凍結する部分は (c) に入らず、(a) か (b) に当たるもの
    だけが blocking（実行しても失敗しないスキルの文の食い違いは non-blocking）
- 直さない（non-blocking）: それ以外の全部（契約の外の呼び出し、将来の形式の変更への備え、効率、重複、文言、置き場、スキルの文の
  改善）。issue にせず、指摘ごとに要旨を 1 文で、担当は報告の「対応しなかった指摘」に、親はマージの本文の Impact に
  `再現なし: <指摘の要旨>` の行で書く。/topic-triage の「振り分け」の表で別のトピックの行に当たる指摘のうち、読み返す場面
  （どの作業で開くか）を具体的に言えるものだけを、その行の「どうするか」のとおり issue にし、行を
  `再現なし → #<issue の番号>: <指摘の要旨>` にする。issue は 1 件に 1 つのトピックを、そのトピックの表題で書く（レビューの指摘を集める issue に
  しない）。担当が残したものも親の
  周では直さない。ただし (a)〜(c) に当たり親が再現を書けた指摘は、担当の分け方の誤りとして blocking に数えて直す
- 修正方法: 指摘の前提に加えて、直した後の文の数値と主張も、引いた先の一次情報で計算し直すか開き直して確かめてからコミットする
- 行: 直したものは「再現: <テスト名 | 手順 | 出典>」、直さないものは上の `再現なし` の行、前提が誤っていた指摘と無いものとして扱う
  hook の抜け道の指摘は確かめた事実か理由を、担当は報告に、親はマージの本文に「マージコミットメッセージ」の節の形で書く（親は
  担当の欄で分け方を確かめる）
- 直す主体: 親が見つけた blocking（親の周、担当の報告から親が分け方の誤りとして数えたもの）を直すのは、マージの許可を待っている
  担当（/delegate の受ける側の手順 9。終わっていない）があればその担当で、親は修正の指示を送る。無ければ、数行なら親が直し、
  大きければ新しい担当に同じブランチの続きとして頼む。報告を返して終わった担当は起こさない
- 終わり: 1 周目は必須。その周に blocking が無ければ終わり（指摘ゼロを待たない）。blocking を直したら、もう 1 周回すかを修正の
  大きさと危険で決める。ロジック（コード、hook の判定、テストの比べる範囲、スキルの手順の分かれ目、ADR の決定）を書き換えた
  修正は回す。数行か数文の修正で、直した後の文を引いた先の一次情報で確かめたもの（「修正方法」の項）は回さなくてよい。回すときは、
  直した箇所と修正方法を焦点に書き（`/code-review <LEVEL> <WORKTREE> <焦点>`。レビューする差分はブランチ全体）、レベルも決めて
  よい（修正を確かめるだけなら `low`）。回さないときは、その判断と理由を、担当は報告の「確かめたこと」に、親はマージの本文の
  Impact に書く。周の数では止めない。直した指摘が次の周にも再現できる形で出たら（前提が誤っている再出は「前提が誤り」で
  数えない）、直さずに issue に書いてユーザーに判断を求める（担当は報告の「マージの前後で人の手が要ること」に書く）

#### 担当が回したブランチ

/delegate の担当が報告でレビューの周と最後の周を回した HEAD の hash を示したブランチでは、親は最後の周の HEAD より後を
`git -C [TARGET_WORKTREE] diff <最後の周の HEAD> HEAD` で読み、挙動（コード、計算、hook の判定、テストの比べる範囲、スキルと
hook の文書、ADR（`commit-refs remap` だけの変更を除く）、記録の数値）を変える変更があれば、「終わり」の項の目安で回すかを
決める（件名やファイル名でなく差分の中身で判断する）。最後の周の HEAD が示されていないか、今のブランチの祖先でなく、rebase の
書き換えを `git range-diff <古い土台>..<旧先端> main..[branch]` で対応するコミットに読み替えられなければ回す。回すときは
ブランチ全体を `review-level.sh [branch]` の `LEVEL` と `WORKTREE` で回す（積み重なったブランチは `--base <土台のブランチ>` を
付ける）。担当の周の打ち切りの理由と、回さなかった周の理由を報告で確かめる。担当がマージの許可を待っているときは、回すことも
担当への指示にし、確かめ終えたら許可を送る（rebase、マージ、後片付けは担当が行う）。

#### rebase の後

レビューを済ませたブランチを rebase した後は、`git range-diff <古い土台>..<旧先端> main..[branch]` で自分のコミットが同一で、
rebase で入った main のコミット（`git log <古い土台>..main`）がブランチと同じファイル（`git diff --name-only <古い土台> main` との
共通部分）にも、関わる挙動（ブランチが呼ぶ関数や型の名前を main の差分で検索）にも触れなければ回さない。衝突を解いたか、
どちらかに触れたなら、「終わり」の項の目安で回すかを決める。回さないときは、本文に range-diff と main の変更を比べた根拠を書き、
`Reviewed:` に最後の周の HEAD に range-diff で対応する rebase 後の hash を書く。

### マージコミットメッセージ

`git log --format='%h %s%n%b' main..[branch]` と `git diff --stat main...[branch]` でブランチの内容を把握してから書く:

```text
Merge: [ブランチの変更を簡潔に要約]

## Why
[目的・背景を1-2文]

## What
- [主要な変更点を3-5項目]

## Impact
[影響するモジュール/機能。proposed のまま残した ADR があれば、番号と理由。レビューの周の数と、回さなかった判断の根拠（直した後、
rebase の後、担当の最後の周の後の変更）]
- 再現: [テスト名 | 手順 | 出典] — [直した指摘の要旨]
- 再現なし: [直さない指摘の要旨]
- 再現なし → #[issue の番号]: [別のトピックとして issue にした指摘の要旨]
- 前提が誤り: [確かめた事実] — [指摘の要旨]
- 抜け道: [理由] — [指摘の要旨]

Reviewed: [最後にレビューの周を回した HEAD の hash]
Branch: [branch]
```

- 指摘の行: 担当と親の全部の周の指摘を 1 行に 1 件、上の形のどれかで書く（先頭の `-` は任意）。どの周にも指摘が無ければ
  `指摘なし` の 1 行
- `Reviewed:`: 1 行だけ、7〜64 桁（SHA-256 のリポジトリは完全な 64 桁）の hash。最後にレビューの周を回した HEAD を書く
  （/delegate の担当の最後の周か、その後に親が回した周。親は許可にその hash を添える）。rebase で書き換わったときは、
  「rebase の後」のとおり range-diff で対応する hash を書く
- `Reviewed:` より後のコミットが挙動を変えうるファイルを変えていれば、hook は止めずにそのファイルを知らせる。知らせはマージの
  後に届くので、回さない判断と理由はマージの前に Impact に書いておく

### マージ

`git merge` は main の worktree から実行する。`DEFAULT_WORKTREE` が none なら、今の worktree で `git switch main` してから行う。
直前に `git -C [DEFAULT_WORKTREE] symbolic-ref --short HEAD`（none なら今の worktree）が main であることを確かめ、違えば止まる:

```bash
git -C [DEFAULT_WORKTREE] merge --no-ff [branch] -m "$(cat <<'EOF'
[マージコミットメッセージ]
EOF
)"
```

成否は終了コードで判定するので、出力を `tail` などにつながず、後片付けとは別の呼び出しにする。hook のチェックが数分かかるので、
マージは 1 本ずつ前景で、Bash の `timeout` を上限の 600000 ms にして行う。git の書き込み（rebase、merge、commit）を
バックグラウンドに重ねず、前景の上限を超えてバックグラウンドに移ったコマンドがあれば、その終わりの通知まで同じリポジトリに
git で書き込まない。通ったら後片付けに進む。hook に拒まれると、マージはステージされたまま main の worktree に MERGE_HEAD が残る:

- 本文の語で拒まれたら、本文を直して `git -C [DEFAULT_WORKTREE] commit -m "…"` でそのマージを終える（品質チェックはもう一度走る。
  `git merge` のやり直しは MERGE_HEAD で拒まれる）
- 品質チェックで拒まれたら、`git -C [DEFAULT_WORKTREE] merge --abort` で取り消し、ブランチで直して rebase の要否、
  「proposed の ADR」の一覧の取り直し、チェックからやり直す

## 後片付け

```bash
~/.claude/scripts/post-merge-cleanup.sh [branch] "$(git -C [DEFAULT_WORKTREE] rev-parse HEAD^1)"
```

第 2 引数はマージの前の main（マージコミットの第 1 親。`DEFAULT_WORKTREE` が none なら今の worktree）で、省かない（`NOTE:` の
判定の起点と、このブランチのマージコミットの確認に使う）。対象の worktree とブランチを削除する。cwd のリポジトリのブランチを片付けるので、別のリポジトリのブランチなら
`cd <そのリポジトリ> &&` を付けて呼ぶ。未マージなら何も消さずに止まり、変更が残っていれば失敗する（確認して対処する）。出力の `NOTE:` の行は、完了メッセージでそのまま
ユーザーに伝える。マージ済みの PR を痕跡にした `NOTE:` の行は、別のリポジトリのブランチを渡しているので、マージした
リポジトリで `cd <そのリポジトリ> &&` を付けてやり直す。

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
