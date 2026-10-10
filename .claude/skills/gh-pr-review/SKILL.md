---
name: gh-pr-review
description: PRレビューコメントを確認し対応。コード修正、返信を実行。
# model 未指定: コード修正を伴うためセッションモデルを継承する
argument-hint: [PR番号 | コメントURL] [--unresolved]
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

- コメント URL（`…/pull/{n}#discussion_r{id}`）→ URL を渡す。出力の `number` と `comment_id` の
  コメントだけに対応する
- PR 番号 → その PR の全コメント
- 引数なし → 引数なしで実行し、現在のブランチの PR を得る

```bash
~/.claude/skills/gh-pr-review/scripts/get-pr-info.sh [{pr_number} | "{URL}"]
```

## Step 2: レビューコメントの取得

```bash
~/.claude/skills/gh-pr-review/scripts/get-review-comments.sh {pr_number} [--unresolved]
~/.claude/skills/gh-pr-review/scripts/get-latest-review.sh {pr_number}
```

`get-latest-review.sh` の **Suppressed comments** は Copilot が低確度と判断した指摘で、
レビュー本文にだけ現れる（スレッドがないので返信できない）。扱いは Step 6 の基準に従う。

## Step 3: 前提の検証と処置の決定

### 3.1 前提を一次情報で検証する

bot の指摘は前提が誤っていることがある。**着手前に、指摘が依拠する事実を一次情報で
確かめる**: コードの実際の挙動（呼び出し元まで読む・実行する）、公式ドキュメントや
ライブラリのソース（バージョンは lock ファイルで確認）、CLAUDE.md や `git log -S` の経緯。
確かめずに直す、指摘文を信じて見送る、のどちらもしない。

### 3.2 処置を決める

判断が付かないものは escalate に倒す:

| 処置         | 条件                                                                                              | 行動                                                                                    |
| ------------ | ------------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------- |
| **fix**      | 検証の結果、指摘が正しく、修正が PR の範囲内に収まる                                              | Step 4 で修正・コミット → Step 5 で `Fixed in {hash}` と返信                            |
| **decline**  | 検証の結果、指摘の前提が誤っている、または意図的な設計判断（CLAUDE.md・過去の決定・仕様上の理由） | 修正せず、Step 5 で根拠（検証で得た事実）を添えて返信                                   |
| **escalate** | 設計方針の変更、PR の範囲を超える修正、要件の衝突、検証しても正誤が確定しない、破壊的な変更を伴う | 修正も返信もせず、Step 8 の「要判断」に検証結果と選択肢を添えて列挙し、ユーザーに委ねる |

decline は事実に基づく反論があるときだけ。根拠を書けないなら fix か escalate。

文書（変更ファイルがすべて `docs/` 配下。判定は `decide-next.sh` の `docs_only`、決定は ADR-0004）だけの PR は、
直すものを絞る:

- fix: 検証できる誤り（コードや事実と食い違う記述、壊れた参照・リンク）と、方針に関わる指摘
- decline: 競合状態・エッジケースなど実装の細部、言い回しや網羅性の指摘。
  `This will be settled with tests in the implementation PR.` か、不要な理由の事実を添えて返信する

レビューは最初の 1 周で止まる（Step 6 の `STOP_DOCS_REVIEWED`）。止まった後に直した文は GitHub のレビューを
受けないので、fix のコミットを push する前に、ローカルで `/code-review medium` を一度だけ回す
（PR を出す前のローカルレビューとは別枠。2 周目はしない）。対象は `~/.claude/scripts/review-level.sh`
が出す `TARGET`（積み重なった PR は `--base <土台のブランチ>` を付ける）。
直すときも、コードの挙動を文書に写さず、コードへの参照（ファイルと関数名）で書く。

対応順は 🔴 Critical（バグ・セキュリティ・ビルド失敗）→ 🟡 Warning（品質・性能）→
🟢 Suggestion（スタイル・nit）→ ℹ️ Question（回答のみ）。

### 3.3 同一指摘の再提起（2 周目以降）

前の周の台帳（Step 8）と突き合わせ、同じ論点なら新規として扱わない:

- decline 済み → 前回の返信 URL を引いて同じ根拠で返信する。再検証は前提が変わったときだけ
- fix 済みの箇所 → 修正の反映漏れ（push 漏れ、別箇所の取り残し）を確認する

## Step 4: 修正

