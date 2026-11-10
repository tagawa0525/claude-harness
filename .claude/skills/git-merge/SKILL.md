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

GitHub リモートの無いリポジトリで、feature ブランチをローカルの main にマージする（PR の CI、自動レビュー、
`gh pr merge --merge` をローカルの手順に置き換えたもの）。

## 現在の状態

!`~/.claude/scripts/git-merge-state.sh $ARGUMENTS`
!`git log --oneline --graph --all -20`

$ARGUMENTS のブランチ（省略時は今のブランチ）が対象。`ERROR:` の行があれば、その指示に従う（main 自身、GitHub リモートあり、
未コミットの変更、design-records が無いときはここで止まる。`ERROR:` は標準エラーに出てほかの行の後に来ることがあるので、
その前の行も使わない）。`ERROR:` が無くても、`fatal:` か `error:` が出たか行の値が欠けたら、その出力を使わずに原因を直して
取り直す（行の値を作るコマンドの失敗は `ERROR:` を出さずに続く）。

別の worktree にあるブランチへは switch できないので、ブランチの操作は `git -C [TARGET_WORKTREE]`、マージは
`git -C [DEFAULT_WORKTREE]` で行う。`TARGET_WORKTREE` が none なら `git -C [DEFAULT_WORKTREE] switch [branch]` で取り出して
以降はそこを指し、マージの前に `git -C [DEFAULT_WORKTREE] switch main` で戻す。`DEFAULT_WORKTREE` も none なら今の worktree を使う。

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

`ROOT_COMMITS` が 1 つで初期化だけの内容であることを `git show` で確かめてから、`git branch main [root-commit]` で作る。
初期化以外の変更を含めば、作らずにユーザーに確認する。

### 積み重なり（`STACKED_ON` が none でない）

土台のブランチから順にこの手順でマージし、対象は後で rebase する。

### rebase（`BEHIND` が 0 でない）

rebase の前に、古い土台（`git merge-base main [branch]`）と旧先端（`git rev-parse [branch]`）を控える（「レビュー」の rebase 後の判断で
`git range-diff <古い土台>..<旧先端> main..[branch]` と `git log <古い土台>..main` に使う。rebase の後では取れない）。同じ範囲を触るブランチは並行に進めず、マージしてから次を始める（並行が多いほど main が
進み、rebase とレビューのやり直しが増える）。

```bash
git -C [TARGET_WORKTREE] rebase main
```

コンフリクトしたら内容と解決案を示し、`git rebase --continue` か `git rebase --abort` かを決める。

`remap.commitRefs` が true のリポジトリでは、rebase と `commit --amend` の後に post-rewrite の hook が文書の中のコミットの hash を
付け替え、作業ツリーに残す。hook の出力に従って片付けてから次に進む（後の手順で amend したときも。残すと、文書は main の履歴に
無い hash を指したまま main に入る）:

- `📝 置き換えはコミットしていません`: `git -C [TARGET_WORKTREE] diff` で旧と新の hash の置き換えだけと確かめ、きっかけを
  書いた件名（`chore: remap the commit refs after the rebase` など）でコミットする
- `❌ コミットの番号を付け直せませんでした`: 先に `git -C [TARGET_WORKTREE] diff` で書き換え済みの分（ERROR の最後の行に出る）を
  見て、原因を直し、出力の修正方法の `commit-refs remap <保存した対応>` を `cd [TARGET_WORKTREE] &&` を付けて実行し（対応は
  二度と渡されない）、上と同じく確かめてコミットする

### 番号の重複（rebase の後）

設計の記録（docs/adr か docs/issues）のあるリポジトリでは、rebase の直後に `design-records -C [TARGET_WORKTREE] check` で確かめる
（重複は `number NNNN is also used by ...`）。このブランチで足した側を `design-records -C [TARGET_WORKTREE] renumber NNNN`
（issue は `design-records -C [TARGET_WORKTREE] renumber --kind issue NNNN`）で振り直し（予約、改名、参照の付け替え、コミットまで行う）、`git show` で付け替えた参照が
振り直した記録を指すと確かめてから次に進む。

### proposed の ADR（どのブランチでも、チェックの前に）

`PROPOSED_ADRS` は、ブランチの feat と fix のコミットが `Refs:` で挙げた ADR のうち、先端で `status: proposed` のもの。
ブランチのコミットを変えた後（rebase、番号の振り直し、コミットの追加や amend）は `~/.claude/scripts/git-merge-state.sh [branch]` を
再実行して取り直す（読み込んだときの none や行の無さで、この段を飛ばさない）。一覧に加えて、次も見る:

