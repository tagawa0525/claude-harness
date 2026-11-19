---
name: gh-pr-review
description: PRレビューコメントを確認し対応。
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

PR のレビューコメントを確認し、対応する。件数・状態・比較は `~/.claude/skills/gh-pr-review/scripts/` の
スクリプトの出力を使い、モデルは判断だけを行う。

## 事前確認

!`gh auth status 2>&1 | head -3`
!`git status --short`
!`git branch -vv | head -5`

## Step 1: 対象の特定

スクリプトには PR の URL（`https://github.com/{owner}/{repo}/pull/{n}`。以下 `{pr_url}`）を渡す。

- コメント URL（`…/pull/{n}#discussion_r{id}`）→ そのまま渡し、出力の `comment_id` のコメントだけに対応する。
  `{pr_url}` は出力の `url`
- PR の URL → その PR の全コメント
- PR 番号・引数なし → PR のリポジトリの worktree で `cd <worktree> && gh pr view [{番号}] --json url -q .url`。
  出力の owner/repo が対象のリポジトリかを確かめてから使う

```bash
~/.claude/skills/gh-pr-review/scripts/get-pr-info.sh "{pr_url または コメント URL}"
```

## Step 2: レビューコメントの取得

```bash
~/.claude/skills/gh-pr-review/scripts/get-review-comments.sh "{pr_url}" [--unresolved]
~/.claude/skills/gh-pr-review/scripts/get-latest-review.sh "{pr_url}"
```

`get-latest-review.sh` の次の 2 つはレビュー本文にだけ現れ、スレッドが無い（返信と resolve はできない）:

- **Suppressed comments**: Copilot が低確度と判断した指摘。扱いは Step 6
- **Previously missed**（`--- previously missed ---` 以降）: 前の周で見落とした、変わっていないコードへの指摘。
  通常の指摘として Step 3 で処置を決め、台帳に載せる。`REMAINING_HIGH` は、開いている指摘とこの節の High の数

## Step 3: 前提の検証と処置の決定

### 3.1 前提を一次情報で検証する

着手前に、指摘が依拠する事実を一次情報で確かめる（bot の指摘は前提が誤ることがある）: コードの実際の挙動
（呼び出し元まで読む・実行する）、公式ドキュメントやライブラリのソース（バージョンは lock ファイル）、
CLAUDE.md や `git log -S` の経緯、文書に書いた数値ならその元の出力やデータ。直すのも見送るのも、確かめた結果で決める。

### 3.2 処置を決める

Copilot の指摘も、直すかは /git-merge の「レビュー」の節の「直す指摘の基準」で決める（Copilot の重大度では決めない）。
判断が付かないものは escalate にする:

| 処置         | 条件                                                                                                                    | 行動                                                                    |
| ------------ | ----------------------------------------------------------------------------------------------------------------------- | ----------------------------------------------------------------------- |
| **fix**      | 「直す指摘の基準」の直す（blocking）に当たり、修正が PR の範囲内に収まる                                                | Step 4 で修正・コミット → Step 5 で返信                                 |
| **decline**  | 指摘の前提が誤っている、意図的な設計判断（CLAUDE.md・過去の決定・仕様上の理由）、または直さない（non-blocking）に当たる | 修正せず、Step 5 で根拠を添えて返信                                     |
| **escalate** | 設計方針の変更、PR の範囲を超える修正、要件の衝突、検証しても正誤が確定しない、破壊的な変更を伴う                       | 修正も返信もせず、Step 8 の「要判断」に検証結果と選択肢を添えて列挙する |

- decline は、検証で得た事実か、直さない（non-blocking）に当たる理由を返信に書けるときだけ。書けないなら fix か escalate
- claude-hooks の抜け道の指摘は、重大度によらず decline する。対象は、普通の手順で書く形（`cd <worktree> &&`、`env` や
  `timeout` の前置き、`bash -c '…'`、`nix develop -c` など）でない、作為的な書き方（珍しい git の設定や全体オプションで
  行き先を変える、pipefail を途中で外す）の抜け道。返信の根拠は「hook は境界ではなく、抜け道はモデルの判断と人のレビューに
  任せる」。普通の形を誤って止める指摘は直す

文書だけの PR（変更ファイルがすべて `docs/` 配下。`decide-next.sh` の `docs_only`）は、上の基準の代わりに次で分ける:

- fix: 検証できる誤り（コードや事実と食い違う記述、壊れた参照・リンク）と、方針に関わる指摘。コードの挙動は文書に
  重複して書かず、コードへの参照（ファイルと関数名）で書く
