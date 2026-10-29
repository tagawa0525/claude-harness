---
status: accepted
date: 2026-10-29
requires: [ADR-0009, ADR-0051]
supersedes: []
superseded-by:
amends: [ADR-0009, ADR-0051]
issues: ["#523", "#524"]
---

# ADR-0067: 決定の一部の変更は同じ ADR の「## 変更」に書き、帰結と確認は確定後も今の事実に直す

## 背景

ADR を確定した後の扱いは、次の 3 つの決定が定めている。

- ADR-0009 の「運用」（ADR-0006、ADR-0008 から引き継いだもの）: 確定した後に直してよいのは、誤字、文法、マークアップ、
  壊れたリンク、フロントマターの status、補足への日付つきの追記だけ
- ADR-0009 の「ADR は増えていく前提にする」: 決定を作り直すたびに新しい ADR で置き換え、前の判断の全文をファイルとして残す。
  今の決定は `status: accepted` で引く
- ADR-0051: 決定の一部だけを変えるときは新しい ADR を書き、`amends` と `amended-by` で両方向に結ぶ

最初に形を決めた ADR-0006 は、「補足」の「調べたが取り入れなかったもの」で 2 つの案を退けていた。1 つは「話題ごとに
1 件の ADR をその場で直す（生きた ADR）」で、前の決定の理由の全文が git の履歴にしか残らないことが理由だった。もう 1 つは
「一部だけ改める（IETF の Updates）」で、置き換えの運用と相容れないことが理由だった。ところが、後の ADR-0051 は
一部だけを改める仕組みを `amends` として足した。nixfiles では、2026-10-29 の main（631f60f3）で 18 件の ADR が
`amends` か `amended-by` を持つ（`grep -l -E '^(amends|amended-by):' docs/adr/*.md`）。たとえば ADR-0027 の今の決定は、
`amended-by` に挙げた ADR-0044、ADR-0054、ADR-0060 の 3 件を合わせて読まないと分からない。

確定した後は直せない規則のもとで、次のことが起きた。

- 確定した ADR はマージの後に直せないので、PR のレビューでは ADR の事実の誤りをマージの前に直し切る必要がある。
  2026-10-29 の PR #517（ADR-0066）のコミットには、「record that ADR-0066 amends ADR-0023's round rule」
  「give the review cap an exit and fix its facts」「keep security in the High definition and fix ADR-0066's facts」
  のように、ADR の事実を直したものが並ぶ（`gh pr view 517 --json commits`）。レビューの周の数のうち、どれだけがこの規則の
  せいかは確かめていない
- xlc では、実装をマージした後も proposed のまま残った ADR が 6 件あった（xlc の docs/issues/0188）。#0188 は、ADR-0202
  について「accepted にすると、以後の広げ方は新しい ADR（`amends`）になる」と書いている。6 件が残った原因が、直せなくなる
  のを避けたことかは確かめていない
- ユーザーは次のように問うた。「ADRが書き換えられない理由は何でですか？意思決定の経緯が参照できることが目的なら１つの
  文書に留まっていた方がわかりやすくないですか？」（ユーザー 2026-10-29）。親のセッションが下の案 A を提案し、ユーザーは
  「君が運用しやすい方針を取ってくれればいいよ」（ユーザー 2026-10-29）と答えた。どの案を採るかは親のセッションが決めた

ほかの仕組みでは、次のように扱っている（2026-10-29 に各文書を読んだ）。

- PEP 1 の「PEP Maintenance」: 「PEPs are no longer substantially modified after they have reached the Accepted, Final,
  Rejected or Superseded state.」「Once resolution is reached, a PEP is considered a historical document rather than a
  living specification.」（peps.python.org/pep-0001）
- Rust RFC の README の「The RFC life-cycle」: 「Modifications to "active" RFCs can be done in follow-up pull requests.」
  「In general, once accepted, RFCs should not be substantially changed. Only very minor changes should be submitted as
  amendments. More substantial changes should be new RFCs, with a note added to the original RFC.」（rust-lang/rfcs
  1dc48d76）
- MADR 4.0.0（adr.github.io/madr）: 確定した後に直せるかは書いていない。テンプレートのフロントマターの `date` は
  「YYYY-MM-DD when the decision was last updated」で、status の例に「superseded by ADR-0123」がある
