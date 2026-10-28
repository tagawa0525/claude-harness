---
name: gh-pr-review
description: PRレビューコメントを確認し対応。コード修正、返信を実行。
# model 未指定: コード修正を伴うためセッションモデルを継承する
argument-hint: [PR の URL | PR番号 | コメントURL] [--unresolved]
allowed-tools:
  - Bash(git *)
  - Bash(gh *)
  - Bash(~/.claude/skills/gh-pr-review/scripts/*)
  - Bash(~/.claude/skills/language-checks/scripts/run-checks.sh)
  - Bash(~/.claude/scripts/gh-actions-diagnose.sh*)
  - Bash(~/.claude/scripts/review-level.sh*)
  - Read
  - Edit
  - Glob
  - Grep
  - WebSearch
  - WebFetch
  - Agent
---

# GitHub PR Review Command

PRについたレビューコメントを確認し、対応する。取得・集計・状態判定は
`~/.claude/skills/gh-pr-review/scripts/` のスクリプトが行う。数える・比べる・探すは
記憶に頼らずスクリプトの出力を使い、判断だけをモデルが行う。

## 事前確認

!`gh auth status 2>&1 | head -3`
!`git status --short`
!`git branch -vv | head -5`

## Step 1: 対象の特定

スクリプトは対象の PR を URL（`https://github.com/{owner}/{repo}/pull/{n}`。以下 `{pr_url}`）で受け、
cwd のリポジトリに依らずその PR を読み書きする。

- コメント URL（`…/pull/{n}#discussion_r{id}`）→ URL を渡す。出力の `number` と `comment_id` の
  コメントだけに対応する。`{pr_url}` は出力の `url`
- PR の URL → その PR の全コメント
- PR 番号・引数なし → PR のリポジトリの worktree で `cd <worktree> && gh pr view [{番号}] --json url -q .url`
  で URL を得る。出力の owner/repo が対象のリポジトリかを確かめてから使う

```bash
~/.claude/skills/gh-pr-review/scripts/get-pr-info.sh "{pr_url または コメント URL}"
```

## Step 2: レビューコメントの取得

```bash
~/.claude/skills/gh-pr-review/scripts/get-review-comments.sh "{pr_url}" [--unresolved]
~/.claude/skills/gh-pr-review/scripts/get-latest-review.sh "{pr_url}"
```

`get-latest-review.sh` の **Suppressed comments** は Copilot が低確度と判断した指摘で、
レビュー本文にだけ現れる（スレッドがないので返信できない）。扱いは Step 6 の基準に従う。

**Previously missed**（`--- previously missed ---` 以降）は、前の周で見落とした、変わっていないコードへの
指摘で、これも本文にだけ現れる。重大度を持つ通常の指摘として Step 3 で処置を決め、台帳に載せる
（返信と resolve はできない）。`REMAINING_HIGH` は、開いている指摘とこの節の High の数。

## Step 3: 前提の検証と処置の決定

### 3.1 前提を一次情報で検証する

bot の指摘は前提が誤っていることがある。**着手前に、指摘が依拠する事実を一次情報で
確かめる**: コードの実際の挙動（呼び出し元まで読む・実行する）、公式ドキュメントや
ライブラリのソース（バージョンは lock ファイルで確認）、CLAUDE.md や `git log -S` の経緯、文書や記録に書いた数値なら
その元の出力やデータ。
直すのも見送るのも、確かめた結果で決める。

### 3.2 処置を決める

判断が付かないものは escalate に倒す:

| 処置         | 条件                                                                                              | 行動                                                                                    |
| ------------ | ------------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------- |
| **fix**      | 検証の結果、指摘が正しく、修正が PR の範囲内に収まる                                              | Step 4 で修正・コミット → Step 5 で `Fixed in {hash}` と返信                            |
| **decline**  | 検証の結果、指摘の前提が誤っている、または意図的な設計判断（CLAUDE.md・過去の決定・仕様上の理由） | 修正せず、Step 5 で根拠（検証で得た事実）を添えて返信                                   |
| **escalate** | 設計方針の変更、PR の範囲を超える修正、要件の衝突、検証しても正誤が確定しない、破壊的な変更を伴う | 修正も返信もせず、Step 8 の「要判断」に検証結果と選択肢を添えて列挙し、ユーザーに委ねる |

decline は事実に基づく反論があるときだけ。根拠を書けないなら fix か escalate。
claude-hooks の、普通に書く形（`cd <worktree> &&`、`env` や `timeout` の前置き、`bash -c '…'`、`nix develop -c` など、普通の手順で書くコマンド。
珍しい git の設定や全体オプションで行き先を変える、pipefail を途中で外す、のような作為的な書き方は含まない）でない抜け道の指摘は、重大度によらず decline する
（hook は境界ではなく、抜け道を 1 つ塞ぐと次の反例が出て周が終わらない。抜け道はモデルの判断と人のレビューに任せる、
と示すことが事実に基づく反論に当たる）。普通の形を誤って止める指摘は直す。

文書（変更ファイルがすべて `docs/` 配下。判定は `decide-next.sh` の `docs_only`）だけの PR は、
直すものを絞る（文章は細部をいくらでも掘れるので、直した文が次の指摘の的になり、周回が収束しない）:

- fix: 検証できる誤り（コードや事実と食い違う記述、壊れた参照・リンク）と、方針に関わる指摘
- decline: 競合状態・エッジケースなど実装の細部、言い回しや網羅性の指摘。
  `This will be settled with tests in the implementation PR.` か、不要な理由の事実を添えて返信する

レビューは最初の 1 周で止まる（Step 6 の `STOP_DOCS_REVIEWED`）。止まった後に直した文は GitHub のレビューを
受けないので、fix のコミットを push する前に、ローカルで `/code-review medium` を回す
（PR を出す前のローカルレビューとは別枠）。対象は `~/.claude/scripts/review-level.sh`
が出す `WORKTREE` を対象にする（PR の worktree で実行する。Step 6 の「ローカルレビュー」と同じ。積み重なった PR は
`--base <土台のブランチ>` を付ける）。
直すときも、コードの挙動を文書に重複して書かず、コードへの参照（ファイルと関数名）で書く。

重大度は Copilot の本文に揃えて High / Medium / Low で数える（ローカルの `/code-review` の指摘も同じ）。
対応順は High（誤動作・セキュリティ・データの破損・ゲートの迂回）→ Medium（限られた条件の
不具合・品質）→ Low（表記・好み）→ 質問（回答のみ）。Copilot の指摘は本文の重大度のまま扱う。

### 3.3 同一指摘の再提起（2 周目以降）

前の周の台帳（Step 8）と突き合わせ、同じ論点なら新規として扱わない:

- decline 済み → 前回の返信 URL を引いて同じ根拠で返信する。再検証は前提が変わったときだけ
- fix 済みの箇所 → 修正の反映漏れ（push 漏れ、別箇所の取り残し）を確認する

## Step 4: 修正

fix のコメントごとに、修正 → `git add` → チェック → コミットを繰り返す（**1 コメント = 1 コミット**）。
直した結果、ADR の決定・帰結・確認に書いた内容が変わるなら、同じコミットで ADR も合わせる（確定した ADR はマージの後に
直せない。/adr の「確定後の運用」の、マージ前の項）。

チェック対象はステージ済みのファイル。`FAILED:` なら `FIX:` か手直しで `ALL_OK` まで直し、
再度 `git add` する。プロジェクトの CLAUDE.md にチェックコマンドがあればそちらを使う。

```bash
~/.claude/skills/language-checks/scripts/run-checks.sh
git commit -m "$(cat <<'EOF'
fix: {指摘内容を簡潔に}

Refs: #discussion_r{comment_id}
EOF
)"
```

全件対応したら `git push`。force push はせず追加コミットで対応する（レビュー履歴を保持）。

## Step 5: 返信と resolve

```bash
~/.claude/skills/gh-pr-review/scripts/reply-to-comment.sh "{pr_url}" {comment_id} "{返信}"
~/.claude/skills/gh-pr-review/scripts/resolve-thread.sh "{pr_url}" {comment_id}
```

- 返信: fix は `Fixed in {hash}.`、decline は `I've decided to keep the current approach
  because {検証で得た事実}.`、再提起は `This is the same point as {前回の返信 URL}.`
- resolve するのは bot（`user_type: Bot`）のスレッドだけ。人間のスレッドは返信のみで、
  閉じるのはレビュアー（`--allow-human` はユーザーの明示的な指示があるときだけ）。
  未解決の人間スレッドで `pre-merge-check` がマージを止めるのは正しい状態なので、
  Step 8 で「レビュアーの確認待ち」として報告する

## Step 6: 再レビューの判定

再レビューは毎周新しい指摘を生みうるので、指摘ゼロを終了条件にすると収束しない。
判定はスクリプトに任せる（周回の上限は無い。head にローカルレビューの記録があれば終わる）:

```bash
~/.claude/skills/gh-pr-review/scripts/decide-next.sh "{pr_url}"
```

ユーザーが途中で追加の変更を求めたときは、周回を数え直す印を PR に付けてよい（`ROUND` の表示が 0 から数え直すだけで、
判定とマージには効かない）。マージの可否は、追加の変更を入れた新しい head のローカルレビューの記録で決まる:

```bash
~/.claude/skills/gh-pr-review/scripts/reset-rounds.sh "{pr_url}" "ユーザーの要望: {要望の要旨}"
```

| VERDICT                  | 意味                                                                                                | 次の一手                                                                                                                                                           |
| ------------------------ | --------------------------------------------------------------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------ |
| `ACT`                    | 未解決スレッドがある                                                                                | Step 3〜5 で対応する                                                                                                                                               |
| `LOCAL_REVIEW_NEEDED`    | 対応を push したが head をローカルで確かめていない                                                  | 下記の「ローカルレビュー」を回して記録し、`decide-next.sh` を再実行する                                                                                            |
| `STOP_LOCAL_REVIEWED`    | head はローカルでレビュー済み（記録に High が無い）。Copilot のレビューの有無・到着・内容は問わない | 要求せず、Copilot を待たずに Step 7 へ（マージ時の hook も同じ条件で通す）。Copilot の指摘が既にあれば、`PREVIOUSLY_MISSED` を含め台帳に処置があるかを先に確かめる |
| `STOP_DOCS_REVIEWED`     | 文書だけの PR で、対応を push 済み                                                                  | 要求せず Step 7 へ（文書は最初のレビューで足りる。一度レビューが成功していれば、マージ時の hook も再レビューを求めない）                                           |
| `STOP_DECLINED`          | 未解決ゼロ・head はレビュー済み                                                                     | 要求せず Step 7 へ（再要求しても同じレビューが返るだけ）                                                                                                           |
| `STOP_PREVIOUSLY_MISSED` | head はレビュー済みで、スレッドの無い Previously missed が残る                                      | 本文を読んで Step 3 で処置を決める。fix するなら Step 4 → push（次の判定はローカルレビューから）、しないなら台帳に理由を書いて Step 7 へ                           |
| `STOP_SUPPRESSED_ONLY`   | Suppressed comments のみ                                                                            | 本文を読んで要否を判断（下記）。対応するなら Step 3〜5 → push → 再レビュー、しないなら Step 7 へ                                                                   |
| `STOP_CLEAN`             | 指摘なし                                                                                            | Step 7 へ                                                                                                                                                          |
| `REVIEW_FAILED`          | Copilot がレビューできずに終わり、head のローカルレビューの記録も無い                               | 下記の「ローカルレビュー」を回して記録する（Copilot の失敗の診断は任意）                                                                                           |
| `WAITING`                | 要求後のレビューが未着で、head のローカルレビューの記録も無い                                       | 下記の「ローカルレビュー」を回して記録する（Copilot を待つなら `~/.claude/scripts/gh-wait-review.sh "{pr_url}"`。任意。background）                                |

**ローカルレビュー**: head のローカルレビューの記録は、Copilot のレビューの代わりになる。
対応を push したら、PR の worktree で `cd <worktree> && ~/.claude/scripts/review-level.sh <ブランチ名>` を実行し、出力の `LEVEL` と
`WORKTREE` で `/code-review <LEVEL> <WORKTREE>` を回す。ブランチ名で渡すと /code-review は cwd のリポジトリで
解くので、cwd が別のリポジトリだと対象を見つけられない。`WORKTREE` が none なら、PR の URL を対象に渡す。
直した差分と、同じ種類の問題が他に残っていないかを見る。回した head（`git rev-parse HEAD`。push 済みで PR の
head と同じもの）に、その回で検証して正しいと判断した指摘の数を、High / Medium / Low で記録する
（decline した指摘は数えない）:

```bash
~/.claude/skills/gh-pr-review/scripts/record-local-review.sh "{pr_url}" {sha} {high} {medium} {low}
```

記録は、回した head とその回の指摘の数の組で、直した後の head の分ではない。指摘を直したら、直した head を push
して回し直し、その head に記録する。High の残る head の記録はレビュー済みとして数えない（`decide-next.sh`、
マージ時の hook）ので、High は直して回し直す。Medium と Low は、直して回し直すか、
直さずにその head で終える（見送りは台帳に書く）。同じ head を回し直したら、最後の記録で決まる。
件数は、後に Copilot が出した数と比べて、効果を測るのに使う。

レビューが対象を見つけられなかった、または実行できなかったとき（「ブランチが存在しない」、エラー、`[]` だけの
返答）は、指摘ゼロではなく未実施である。`record-local-review.sh` に 0 件で記録せず、対象を直して回し直す。
`[]` が指摘ゼロだと言えるのは、返答が対象の差分を読んだと示しているとき（見たファイルや確かめた点が書かれている）だけ。
記録するのは、その head に回した `/code-review` の返答の指摘の数だけである。自分で差分を読んだだけ（`/code-review` を回していない）の
head は記録しない。記録はマージの hook が Copilot のレビューの代わりに数えるので、読んだだけの記録は、誰もレビューしていない head を
レビュー済みにする。Copilot がクォータ切れで返したとき（`REVIEW_FAILED`）も同じで、`/code-review` を回して記録する
（2026-10-28 に、#500 と #501 で親が差分を読んで 0 件を記録してマージした。#501 の head は担当が最後のレビューの後に積んだ未レビューの修正だった）。

**Copilot への再レビューの要求**は任意で、head の記録が無くても Copilot のレビューを受けたいときだけ、
次のスクリプトで行う（要求後そのまま待機する。約 10 分。
`run_in_background=true` で実行）。

```bash
~/.claude/skills/gh-pr-review/scripts/request-rereview.sh "{pr_url}"
```

`@copilot` メンションには copilot-swe-agent がコメントを返すだけで、レビューは走らない。
要求が失敗したらフォールバックせず、対応内容を PR コメントで伝えてレビュアーの判断を待つ。
待機がタイムアウトしても失敗とは限らない（Greptile 等は指摘ゼロだと何も投稿しない）。
head SHA の check-run と未解決スレッド数で判断する。

**Suppressed comments**: 実害（誤動作・データ破損・機能の穴）があれば対応する。理論上の
可能性・スタイル・些末な表記は対応せず、Step 8 に列挙してユーザーに委ねる。対応には
再レビュー 1 周のコストがかかるので、それに見合うかで決める。

レビューが届いたら `decide-next.sh` を再実行する。新しい周に入るときは、
**台帳（Step 8 の形式）を先に出力してから** Step 2 に戻る（コンテキストが要約されても
処置の履歴を失わないため）。

## Step 7: CI確認

```bash
~/.claude/scripts/gh-actions-diagnose.sh "{pr_url}"
```

`CAUSE:` への対応は gh-actions-check スキルの表に従う。`CODE` はレビュー対応と同じく
修正・コミット・push する。

## Step 8: 完了報告

周回ごとに台帳を出力し、最終周でまとめる。件数は論点の数（再提起は同じ行に周回を追記）。

```text
✅ レビュー対応が完了しました

PR: {url}
コミット: {hash}
レビュー周回: {ROUND}
終了理由: {指摘なし | 全件 decline | Suppressed comments のみ | 文書の初回レビュー対応済み | ローカルレビュー済み}

台帳:
| 周 | コメント | 場所 | 重大度 | 処置 | 結果（commit / 理由 / 前回参照） |
| -- | -------- | ---- | ------ | ---- | -------------------------------- |
| 1 | #discussion_r{id} | path:line | High | fix | {hash} |
| 1,2 | #discussion_r{id} | path:line | Medium | decline | 2 周目は同一指摘。前回返信を参照 |
| 2 | Previously missed | path:line | Medium | fix | {hash} |

対応サマリー: fix {n}（High {n} / Medium {n} / Low {n}）、decline {n}、回答 {n}、要判断 {n}
以下は該当があるときだけ:
- レビュアーの確認待ち: {人間のスレッドで返信済み・未 resolve}
- 未確認の対応: {周回上限到達時、最終周で対応・見送りした指摘}
- 対応しなかった Suppressed comments: {本文の要旨}
- レビューを受けていない変更: {commit hash と要旨}

要判断:
- {コメント URL}: {指摘の要旨} / 検証結果: {事実} / 選択肢: {A: …, B: …}
```

要判断があればユーザーの指示を待つ。なければ /gh-pr-merge へ進む。