fix のコメントごとに、修正 → `git add` → チェック → コミットを繰り返す（**1 コメント = 1 コミット**）。

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
~/.claude/skills/gh-pr-review/scripts/reply-to-comment.sh {pr_number} {comment_id} "{返信}"
~/.claude/skills/gh-pr-review/scripts/resolve-thread.sh {pr_number} {comment_id}
```

- 返信: fix は `Fixed in {hash}.`、decline は `I've decided to keep the current approach
  because {検証で得た事実}.`、再提起は `This is the same point as {前回の返信 URL}.`
- resolve するのは bot（`user_type: Bot`）のスレッドだけ。人間のスレッドは返信のみで、
  閉じるのはレビュアー（`--allow-human` はユーザーの明示的な指示があるときだけ）。
  未解決の人間スレッドで `pre-merge-check` がマージを止めるのは正しい状態なので、
  Step 8 で「レビュアーの確認待ち」として報告する

## Step 6: 再レビューの判定

周回数の上限は 5 周（再レビューは毎周新しい指摘を生みうるので、指摘ゼロを
終了条件にすると収束しない）。判定はスクリプトに任せる:

```bash
~/.claude/skills/gh-pr-review/scripts/decide-next.sh {pr_number}
```

ユーザーが途中で追加の変更を求め、それを入れたら、周回を 0 から数え直す（指摘への対応とは
別の仕事なので）。push の後、再レビューを要求する前に、理由を書いた印を PR に付ける。
自分の判断で足した変更や、指摘への対応では数え直さない:

```bash
~/.claude/skills/gh-pr-review/scripts/reset-rounds.sh {pr_number} "ユーザーの要望: {要望の要旨}"
```

| VERDICT                | 意味                                         | 次の一手                                                                                                                 |
| ---------------------- | -------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------ |
| `ACT`                  | 未解決スレッドがある                         | Step 3〜5 で対応する                                                                                                     |
| `REREVIEW_NEEDED`      | 対応を push したが再レビューを要求していない | `request-rereview.sh` で要求する（挙動を変えない修正だけなら下記の省略条件）                                             |
| `STOP_DOCS_REVIEWED`   | 文書だけの PR で、対応を push 済み           | 要求せず Step 7 へ（文書は最初のレビューで足りる。一度レビューが成功していれば、マージ時の hook も再レビューを求めない） |
| `STOP_LIMIT`           | 要求が必要だが ROUND = 5                     | 要求せず Step 7 へ。最終周の対応・見送りを報告に列挙する                                                                 |
| `STOP_DECLINED`        | 未解決ゼロ・head はレビュー済み              | 要求せず Step 7 へ（再要求しても同じレビューが返るだけ）                                                                 |
| `STOP_SUPPRESSED_ONLY` | Suppressed comments のみ                     | 本文を読んで要否を判断（下記）。対応するなら Step 3〜5 → push → 再レビュー、しないなら Step 7 へ                         |
| `STOP_CLEAN`           | 指摘なし                                     | Step 7 へ                                                                                                                |
| `REVIEW_FAILED`        | Copilot がレビューできずに終わった           | マージに進まない。Step 7 で診断し、直せたら再レビュー、直せなければ要判断へ                                              |
| `WAITING`              | 要求後のレビューが未着                       | `~/.claude/scripts/gh-wait-review.sh {pr_number}` で待つ（下記と同様 background）                                        |

**再レビューの要求**は次のスクリプトだけで行う（要求後そのまま待機する。約 10 分。
`run_in_background=true` で実行）。

```bash
~/.claude/skills/gh-pr-review/scripts/request-rereview.sh {pr_number}
```

`@copilot` メンションには copilot-swe-agent がコメントを返すだけで、レビューは走らない。
要求が失敗したらフォールバックせず、対応内容を PR コメントで伝えてレビュアーの判断を待つ。
待機がタイムアウトしても失敗とは限らない（Greptile 等は指摘ゼロだと何も投稿しない）。
head SHA の check-run と未解決スレッド数で判断する。

**Suppressed comments**: 実害（誤動作・データ破損・機能の穴）があれば対応する。理論上の
可能性・スタイル・些末な表記は対応せず、Step 8 に列挙してユーザーに委ねる。対応には
再レビュー 1 周のコストがかかるので、それに見合うかで決める。

**再レビューの省略**: 表記・コメント・整形など挙動を変えない修正だけなら要求せず、
/gh-pr-merge で `ALLOW_UNREVIEWED_HEAD=1` を付けてマージしてよい。ロジック・条件式・API 呼び出し・テストの期待値に触れたら対象外。
迷ったら再レビューを要求する。省いたら Step 8 に commit hash と要旨を載せる。

レビューが届いたら `decide-next.sh` を再実行する。新しい周に入るときは、
**台帳（Step 8 の形式）を先に出力してから** Step 2 に戻る（コンテキストが要約されても
処置の履歴を失わないため）。

## Step 7: CI確認

```bash
~/.claude/scripts/gh-actions-diagnose.sh {pr_number}
```

`CAUSE:` への対応は gh-actions-check スキルの表に従う。`CODE` はレビュー対応と同じく
修正・コミット・push する。

## Step 8: 完了報告

周回ごとに台帳を出力し、最終周でまとめる。件数は論点の数（再提起は同じ行に周回を追記）。

```text
✅ レビュー対応が完了しました

PR: {url}
コミット: {hash}
レビュー周回: {ROUND}/5
終了理由: {指摘なし | 全件 decline | Suppressed comments のみ | 文書の初回レビュー対応済み | 周回上限到達}

台帳:
| 周 | コメント | 場所 | 優先度 | 処置 | 結果（commit / 理由 / 前回参照） |
| -- | -------- | ---- | ------ | ---- | -------------------------------- |
| 1 | #discussion_r{id} | path:line | 🔴 | fix | {hash} |
| 1,2 | #discussion_r{id} | path:line | 🟡 | decline | 2 周目は同一指摘。前回返信を参照 |

対応サマリー: fix {n}（🔴{n} / 🟡{n} / 🟢{n}）、decline {n}、回答 {n}、要判断 {n}
以下は該当があるときだけ:
- レビュアーの確認待ち: {人間のスレッドで返信済み・未 resolve}
- 未確認の対応: {周回上限到達時、最終周で対応・見送りした指摘}
- 対応しなかった Suppressed comments: {本文の要旨}
- レビューを受けていない変更: {commit hash と要旨}

要判断:
- {コメント URL}: {指摘の要旨} / 検証結果: {事実} / 選択肢: {A: …, B: …}
```

要判断があればユーザーの指示を待つ。なければ /gh-pr-merge へ進む。
