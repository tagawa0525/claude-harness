---
type: ADR
status: accepted
date: 2026-11-19
requires: []
supersedes: []
superseded-by:
issues: ["#333"]
---

# ADR-0099: 閉じた issue の resolution に、決まった語のほか、根拠（ADR、コミット）と理由を書く形も通す

## 背景

`design-records check` は、フロントマターを持つ docs/issues の issue の形を注意（warning）として示す。閉じた issue の
`resolution` には決まった 7 語（`done`、`wontfix`、`by-design`、`external`、`cannot-reproduce`、`duplicate`、`obsolete`）を
求め、根拠は別のキー（`resolved-by` に ADR かコミット、`duplicate-of` に issue）に書く（/docs-issues の「形」）。この形の
検査はコミット a1824f75（2026-11-15）で足し、形の全体を決めた ADR は無い（#6 の「やること」に残る。ADR-0018 の「検査」の
項は、issue の形を検査しないとした時点の文のまま）。この ADR は `resolution` の形だけを決める。

xlc のリポジトリ（/home/tagawa/github/xlc）は、docs/issues の README で `resolution` を `ADR-NNNN`（決めた ADR）、
`コミット <hash>`（解決したコミット。7〜40 桁の英小文字の16進）、`wontfix: 理由` のどれかとし、自分の検査（crates/docs）で
確かめる。閉じた issue の 99 件（`ADR-NNNN` が 52 件、`コミット <hash>` が 31 件、`wontfix: 理由` が 16 件。HEAD 0500a720e、
2026-11-19）すべてに `design-records check` が注意を出していた（#333）。毎回出る注意は読まれなくなる。

## 決定の要因

- 今の 7 語で書いた issue の検査の結果を変えない
- xlc の 3 種の形に注意を出さない
- どちらの形でもない値（空、ほかの語）には注意を出し続ける
- 根拠（決めた ADR、解決したコミット）を機械で辿れる

## 検討した案

- 案 A: 決まった語の形だけを通し、xlc の issue を `resolution: done` と `resolved-by: <ADR かコミット>` に書き直す。持つ
  情報は同じで、99 件を書き直す作業は何も足さない。xlc の検査（crates/docs）も合わせて直すことになる
- 案 B: 決まった語の形に加え、根拠を書く形（`ADR-NNNN`、`コミット <hash>`）と、語に理由を添えた形（`<語>: <理由>`）も通す
- 案 C: リポジトリごとに通す形を設定で選ぶ。2 つの形は重ならず（7 語は `ADR-` でも「コミット」と空白でも始まらない）、両方を
  通しても片方の誤りを見逃さないので、設定を持つ費用に見合わない

## 決定と理由

採用: 案 B。根拠を書く形も許し、実例を見て親のセッションも判断することを、ユーザーが求めた（ユーザー 2026-11-19 の発言の
要約）。xlc の閉じた issue はこの形で揃っていて、根拠を機械で辿れる。

閉じた issue の `resolution` に通す形:

- 決まった 7 語のどれか
- `<7 語のどれか>: <理由>`（理由は空でない）。理由は読まない。/docs-issues は引用符で囲んで書かせる（`"wontfix: 理由"`。
  YAML の引用符の無い値はコロンと空白を含めないので、ほかの YAML の読み手が読めない）。囲まない値も通す（xlc の 10 件がこの形で、
  `#` のコメントと違って design-records と値の読み違いは起きない）
- `ADR-NNNN`（決めた ADR）
- `コミット <hash>`（解決したコミット。7〜40 桁の英小文字の16進）

関係の検査は、新しい形にも今の形と同じ見方で効かせる:

- `ADR-NNNN` は、`resolved-by` の ADR と同じく docs/adr にあるかを見て、無ければ注意する。ADR の `issues` がこの issue を
  挙げているかは見ない（`resolved-by` でも見ていない。xlc は自分の検査で見る）
- `コミット <hash>` の hash は、`resolved-by` のコミットと同じく探さない。hash は rebase で変わり、探すと正しい記録を誤って
  示す
- `duplicate: <理由>` は `duplicate` と同じく `duplicate-of` を求める。/handoff（resume-state。決めた時点の名前は /resume）も同じく重複先をたどる。
  `resolution` の形の解釈は design-records の `Resolution` の 1 か所に置き、両方から使う
- 開いている issue の `resolution` は、形によらず閉じるときのキーとして注意する（空の `resolution:` は書いていないものとする）

### 帰結

- 良い点: xlc で `resolution` の注意が 99 件から 0 件になり、ほかの注意（287 件）は変わらない（`design-records -C
  /home/tagawa/github/xlc check` を、配備されている版と、このブランチでビルドした版で比べた。2026-11-19）
- 良い点: 今の 7 語で書いた issue の検査の結果は変わらない。形に合わない値への注意の文は、通る形を並べる文に変わった
- 悪い点: 同じ情報を書く形が 2 つあり、リポジトリの中で混ざっても止めない
- 悪い点: `resolution: ADR-NNNN` と `resolved-by` の両方を書いて食い違っても見ない

### 確認

`crates/design-records/tests/cli.rs` の `check_accepts_resolutions_that_are_a_word_or_name_the_evidence`、
`check_warns_about_resolutions_in_neither_form`、`check_follows_the_links_of_resolutions_that_name_the_evidence`、
`crates/claude-hooks/tests/script_resume_state.rs` の `a_blocker_closed_as_a_duplicate_with_a_reason_is_read_as_a_duplicate` と、
/docs-issues の「形」（`claude/skills/docs-issues/SKILL.md`）。
