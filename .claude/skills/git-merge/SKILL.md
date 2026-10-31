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

$ARGUMENTS のブランチ（省略時は現在のブランチ）が対象。上に `ERROR:` の行があれば（標準エラーに出るので、表示の最後とは限らない）、その指示に従う
（main 自身、GitHub リモートあり、未コミットの変更、ADR の status を読む design-records が無いときは、ここで止まる。
design-records の ERROR と、途中で失敗したときの ERROR は、ほかの行の後に出ることがあるので、その前の行を使って先に進まない）。
`ERROR:` の行が無くても、`fatal:` か `error:` が出たか、行の値が欠けたときは、その出力を使わずに原因を直して取り直す
（途中の失敗のうち、行の値を作るコマンドの失敗などは `ERROR:` を出さずに続く）。

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
- 本文に `Reviewed:` の行と指摘ごとの行（「マージコミットメッセージ」の節の形）が無いマージ。`Reviewed:` の hash が
  ブランチの先端の祖先でないマージと、その hash より後に、挙動を変えうるファイル（スキルと hook の文書と ADR を除く
  `.md`、`dvc.lock` と `.dvc`、コミットの hash の付け替えだけの変更、のどれでもないもの）を変えたマージ

## 判断が要る手順

下の節は、当てはまるものを上から順に通る。「proposed の ADR」の節は、rebase が要らないブランチでも、チェックの前に必ず通る。

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
できたコミットを `git show` で見て、付け替えた参照が振り直した記録を指していることを確かめてから、次の「proposed の ADR」に進む。

### proposed の ADR（どのブランチでも、チェックの前に）

`PROPOSED_ADRS` は、このブランチの feat と fix のコミットが本文の `Refs:` で挙げた ADR のうち、ブランチの先端でまだ
`status: proposed` のもの（docs/adr の無いリポジトリでは行が出ない）。上の「現在の状態」はスキルを読み込んだときの
ブランチの先端のものなので、ブランチのコミットを変えていなければその行で決め、変えた後（rebase、番号の振り直し、
コミットの追加や amend。レビューやチェックの指摘の修正を含む）は `~/.claude/scripts/git-merge-state.sh [branch]` を
再実行して、この行を取り直してから決める（rebase で、main で accepted になった ADR は一覧から外れ、main で入った
proposed の ADR は一覧に入りうる。rebase で docs/adr が入れば行も現れる。読み込んだときの none や行の無さで、この段を
飛ばさない）。一覧は feat と fix の `Refs:` に頼るので、このブランチが足すか直した proposed の ADR
（`git diff --name-status main...[branch] -- docs/adr`）も一覧と合わせて見る。`Refs:` を書き忘れたコミットや perf などの型の
コミットだけが実装した、main にある ADR は、一覧にもこの diff にも出ないので、ブランチの実装から自分で見る。使う行（取り直したときは取り直した行）が無いか none で、このブランチが proposed の
ADR を足しも直しもせず、番号も振り直していなければ、ここで決めることは無い。
番号を振り直した ADR は、前のコミットの `Refs:` に古い番号のまま残る。振り直した ADR は一覧から漏れるので、一覧が
none でも自分で見て下の選択肢から決める。古い番号は main の別の ADR を指しうるので、一覧の ADR がこのブランチの実装した
ものかを本文で確かめてから直す。

実装をマージするブランチは、実装した ADR を同じブランチで実装に合わせて直し、accepted にする。accepted になった
ADR の凍結する部分（/adr の「確定後の運用」）はマージの後には「## 変更」でしか直せず、誤った決定が決めた時点の記録として残るので、マージの前に、ADR ごとに次のどれかを決めてコミットする（hook は止めない。一部だけを実装したブランチや方法を試す
ブランチは proposed のまま残してよく、機械では決められないため）:

- 決定の全部を実装した: 決定と帰結を実装に合わせて直し、「確認」に実際のテストと結果を書いて、`status: accepted` と
  `date:` をその日にする
- 一部を実装した: 残りを「帰結」に書き、issue に残し、「確認」を書いてから accepted にする。または、残りを決めてから
  accepted にするために proposed のまま残す
- 試して、採らないと決めた: `status: rejected` と `date:` をその日にし、採らない理由（試した結果）を「帰結」に書く。
  採らなかった決定の実装はこのブランチから外す（残すと、rejected の ADR と main のコードが食い違ったまま確定する）
- 試しただけ、まだ決めない: proposed のまま残す
- 決定だけを足し、実装は後のブランチで行う: proposed のまま残す

proposed のまま残した ADR は、マージの本文の Impact に番号と理由を書く（次に誰が accepted にするかを読めるように）。
チェックとレビューの前に決めておくと、直した ADR も 1 回のチェックとレビューで見られる。この段で直した ADR は、
docs/ だけの変更でもレビューを受けさせる（/delegate の担当の最後の周の後に直したときも、「レビュー」の節の「挙動を変えない
文書だけなら回さない」に当てず回す。確定した ADR の凍結する部分は、マージの後には「## 変更」でしか直せない）。レビューの指摘で後から ADR を直したときや
ブランチのコミットを変えたとき（追加、amend）は、「レビュー」の節のとおり rebase の要否とチェックからやり直し、この段の一覧も取り直す。