- このブランチが足すか直した proposed の ADR（`git diff --name-status main...[branch] -- docs/adr`）
- `Refs:` の無いコミットや perf などの型のコミットだけが実装した main の ADR（一覧にも diff にも出ないので、実装から見る）
- 番号を振り直した ADR（一覧から漏れる。前のコミットの `Refs:` に残る古い番号は main の別の ADR を指しうるので、一覧の ADR が
  このブランチの実装したものかを本文で確かめてから直す）

どれも無ければ、ここで決めることは無い。

実装をマージするブランチは、実装した ADR を同じブランチで実装に合わせて直し、accepted にする。accepted になった
ADR の凍結する部分（/adr の「確定後の運用」）はマージの後には「## 変更」でしか直せず、誤った決定が決めた時点の記録として残るので、
マージの前に、ADR ごとに次のどれかを決めてコミットする（hook は止めない。一部だけを実装したブランチや方法を試すブランチは
proposed のまま残してよく、機械では決められないため）:

- 決定の全部を実装した: 決定と帰結を実装に合わせ、「確認」に実際のテストと結果を書いて、`status: accepted` と `date:` をその日にする
- 一部を実装した: 残りを「帰結」と issue に書き、「確認」を書いて accepted にする。または残りを決めるまで proposed のまま残す
- 試して、採らないと決めた: `status: rejected` と `date:` をその日にし、理由（試した結果）を「帰結」に書き、その実装をブランチから
  外す（残すと rejected の ADR と main のコードが食い違ったまま確定する）
- 試しただけでまだ決めない、または決定だけを足し実装は後のブランチで行う: proposed のまま残す

proposed のまま残した ADR は、マージの本文の Impact に番号と理由を書く。この段で直した ADR は、docs/ だけの変更でも
（/delegate の担当の最後の周の後でも、「レビュー」の挙動を変えない文書の扱いに当てず）レビューを受けさせる。後から ADR や
ブランチのコミットを変えたら、「レビュー」の節のとおり rebase の要否、この段の一覧の取り直し、チェックからやり直す。

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

出力の `LEVEL` と `WORKTREE`（`TARGET_WORKTREE` と同じ）で `/code-review <LEVEL> <WORKTREE>` を
回し、指摘に対応してからマージする（引数なしやブランチ名では cwd のリポジトリを見て「指摘なし」と返る。差分が空ならスクリプトが
止まる）。積み重なったブランチ（`STACKED_ON` が none でない）は、`review-level.sh` に `--base <土台のブランチ>` を付ける。

親が回す周は Opus で回す（周の費用を Opus の担当の周にそろえるため）。/code-review は呼んだ側のモデルの fork で走るので、
親が Skill で呼ばず、Agent（`model: "opus"`。fork は `model` を無視するので fork でない型にし、description は
`~/.claude/skills/delegate/references/requesting.md` の「Agent の `description`」の項のとおり）で起動した担当に「`/code-review <LEVEL> <WORKTREE> [焦点]` を Skill で
呼び、background で走ったら終わりの通知を待ってから、結果をそのまま返す」と頼む。対象を見つけられなかった、実行できなかった
返答は、指摘ゼロでなく未実施として扱う（`[]` は差分を読んだと示しているときだけ指摘ゼロ）。

- 直す前に、指摘が依拠する事実を一次情報で確かめる（/gh-pr-review の「3.1 前提を一次情報で検証する」）。誤っていれば、確かめた
  事実を直さない理由にする（レビューも誤る）
- 直した結果、ADR の凍結する部分（背景、決定の要因、検討した案、決定と理由、各案の長所と短所、表題、フロントマターの `requires` と
  `supersedes`）の内容が変わるなら、同じコミットで ADR も合わせる（マージの後は「## 変更」が要り、誤った決定が前の文として残る）。
  帰結と確認は (c) に当たるものだけ blocking として直し、ほかはマージの後に直せる（/adr の「確定後の運用」）。直したらコミットして、
  rebase の要否、「proposed の ADR」の一覧の取り直し、チェックからやり直す