- decline: 競合状態・エッジケースなど実装の細部、言い回しや網羅性の指摘。
  `This will be settled with tests in the implementation PR.` か、不要な理由の事実を添えて返信する
- レビューは最初の 1 周で止まる（`STOP_DOCS_REVIEWED`）。その後に直した文は GitHub のレビューを受けないので、fix の
  コミットを push する前に、Step 6 の「ローカルレビュー」の回し方で `/code-review medium` を回す

重大度は Copilot の本文のまま High / Medium / Low で数える。ローカルの `/code-review` の指摘は、「直す指摘の基準」の
blocking を High、non-blocking を Medium か Low（表記・好み）として数える。対応順は High（誤動作・セキュリティ・
データの破損・ゲートの迂回）→ Medium（限られた条件の不具合・品質）→ Low（表記・好み）→ 質問（回答のみ）。

### 3.3 同一指摘の再提起（2 周目以降）

前の周の台帳（Step 8）と突き合わせ、同じ論点は新規として扱わない:

- decline 済み → 前回の返信 URL を引いて同じ根拠で返信する。再検証は前提が変わったときだけ
- fix 済みの箇所 → 修正の反映漏れ（push 漏れ、別箇所の取り残し）を確認する

## Step 4: 修正

fix のコメントごとに、修正 → `git add` → チェック → コミットを繰り返す（1 コメント = 1 コミット）。
修正で ADR の決定・帰結・確認に書いた内容が変わるなら、同じコミットで ADR も合わせる（/adr の「確定後の運用」の、
マージ前の項）。

チェックの対象はステージ済みのファイル。`FAILED:` なら `FIX:` か手直しで `ALL_OK` まで直し、再度 `git add` する。
プロジェクトの CLAUDE.md にチェックコマンドがあればそちらを使う。

```bash
~/.claude/skills/language-checks/scripts/run-checks.sh
git commit -m "$(cat <<'EOF'
fix: {指摘内容を簡潔に}

Refs: #discussion_r{comment_id}
EOF
)"
```

全件対応したら `git push`。追加コミットで対応する（force push はレビュー履歴を失う）。

## Step 5: 返信と resolve

```bash
~/.claude/skills/gh-pr-review/scripts/reply-to-comment.sh "{pr_url}" {comment_id} "{返信}"
~/.claude/skills/gh-pr-review/scripts/resolve-thread.sh "{pr_url}" {comment_id}
```

- 返信: fix は `Fixed in {hash}.`、decline は `I've decided to keep the current approach
  because {検証で得た事実か、直さない（non-blocking）に当たる理由}.`、再提起は `This is the same point as {前回の返信 URL}.`
- resolve するのは bot（`user_type: Bot`）のスレッドだけ。人間のスレッドは返信のみ行い、Step 8 で「レビュアーの
  確認待ち」として報告する（`--allow-human` はユーザーの明示的な指示があるときだけ。未解決の人間スレッドで
  `pre-merge-check` がマージを止めるのは正しい状態）

## Step 6: 再レビューの判定

周の終わりは、指摘ゼロでなく次の `VERDICT` で決める（周回の上限は無い）:

```bash
~/.claude/skills/gh-pr-review/scripts/decide-next.sh "{pr_url}"
```

| VERDICT                  | 意味                                                                  |
| ------------------------ | --------------------------------------------------------------------- |
| `ACT`                    | 未解決スレッドがある                                                  |
| `LOCAL_REVIEW_NEEDED`    | 対応を push したが head をローカルで確かめていない                    |
| `STOP_LOCAL_REVIEWED`    | head はローカルでレビュー済み（記録に High が無い）                   |
| `STOP_DOCS_REVIEWED`     | 文書だけの PR で、対応を push 済み                                    |
| `STOP_DECLINED`          | 未解決ゼロ・head はレビュー済み                                       |
| `STOP_PREVIOUSLY_MISSED` | head はレビュー済みで、スレッドの無い Previously missed が残る        |
| `STOP_SUPPRESSED_ONLY`   | Suppressed comments のみ                                              |
| `STOP_CLEAN`             | 指摘なし                                                              |
| `REVIEW_FAILED`          | Copilot がレビューできずに終わり、head のローカルレビューの記録も無い |
| `WAITING`                | 要求後のレビューが未着で、head のローカルレビューの記録も無い         |

次の一手は、`VERDICT` の次の `NEXT:` の行（と `NOTE:` の行）に従う。