### チェック（CI の代わり）

main との分岐点からの変更を検査する。マージ時にも hook が走るが、ここで先に直しておく:

```bash
~/.claude/skills/language-checks/scripts/run-checks.sh -C [TARGET_WORKTREE] --base main
```

失敗したら原因を直してコミットし、「proposed の ADR」の一覧を取り直してから、もう一度チェックする。通ってからマージする。

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
- 直すのは下の「直す指摘の基準」の blocking だけ。直した結果、ADR の凍結する部分（背景、決定の要因、検討した案、決定と理由、
  各案の長所と短所、表題、フロントマターの `requires` と `supersedes`）に書いた内容が変わるなら、同じコミットで ADR も合わせる
  （マージの後に凍結する部分を直すには「## 変更」が要り、誤った決定が前の文として記録に残る。だからマージの前に直す。帰結と
  確認はマージの後にも直せる。/adr の「確定後の運用」）。
  修正したら、コミットして rebase の要否、「proposed の ADR」の一覧の取り直し、チェックからやり直す
- claude-hooks の、普通に書く形（`cd <worktree> &&`、`env` や `timeout` の前置き、`bash -c '…'`、`nix develop -c` など、普通の手順で書くコマンド。
  珍しい git の設定や全体オプションで行き先を変える、pipefail を途中で外す、のような作為的な書き方は含まない）でない抜け道の指摘は直さず、無いものとして打ち切りを決める
  （hook は境界ではなく、抜け道を 1 つ塞ぐと次の反例が出て周が終わらない。抜け道はモデルの判断と人のレビューに任せる）。
  普通の形を誤って止める指摘は直す
- **直す指摘の基準**（/delegate の担当、/gh-pr-create と /gh-pr-review のローカルの `/code-review` の周もここに従う。Copilot の指摘は /gh-pr-review の規則のまま）: 直すかは、重大度の自己申告でなく、
  報告を読む側が確かめられる再現で決める（レビューも修正もモデルで、周ごとに新しい候補が出る。「精度に関わる」で分けた
  2026-10-29 の担当 3 本は 10 周、push した head でさらに 2 周（合計 12 周）、8 周、5 周回った。周の数は報告による。記録で数え直していない）:
  - 直す（blocking）: (a) 通常の入力と流れで到達する誤動作、誤った数値、データの破損、セキュリティの欠陥、ゲートの迂回で、
    落ちるテストか再現する手順を先に書けるもの。スキルと hook の文書の、手順のとおりに実行すると失敗する誤り（壊れたパス、
    無い `VERDICT` の名前）も、その実行を再現の手順として数える。(b) ADR の凍結する部分（背景、決定の要因、
    検討した案、決定と理由、各案の長所と短所、表題、フロントマターの `requires` と `supersedes`）の事実の誤りで、一次情報で
    確かめたもの（マージの後に直すには「## 変更」が要り、誤った決定が前の文として記録に残るので、マージの前に直す）
  - 直さない（non-blocking）: それ以外の全部（契約の外の呼び出し、将来の形式の変更への備え、効率、重複、文言、置き場、
    スキルの文の改善）。1 件の issue にまとめ、担当は報告の「対応しなかった指摘」、親はマージの本文の Impact に番号を書く。
    マージの後に別のトピックで扱い、担当が残したものも親の周では直さない（直した修正が新しい誤りを作ることがある）。
    ただし (a) か (b) に当たり、親が再現を書けた指摘は、担当の分け方の誤りとして blocking に数えて直す
  - 指摘ごとに、直したものは「再現: <テスト名 | 手順 | 出典>」、直さないものは「再現なし → <issue の番号>」を、担当は報告に、
    親はマージの本文に「マージコミットメッセージ」の節の形で書く（親は担当の欄を読んで分け方を確かめる。hook が本文の行を確かめる）。前提が誤っていた指摘と、無いものとして扱う hook の抜け道の
    指摘は issue にせず、確かめた事実か理由を書く
  - 終わり: その周に blocking が無ければ終わり（指摘ゼロを待たない）。blocking を直したら、下の歯止めの中で、直した差分を
    焦点に書いてもう 1 周回す（`/code-review <LEVEL> <WORKTREE> <直した箇所と直し方>`。レビューする差分はブランチ全体）
  - 歯止め: 1 つのブランチでローカルの `/code-review` は、担当と親がそれぞれ 3 周まで（親が自分で書いたブランチは親の 3 周）。
    重大度の自己申告がずれたときのためで、普段は届かない。対象を見つけられなかった回とエラーの回は数えない。数え直しはしない
    （追加の指示で変更が足されても、そのブランチで回した周の合計で数える）。rebase の後の周（親の衝突の後、担当の計算の前）も
    数え、3 周を使い切っていれば回さず、その変更を未レビューとして報告（親はマージの本文の Impact）に書く
  - 3 周目で blocking を直したら（担当も親も）: 直した head に記録のための 1 周を回し（3 周に数えない）、その周の指摘は直さない。
    直しを読んだだけでマージしない（2026-10-29 に nixfiles の PR #525 の 3 周目の後の 1 行の直しが事実の誤りを 4 件作り、記録の周が見つけた）。
    blocking（直しきれずに同じ指摘が出たものを含む）が無ければ、その head を記録して（リモートありでは `record-local-review.sh`、
    担当は報告の最後の周の HEAD）マージに進む。blocking があれば、直しを戻さず、マージせず、blocking を issue に書いてユーザーに
    判断を求める（担当は報告の「マージの前後で人の手が要ること」に書く）。マージの hook のエスケープは付けない。前の head に
    戻さないのは、最後の周の head には直していない blocking があり、blocking の無かった周の head に限っても、後の周が見つけた
    blocking がそのコードにあるか、その後の追加の変更を落とすため