- Kubernetes の KEP（kubernetes/enhancements a904b502）: KEP-0000 の「Motivation」は「A KEP is broken into sections which
  can be merged into source control incrementally in order to support an iterative development process.」と書き、
  メタデータの `last-updated` は「The date that the KEP was last changed significantly.」。テンプレートの
  「Implementation History」の節には、節のマージ、実装の開始、GA、置き換えなどの節目を書き足していく。
  KEP が生きた文書だと明言した文は見つけていない

## 決定の要因

- 決めた時点の理由（背景、比べた案、採った理由）は、後から読み返せる形で残す。これが ADR の価値である
- 今の決定は、1 件の ADR を読めば分かる。いくつもの ADR のチェーンをたどらせない
- マージの後に分かった事実（実装の進み、測った値、issue）は、それが関わる ADR に書ける
- 規則は検査とスキルで示せる形にし、守るための手間を増やさない

## 検討した案

- 案 A: 「背景」「検討した案」「決定と理由」は決めた時点の記録として凍結し、「帰結」と「確認」は確定後も今の事実に直す。
  決定の一部を変えるときは、同じ ADR の「決定と理由」を直し、末尾の「## 変更」に、日付、変えた部分の前の文、理由を書く
- 案 B: 今の規則のまま（確定した後は直さず、一部の変更は `amends` の新しい ADR、全体の変更は `supersedes` の新しい ADR）
- 案 C: ADR を生きた文書にし、どの節もその場で直す。前の版は git の履歴に任せる
- 案 D: 確定した ADR の本文は直さず、決定の変更は「## 補足」に日付つきで書き足す

## 決定と理由

採用: 「案 A」。

- 凍結する部分: 「背景」「検討した案」「決定と理由」（決定と理由の下に置いた「帰結」と「確認」を除く）、「決定の要因」と
  「各案の長所と短所」（MADR の Decision Drivers と Pros and Cons of the Options。何を重んじ、各案をどう比べたかという決めた
  時点の文脈）、表題、フロントマターのうち確定した後に直してよいキーでないもの。直してよいのは、誤字、文法、マークアップ、壊れたリンク、
  日本語の辞書に合わせた意味を変えない語の置き換えだけ。決めた時点に何を知っていて、何と比べ、なぜそう決めたかの記録で、
  後から書き換えると経緯が失われる
- 確定した後も直す節: 「帰結」と「確認」。今の事実を書く節なので、マージの後の実装の進み、測った値、issue への結びを
  その場で書き直す。帰結に書いた残りの issue は、フロントマターの `issues` にも足してよい（`issues` は直してよいキーに
  加える。検査は `issues` の変更を見ないので、決めたときの issue を消す誤りはレビューで見る）。「## 補足」も今までどおり書き足してよい
- 決定の一部を変えるとき: 新しい ADR を作らず、同じ ADR の「決定と理由」を今の決定に直し、末尾の「## 変更」に 1 項を足す。
  項の形は `- YYYY-MM-DD: <変えた部分>。前は「<前の文の引用>」。<理由>`。前の文を引用するので、前の決定はファイルの中に残り、
  git の履歴を探さずに読める。比べた案があるなら、その項に書くか issue に書いて項から指す
- 決定の全体を変えるとき: 今までどおり新しい ADR で置き換える（`supersedes` と `superseded-by`）。古い ADR の末尾にも
  「## 変更」で、置き換えた日付と新しい ADR を書いてよい
- `amends` と `amended-by` は、これからは新しく使わない。既に書いた結び（ADR-0009 と ADR-0049、ADR-0051 など）は残し、
  `design-records check` はその一致を今までどおり確かめる。この ADR が ADR-0009 と ADR-0051 に結ぶ `amends` が最後の使用で、
  同時に、2 件の末尾に「## 変更」を足して新しい規則の最初の適用にする
- 既存の ADR は書き直さない。新しい規則は、この ADR の後の変更から当てる
- `design-records check` が FILE を渡したときに出す「確定した ADR の本文の変更」の注意（warning）は、凍結する部分の変更
  だけに絞る。決定の一部を変えるときも決定と理由が変わるので、注意は出る。注意は止めないので、「## 変更」を書いたかを
  確かめる合図にする。「## 変更」の項の形（日付と引用と理由）の検査は、`design-records` の変更が要るので #523 に置く。
  検査の有無で運用は変わらない