レビューが届いたら `decide-next.sh` を再実行する。新しい周に入るときは、台帳（Step 8 の形式）を先に出力してから
Step 2 に戻る（コンテキストが要約されても処置の履歴を残すため）。

**ローカルレビュー**: head のローカルレビューの記録は、Copilot のレビューの代わりになる。

- 記録は `/code-review` を回した head に付ける（PR を作る前の /gh-pr-create の周を含む）。`--unlogged` の理由には、
  その head を回した実際の回し方を書く（マージの hook が記録をレビュー済みに数える）
- 最初の head は必ず回す。対応を push した head に回すかは、/git-merge の「直す指摘の基準」の「終わり」の項で決め、
  回さないなら理由をマージの本文に書く
- 回し方: `cd <worktree> && ~/.claude/scripts/review-level.sh <ブランチ名>`（積み重なった PR は `--base <土台のブランチ>` を
  付ける）の `LEVEL` と `WORKTREE` で `/code-review <LEVEL> <WORKTREE>`。対象はブランチ名でなく `WORKTREE` で渡し
  （ブランチ名は cwd のリポジトリで解かれる）、`WORKTREE` が none なら PR の URL を渡す。見るのは PR の差分（対応の後なら、
  直した差分と、同じ種類の問題が他に残っていないか）
- 記録: 回した head（`git rev-parse HEAD`。push 済みで PR の head と同じ）に、その回で検証して正しいと判断した指摘の数を
  High / Medium / Low で付ける（decline した指摘は数えない）

```bash
cd <worktree> && ~/.claude/skills/gh-pr-review/scripts/record-local-review.sh "{pr_url}" {sha} {high} {medium} {low} --findings {file}
```

- 指摘が 1 件でもあれば、指摘ごとに 1 行（重大度、ファイルと行、要旨、直すか `再現なし` か）を scratchpad のファイルに書き、
  `--findings` で渡す（記録のコメントにコピーされる。件数だけでは、セッションが替わると対応しなかった指摘をマージの本文に
  書けない）。0 件なら `--findings` は付けない

- PR の URL を対象に渡した周（`WORKTREE` が none）は、`cd` を付けずに、末尾に `--unlogged '<その回し方>'` を付ける
- 記録は回した head とその回の指摘の数の組。指摘を直したら、直した head を push して回し直し、その head に記録する。
  同じ head を回し直したら、最後の記録で決まる
- High（blocking）の残る記録はレビュー済みに数えないので直し、回し直すかは「終わり」の項で決める。Medium と Low
  （non-blocking）は直さず、「直す指摘の基準」のとおり扱ってその head で終える
- 対象が見つからない、エラー、`[]` だけの返答の回は未実施で、0 件で記録せずに対象を直して回し直す
- 記録の出力の `NOTE:` と、拒否の文の案内に従う

**Copilot への再レビューの要求**は任意で、head の記録が無くても Copilot のレビューを受けたいときだけ行う
（約 10 分待つので `run_in_background=true`）:

```bash
~/.claude/skills/gh-pr-review/scripts/request-rereview.sh "{pr_url}"
```

要求が失敗したら、対応内容を PR コメントで伝えてレビュアーの判断を待つ（`@copilot` メンションではレビューは走らない）。
待機のタイムアウトは、出力の `COPILOT_CHECK:` と `UNRESOLVED:` で判断する。

**Suppressed comments**: 3.2 の分け方（文書だけの PR はその分け方）で fix に当たるものだけ対応する。escalate に
当たるものは Step 8 の「要判断」に入れ、ほかは対応せず Step 8 に列挙する。

**追加の変更**: ユーザーが途中で追加の変更を求めたときは、周回を数え直す印を付けてよい（`ROUND` の表示だけが 0 に戻る。
マージの可否は、新しい head のローカルレビューの記録で決まる）:

```bash
~/.claude/skills/gh-pr-review/scripts/reset-rounds.sh "{pr_url}" "ユーザーの要望: {要望の要旨}"
```

## Step 7: CI確認

```bash
~/.claude/scripts/gh-actions-diagnose.sh "{pr_url}"
```

`NEXT:` に従う。`CODE` は Step 4 と同じく修正・コミット・push する。

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
- 未確認の対応: {最終周で対応・見送りした指摘}
- 対応しなかった Suppressed comments: {本文の要旨}
- レビューを受けていない変更: {commit hash と要旨}

要判断:
- {コメント URL}: {指摘の要旨} / 検証結果: {事実} / 選択肢: {A: …, B: …}
```

要判断があればユーザーの指示を待つ。なければ /gh-pr-merge へ進む。