- /delegate の担当が報告でレビューの周と最後の周を回した HEAD の hash を示したブランチでは、親が回すかを下の手順で決める。
  担当のレビューを親が毎回繰り返すのは使用量の無駄だが、報告を読んだだけで済ませると、レビューを受けていない挙動の修正を
  見落とす（2026-10-28 に、担当が「確かめていないこと」に並べただけの修正を、親が読んで済ませた）。手順:
  - 担当が最後の周の HEAD を示していない、または hash が今のブランチの祖先でない（rebase で書き換わった場合は
    `git range-diff <古い土台>..<旧先端> main..[branch]` で対応するコミットに読み替える。読み替えられなければ）ときは回す
  - 示された HEAD（rebase で読み替えたときは読み替えたコミット）より後の変更を `git -C [TARGET_WORKTREE] diff <最後の周の HEAD> HEAD`
    で読み、挙動（コード、計算、hook の判定、テストの比べる範囲、スキルと hook の文書、ADR、記録の数値）を変える変更があれば回す。
    件名やファイル名だけで判断しない（`docs(…)` の件名でもテストの比べる範囲に触れ、記録のファイルでも数値を直すことがある）。
    挙動を変えない文書だけ（ADR を除く docs/ の説明、記録の本文など）なら回さず、その判断をマージの本文に書く。本文の `Reviewed:` には
    担当の最後の周の HEAD（読み替えたときは読み替えたコミット）を書く。hook はその hash より後のファイルを見て、挙動を変えうる
    ものがあれば止める（線は「止まる条件」。記録の数値の変更は見分けないので、diff を読む判断はこの手順のまま）
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
  呼ぶ関数や型の名前を main の差分で検索して確かめる）。衝突を解いた、同じファイルや関わる挙動に触れた、のどちらかなら回す。回さないときは、マージの本文に range-diff と main の変更を比べた根拠を書き、
  `Reviewed:` には最後の周の HEAD に range-diff で対応する rebase 後のコミットの hash を書く（hook は本文の hash と先端だけを照らし、range-diff は見ない）
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
[影響するモジュール/機能。proposed のまま残した ADR があれば、番号と理由。レビューの周の数と、回さなかった判断の根拠（rebase の後、
担当の最後の周の後の文書）。未レビューの変更（歯止めの 3 周を使い切った後の変更）と、non-blocking の指摘（記録の周の指摘を含む）をまとめた issue の番号]
- 再現: [テスト名 | 手順 | 出典] — [直した指摘の要旨]
- 再現なし → #[issue の番号]: [直さない指摘の要旨]
- 前提が誤り: [確かめた事実] — [指摘の要旨]
- 抜け道: [理由] — [指摘の要旨]

Reviewed: [最後にレビューの周を回した HEAD の hash]
Branch: [branch]
```

レビューの記録の行は hook（本文の形と、`Reviewed:` より後のコミット）が確かめる。

- 指摘の行: 担当と親の全部の周の指摘を 1 行に 1 件、上の 4 つの形のどれかで書く（行の先頭の `-` は付けても付けなくてもよい）。
  どの周にも指摘が無ければ `指摘なし` の 1 行。1 つの文に括弧で入れた「（再現: …）」は行として読まれない
- `Reviewed:`: 1 行だけ。7〜40 桁の hash。/delegate の担当の報告の「最後の周を回した HEAD の hash」をそのまま書き、
  親が回したときは親の最後の周の HEAD を書く。rebase で書き換わったときは、「レビュー」の節の rebase 後の判断のとおり
  range-diff で対応する rebase 後の hash を書く
- `Reviewed:` より後のコミットが挙動を変えうるファイルを変えていれば hook が止める（線は「止まる条件」）。止まったら、
  ブランチ全体に回し直してその HEAD を書く。歯止めの 3 周を使い切った後の変更を未レビューとして Impact に書いてマージするときは、
  拒否の文言に従う

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
  rebase の要否、「proposed の ADR」の一覧の取り直し、チェックからやり直す

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