- claude-hooks の、普通に書く形（`cd <worktree> &&`、`env` や `timeout` の前置き、`bash -c '…'`、`nix develop -c` など、普通の手順で書くコマンド。
  珍しい git の設定や全体オプションで行き先を変える、pipefail を途中で外す、のような作為的な書き方は含まない）でない抜け道の指摘は直さず、無いものとして打ち切りを決める
  （hook は境界ではなく、抜け道を 1 つ塞ぐと次の反例が出て周が終わらない。抜け道はモデルの判断と人のレビューに任せる）。
  普通の形を誤って止める指摘は直す
- 対応しない指摘は、その理由をユーザーに伝える

#### 直す指摘の基準

/delegate の担当と、/gh-pr-create と /gh-pr-review のローカルの `/code-review` の周もここに従う。Copilot の指摘も直すかはここで
決める（処置と返信、文書だけの PR の分け方は /gh-pr-review の「3.2 処置を決める」、周の終わりは同じスキルの Step 6）。直すかは、重大度の自己申告でなく、報告を読む側が確かめられる再現で決める（レビューも修正もモデルで、周ごとに
新しい候補が出るので、自己申告で分けると周が終わらない）。

- 直す（blocking）:
  - (a) 通常の入力と流れで到達する誤動作、誤った数値、データの破損、セキュリティの欠陥、ゲートの迂回で、落ちるテストか
    再現する手順を先に書けるもの。スキルと hook の文書の、手順のとおりに実行すると失敗する誤り（壊れたパス、無い `VERDICT` の
    名前）も、その実行を再現の手順として数える
  - (b) ADR の凍結する部分（背景、決定の要因、検討した案、決定と理由、各案の長所と短所、表題、フロントマターの `requires` と
    `supersedes`）の事実の誤りで、一次情報で確かめたもの（マージの後に直すには「## 変更」が要り、誤った決定が前の文として
    記録に残るので、マージの前に直す）
  - (c) issue、計画、検証の記録の本文と、ADR の凍結しない部分（帰結、確認など）が、引いた先の一次情報（表、比較の JSON、ログ、
    sacct、コミット）と食い違う値・符号・事実を指す語（ジョブの状態など）、または、そのリポジトリの CLAUDE.md か記録の規則が
    求める出どころ（コミットと日付など）の抜け（規則が無いリポジトリでは数えない）。再現は、食い違いなら出典、抜けなら求める
    項目と抜けている箇所と見つけた引いた先。(a) と (b) と同じく blocking に数える（記録の周の後の扱いは「3 周目で blocking を
    直したら」の例外の項）。言い回しの指摘と、引いた先が見つからない抜けは non-blocking。引いた先そのもの
    （検証の記録の表、計算の値）の誤りは (a) の誤った数値。どちらが誤りかは、引いた先が別の一次情報から描かれたもの（記録の表は
    比較の JSON から）ならその元を開いて決め、元と食い違えば (a)。それでも決められないものは、どの周でも issue に書いてユーザーに
    判断を求め、答えが来るまでマージしない。スキルと hook の文書と ADR の凍結する部分は (c) に入らず、(a) か (b) に当たるもの
    だけが blocking（実行しても失敗しないスキルの文の食い違いは non-blocking）
- 直さない（non-blocking）: それ以外の全部（契約の外の呼び出し、将来の形式の変更への備え、効率、重複、文言、置き場、スキルの文の
  改善）。issue にせず（読み返す場面が無く、一覧を埋める）、指摘ごとに要旨を 1 文で、担当は報告の「対応しなかった指摘」に、親は
  マージの本文の Impact に `再現なし: <指摘の要旨>` の行で書く。読み返す場面（どの作業で開くか）を具体的に言える別のトピック
  （/topic-triage の「振り分け」）と判断したものだけ issue にし、行を `再現なし → #<issue の番号>: <指摘の要旨>` にする。担当が
  残したものも親の周では直さない（直しが新しい誤りを作りうる）。ただし (a)〜(c) に当たり親が再現を書けた指摘は、担当の分け方の
  誤りとして blocking に数えて直す
- 修正方法: 指摘の前提に加えて、直した後の文の数値と主張も、引いた先の一次情報で計算し直すか開き直して確かめてからコミットする
  （直した文そのものが次の周の blocking になるため）
- 行: 直したものは「再現: <テスト名 | 手順 | 出典>」、直さないものは上の `再現なし` の行、前提が誤っていた指摘と無いものとして扱う
  hook の抜け道の指摘は確かめた事実か理由を、担当は報告に、親はマージの本文に「マージコミットメッセージ」の節の形で書く（親は
  担当の欄で分け方を確かめ、hook が本文の行を確かめる）