採らなかった案:

- 案 B: 背景の 3 つの問題（マージの前に ADR を直し切る圧力、proposed のまま残すこと、チェーンをたどらないと分からない今の決定）が
  続く。退けたはずの一部の改めを `amends` として足した時点で、置き換えだけで今の決定が分かるという前提は崩れている
- 案 C: 今の決定は 1 件で分かるが、前の決定の理由が git の履歴にしか残らない（ADR-0006 が生きた ADR を退けた理由と同じ）。
  ADR を読む LLM は、ファイルの今の版を読み、git の履歴までは読まない
- 案 D: 今の決定が「決定と理由」と補足の項に分かれ、読む人が合成しなければならない。案 B のチェーンが 1 件の中に移るだけ

これは ADR-0009 の「運用」の「確定した後に直すのは、誤字、文法、マークアップ、壊れたリンク、フロントマターの status、
補足への日付つきの追記だけ」の部分と、「ADR は増えていく前提にする」の「そのたびに新しい ADR で置き換え」の部分を、
決定の全体を変えるときに限る形に改める。ADR-0051 の「一部を置き換えた ADR と置き換えられた ADR を、フロントマターの
amends と amended-by で両方向に結ぶ」決定は、これからの一部の変更には当てず、既にある結びの検査だけを残す形に改める。
ほかの部分は変わらない。

### 帰結

- 良い点: 今の決定は、その ADR の「決定と理由」を読めば分かる。前の決定と変えた理由は、同じファイルの「## 変更」にある
- 良い点: マージの後に分かった実装の進みや測った値を、「帰結」と「確認」にその場で書ける。マージの前に直し切る必要が
  あるのは、凍結する部分（3 節、決定の要因、各案の長所と短所、表題、直せないフロントマターのキー）だけになる
- 悪い点: 決定と理由を直しても、「## 変更」を書き忘れたことは検査で捕まらない（注意は出るが、項の有無と形は見ない。#523）。
  それまではレビューで見る
- 悪い点: 既存の ADR には、`amends` のチェーンと「## 変更」の 2 つの形が混ざる
- 実装が決定に追いついていない点: git-merge、delegate、gh-pr-review のスキルと `git-merge-state.sh` のコメントに残る
  「確定した ADR はマージの後に直せない」は、PR #517 が同じファイルを直しているので、この PR では変えない（#524）。
  xlc の docs/adr/README.md の「運用」は、xlc の側で直す

### 確認

- `modules/home/parts/design-records/src/check.rs` のテスト: 背景、検討した案、決定と理由の変更は本文の変更になる
  （`decided_sections_change_the_body`）。決定と理由の下の 3 段目と 2 段目の、帰結と確認の変更はならない
  （`consequences_and_confirmation_keep_the_body`）。決定と理由の下の、帰結と確認でない 3 段目の節は本文
  （`other_subsections_of_the_decision_change_the_body`）。「## 変更」とほかの 2 段目の節の変更はならない
  （`changes_section_and_other_sections_keep_the_body`）。決定の要因と各案の長所と短所の変更は本文の変更になる
  （`drivers_and_pros_and_cons_change_the_body`）。背景や検討した案の下の帰結と確認の見出しは本文
  （`consequences_under_other_decided_sections_change_the_body`）。表題の後の 1 段目の見出しは凍結する部分に戻さない
  （`later_top_level_headings_do_not_reopen_the_decided_sections`）。`issues` の追加は本文の変更にならない
  （`adding_issues_keeps_the_body`）
- `modules/home/parts/design-records/tests/cli.rs`: 確定した ADR の背景を変えると注意が出る
  （`check_warns_when_the_body_of_a_decided_adr_on_main_changes`、`check_follows_renames_when_comparing_the_body`）
- スキル `adr` の「確定後の運用」が、凍結する部分、直す節、「## 変更」の形を示す
- ADR-0009 と ADR-0051 の末尾の「## 変更」が、この ADR の形で書いてある