- 直す主体: 親が見つけた blocking（親の周、記録の周と記録の周の後の例外、担当の報告から親が分け方の誤りとして数えたもの）を
  直すのは、親か新しい担当。数行なら親が直し、大きければ新しい担当に同じブランチの
  続きとして頼む。報告を返して終わった担当を起こして直させない（止まった後の再開は伸びた文脈の全体を書き直し、その後も毎回
  読み直す）
- 終わり: その周に blocking が無ければ終わり（指摘ゼロを待たない）。直したら、歯止めの中で、直した箇所と修正方法を焦点に書いて
  もう 1 周回す（`/code-review <LEVEL> <WORKTREE> <焦点>`。レビューする差分はブランチ全体）
- 歯止め: 1 つのブランチの周の上限は `review-level.sh` の `ROUNDS:` で、hook（cap-review-rounds）が担当と親の周の合計を
  数えて、`ROUNDS` と記録の周と例外の周の分を超える呼び出しを止める（数え方は拒否の文言にある）。hook が止めたら、残った
  blocking を issue に書いてユーザーに判断を求める（担当は報告の「マージの前後で人の手が要ること」に書く）。周を使い切った後の
  変更は回さず、未レビューとして報告（親はマージの本文の Impact）に書く
- 文書だけのブランチ: `review-level.sh` が `ROUNDS: 1` を出すブランチ（対象はスクリプトの先頭のコメント）。その周に blocking を
  直したら、「3 周目で blocking を直したら」の 3 周目をその 1 周と読み替えて記録の周を回す。(c) だけで、直した head をマージの hook が記録の周なしに通す（下の文のとおり）なら、親が blocking ごとに引いた先の
  一次情報で確かめ、「直す主体」の項のとおり直し（決められないものは (c) の定義のとおり）、周を回さずにマージする。周を回さないのは、これらの文書は凍結せずマージの後も直せるので、周を重ねて取り切るより後で直す方が安いため。(a) か (b) があるか、マージの hook が通さない直し（リモートなしでは ADR の直し、リモートありでは
  docs/ の外を含む PR の直し）なら、「3 周目で blocking を直したら」の例外の項のとおり例外の周を回す。リモートなしでは `Reviewed:` に記録の周の head を書き、直した箇所と
  出典を Impact に書く（マージの hook は `Reviewed:` より後の、スキルと hook の文書と ADR を除く `.md` の変更を通す）。リモート
  ありでは、docs/ の下だけの PR は自動レビューが 1 度成功していれば、記録の無い head でも /gh-pr-merge の hook が通す
- 3 周目で blocking を直したら（担当も親も）: 直した head に記録のための 1 周を回し（3 周に数えない）、その周の指摘は直さない。
  直しを読んだだけでマージしない（直しが作った誤りが残るため）。blocking（直しきれずに同じ指摘が出たものを含む）が無ければ、
  その head を記録して（リモートありでは `record-local-review.sh`、担当は報告の最後の周の HEAD）マージに進む。あれば、直しを
  戻さず、マージせず、blocking を issue に書いてユーザーに判断を求める（担当は報告の「マージの前後で人の手が要ること」に書く）。
  マージの hook のエスケープは付けない。前の head に戻さないのは、最後の head には直していない blocking があり、blocking の無かった
  周の head にも後の周が見つけた blocking があるか、その後の変更を落とすため
  - 例外（確かめた誤りのとき）: 記録の周の blocking を確かめて、(a)〜(c) のどれかに当たるときの手順（言い回しの指摘は
    non-blocking なので入らない）。文書だけのブランチは、「文書だけのブランチ」の項が周を回さずにマージする (c) の直しでは入らない。担当は自分の記録の周で blocking が出たら、例外に
    入るかによらず直さず、報告の「マージの前後で人の手が要ること」に blocking と引いた先を書いて止まる（「対応しなかった指摘」には
    書かない）。親は blocking ごとに、指摘の前提と再現（(a) は落ちるテストか再現する手順、(b) と (c) は引いた先の一次情報）を確かめ
    （(c) のどちらが誤りかの決め方と決められないときの扱いは (c) の定義のとおり）、決められないものが無ければ、ユーザーの判断を待たずに「直す主体」の項のとおり直す。決められない
    ものがあれば、答えを待ってから、確かめられたものと答えで直すものをまとめて直す（答えで直したものの行は「再現: <出典>」）。
    直した head に、直した箇所を焦点に例外の周を 1 周回し、記録する head はその後に取り直す。例外の周は 3 周に
    数えず（3 周と記録の周の後に来るので、数えると 1 周も回せない）、その周の blocking も同じく扱い（確かめた誤りならまた確かめて
    直して回し、無ければその head を記録してマージに進む）、hook の上限まで回す。上限の周に blocking が出たら（次の周を hook が止める）直さず、上のとおり
    ユーザーに判断を求める。直した指摘が次の周にも再現できる形で出たときも同じ（前提が誤っている再出は「前提が誤り」で数えない）。どちらも直しが収束していない
    合図で、続けても周が終わらないため。(a) と (b) も入れるのは、どれも再現か一次情報で確かめた誤りで、直しが作りうる誤りは
    直した箇所を焦点にした次の周が読み、ユーザーに聞く往復と、別の名前のブランチで周を数え直す回り道が誤りの大きさに見合わない
    ため（言い回しの指摘で周が重なることは、blocking の定義と hook の上限が止める）。直した誤りと周の数（3 周、記録の周、例外の周）はマージの本文の Impact に書く

#### 担当が回したブランチ

/delegate の担当が報告でレビューの周と最後の周を回した HEAD の hash を示したブランチでは、親が回すかを次で決める（毎回
繰り返すと使用量の無駄で、報告を読むだけで済ませると、レビューを受けていない挙動の修正を見落とす）:

- 最後の周の HEAD が示されていないか、今のブランチの祖先でなく、rebase の書き換えを
  `git range-diff <古い土台>..<旧先端> main..[branch]` で対応するコミットに読み替えられなければ回す
- 示された HEAD（読み替えたコミット）より後を `git -C [TARGET_WORKTREE] diff <最後の周の HEAD> HEAD` で読み、挙動（コード、計算、
  hook の判定、テストの比べる範囲、スキルと hook の文書、ADR（`commit-refs remap` だけの変更を除く）、記録の数値）を変える変更が
  あれば回す。件名やファイル名だけで判断しない（`docs(…)` の件名でもテストの比べる範囲に触れ、記録のファイルでも数値を直す
  ことがある）。挙動を変えない文書だけ（ADR を除く docs/ の説明、記録の本文など）なら回さず、その判断をマージの本文に書き、
  `Reviewed:` に担当の最後の周の HEAD（読み替えたコミット）を書く（hook は記録の数値の変更を見分けないので、diff を読む判断は
  ここで行う）。記録の周の後の例外で直したコミットに回す周は、その例外の項のとおり 3 周に数えない（この手順より例外の項を
  優先する）
- 回すときはブランチ全体を `review-level.sh [branch]` の `LEVEL` と `WORKTREE` で回す（積み重なったブランチの `--base <土台のブランチ>`
  は「レビュー」の節の初めの段落のとおり付ける。/code-review はブランチ全体を見るので、
  `--base <最後の周の HEAD>` で LEVEL を決めると、見る範囲とずれる）
- 担当の周をそのブランチの初めてのレビューとし、周が指摘に対応して終わったか（打ち切りの理由）を報告で確かめる

#### rebase の後

レビューを済ませたブランチを rebase した後は、次の 2 つがそろえば回さない（初めてのレビューは省かない。上の、最後の周より後の
コミットの判断とは別）: `git range-diff <古い土台>..<旧先端> main..[branch]` で自分のコミットが同一であること、rebase で入った main の
コミット（`git log <古い土台>..main`）がブランチと同じファイル（`git diff --name-only <古い土台> main` との共通部分）にも、関わる挙動
（ブランチが呼ぶ関数や型の名前を main の差分で検索）にも触れないこと。衝突を解いたか、どちらかに触れたなら回す。回さないときは、
本文に range-diff と main の変更を比べた根拠を書き（ユーザーに判断を任されても「担当が range-diff と main の変更から判断した」と
書く）、`Reviewed:` に最後の周の HEAD に range-diff で対応する rebase 後の hash を書く（hook は本文の hash と先端だけを照らす）。

### マージコミットメッセージ

`git log --format='%h %s%n%b' main..[branch]` と `git diff --stat main...[branch]` でブランチの内容を把握してから書く:

```text
Merge: [ブランチの変更を簡潔に要約]

## Why
[目的・背景を1-2文]

## What
- [主要な変更点を3-5項目]

## Impact
[影響するモジュール/機能。proposed のまま残した ADR があれば、番号と理由。レビューの周の数と、回さなかった判断の根拠（rebase の後、
担当の最後の周の後の文書）。未レビューの変更（歯止めを使い切った後の変更）]
- 再現: [テスト名 | 手順 | 出典] — [直した指摘の要旨]
- 再現なし: [直さない指摘の要旨]
- 再現なし → #[issue の番号]: [別のトピックとして issue にした指摘の要旨]
- 前提が誤り: [確かめた事実] — [指摘の要旨]
- 抜け道: [理由] — [指摘の要旨]

Reviewed: [最後にレビューの周を回した HEAD の hash]
Branch: [branch]
```

レビューの記録の行は hook（本文の形と、`Reviewed:` より後のコミット）が確かめる:

- 指摘の行: 担当と親の全部の周の指摘を 1 行に 1 件、上の形のどれかで書く（先頭の `-` は任意）。どの周にも指摘が無ければ
  `指摘なし` の 1 行。1 つの文に括弧で入れた「（再現: …）」は行として読まれない
- `Reviewed:`: 1 行だけ、7〜40 桁の hash。/delegate の担当の報告の「最後の周を回した HEAD の hash」をそのまま書き、親が回したときは
  親の最後の周の HEAD を書く。rebase で書き換わったときは、「rebase の後」のとおり range-diff で対応する hash を書く
- `Reviewed:` より後のコミットが挙動を変えうるファイルを変えていれば hook が止める（線は「止まる条件」）。止まったらブランチ全体に
  回し直してその HEAD を書く。歯止めを使い切った後の変更を未レビューとして Impact に書いてマージするときは、拒否の文言に従う

### マージ

必ずマージコミットを作る（GitHub の `--merge` と同じ履歴）。`git merge` は main の worktree から実行する（main の worktree が
feature ブランチのままだと、ブランチを自分自身にマージして何も起きない）。`DEFAULT_WORKTREE` が none なら、今の worktree で
`git switch main` してから行う。直前に `git -C [DEFAULT_WORKTREE] symbolic-ref --short HEAD`（none なら今の worktree）が main で
あることを確かめ、違えば止まる:

```bash
git -C [DEFAULT_WORKTREE] merge --no-ff [branch] -m "$(cat <<'EOF'
[マージコミットメッセージ]
EOF
)"
```

成否は終了コードで判定するので、出力を `tail` などにつながず、後片付けとは別の呼び出しにする。hook のチェックが数分かかるので、
マージは 1 本ずつ前景で、Bash の `timeout` を上限の 600000 ms にして行う。git の書き込み（rebase、merge、commit）をバックグラウンドに
重ねず、前景の上限を超えてバックグラウンドに移ったコマンドがあれば、その終わりの通知まで同じリポジトリに git で書き込まない
（重ねると競合し、main にインデックスの残骸が残る）。通ったら、`git -C [DEFAULT_WORKTREE] rev-parse HEAD^2`（none なら今の
worktree）がブランチの先端と一致することを確かめてから後片付けに進む。hook に拒まれると、マージはステージされたまま main の
worktree に MERGE_HEAD が残る:

- 本文の語で拒まれたら、本文を直して `git -C [DEFAULT_WORKTREE] commit -m "…"` でそのマージを終える（品質チェックはもう一度走る。
  `git merge` のやり直しは MERGE_HEAD で拒まれる）
- 品質チェックで拒まれたら、`git -C [DEFAULT_WORKTREE] merge --abort` で取り消し、ブランチで直して「レビュー」の節のとおり rebase の
  要否、「proposed の ADR」の一覧の取り直し、チェックからやり直す

## 後片付け

```bash
~/.claude/scripts/post-merge-cleanup.sh [branch]
```

対象の worktree とブランチを削除する。未マージなら何も消さずに止まり、変更が残っていれば失敗する（確認して対処する）。cwd の
リポジトリにブランチの痕跡（ローカルのブランチ、worktree、リモート追跡の ref、リモートのブランチ）が無ければ何も変えずに止まるので、
別のリポジトリのブランチなら `cd <そのリポジトリ> &&` を付けて呼ぶ。出力の `NOTE:` の行（rebuild か claude-sync が要る。判定は
post-merge-cleanup.sh）は、完了メッセージでそのままユーザーに伝える。マージ済みの PR を痕跡にした `NOTE:` の行（cwd が GitHub の
リポジトリのときに出る）は、別のリポジトリのブランチを渡している（ブランチ名の重なり）ので、マージしたリポジトリで
`cd <そのリポジトリ> &&` を付けてやり直す。

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
