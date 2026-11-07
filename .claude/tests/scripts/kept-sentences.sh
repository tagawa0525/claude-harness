#!/usr/bin/env bash
# .claude/scripts/kept-sentences.py（規則の文書を縮めたとき、残した文が元の版の部分文字列かを確かめる）のテスト
#
# 実行: bash .claude/tests/scripts/kept-sentences.sh（まとめて実行: bash .claude/tests/scripts.sh）

source "$(dirname "$0")/../lib.sh"

KEPT="$SCRIPTS_DIR/kept-sentences.py"
D="$TEST_ROOT/docs"
mkdir -p "$D"

# /git-merge と /delegate を縮めたブランチ（2026-11-08 のマージ）の前の版の、変わった 5 件の段落
cat >"$D/old.md" <<'MD'

rebase の前に、古い土台（`git merge-base main [branch]`）と旧先端（`git rev-parse [branch]`）を控える（「レビュー」の rebase 後の判断で
`git range-diff <古い土台>..<旧先端> main..[branch]` と `git log <古い土台>..main` に使う。rebase の後では取れない）。

- claude-hooks の、普通に書く形（`cd <worktree> &&`、`env` や `timeout` の前置き、`bash -c '…'`、`nix develop -c` など、普通の手順で書くコマンド。
  珍しい git の設定や全体オプションで行き先を変える、pipefail を途中で外す、のような作為的な書き方は含まない）でない抜け道の指摘は直さず、無いものとして打ち切りを決める
  （hook は境界ではなく、抜け道を 1 つ塞ぐと次の反例が出て周が終わらない。抜け道はモデルの判断と人のレビューに任せる）。
  普通の形を誤って止める指摘は直す

  - 直す（blocking）: (a) 通常の入力と流れで到達する誤動作、誤った数値、データの破損、セキュリティの欠陥、ゲートの迂回で、
    落ちるテストか再現する手順を先に書けるもの。スキルと hook の文書の、手順のとおりに実行すると失敗する誤り（壊れたパス、
    無い `VERDICT` の名前）も、その実行を再現の手順として数える。(b) ADR の凍結する部分（背景、決定の要因、
    検討した案、決定と理由、各案の長所と短所、表題、フロントマターの `requires` と `supersedes`）の事実の誤りで、一次情報で
    確かめたもの（マージの後に直すには「## 変更」が要り、誤った決定が前の文として記録に残るので、マージの前に直す）。

- 依頼の「前提」の欄の行を順に、作業を始める前に出典で確かめる。確かめ方は「頼む側（親のセッション）の書き方」の
  「前提」の項の出典の規則と同じ。出典が添えてなければ、その規則の種類の出典を探して確かめ、その行と見つけた出典を報告の
  「確認の結果」に書く（親が書き漏れに気づくため）。欄の外（ほかの欄、案、欄の無い依頼）に事実として書かれた
  値と前提のうち、その規則の種類で判断の前提にするものも、同じく作業の前に確かめ、確かめたものを報告の「確認の結果」に
  書く（欄は書く側の漏れを減らすもので、欄の外に残った誤りも止める）。「確かめていない」と添えた値は、欄の行でも欄の外でも、
  作業の前に止まる対象でなく、確かめることが作業の一部で、結果を報告に書く。作業中に誤りと分かり、それに頼った作業が
  成り立たなくなれば、案の反例と同じくその時点で止まる。それ以外は、食い違うか、出典が見つからなければ、上と同じく報告に書いて止まる。最初の依頼に「前提」の欄が無ければ、
  報告の「親が先に読む要点」に「前提の欄が無い」と書く（親が次の依頼で欄を書くため。欄の無い追加の指示には書かない）

- 報告には、直した指摘ごとに「再現: <テスト名 | 手順 | 出典>」を「確かめたこと」に、直さない指摘ごとに「再現なし: <指摘の要旨>」（別のトピックとして
  issue にしたものは「再現なし → #<issue の番号>: <指摘の要旨>」）「前提が誤り: <確かめた事実>」「抜け道: <理由>」のどれかと、直さない理由を
  その場の 1 文で「対応しなかった指摘」に書く（親が分け方を確かめるため。issue にするかは /git-merge の「直す指摘の基準」の
  「直さない（non-blocking）」の項に従う）。
MD

# 同じブランチの後の版の 5 件（凍結する部分の一覧と理由、「順に」、行の形の一覧、普通の手順のまとめの語、
# range-diff と git log のコマンドが落ちた）
cat >"$D/slim.md" <<'MD'
rebase の前に、古い土台（`git merge-base main [branch]`）と旧先端（`git rev-parse [branch]`）を控える（「レビュー」の rebase 後の
判断に使い、rebase の後では取れない）。

- claude-hooks の、普通に書く形（`cd <worktree> &&`、`env` や `timeout` の前置き、`bash -c '…'`、`nix develop -c` など）でない
  抜け道（珍しい git の設定や全体オプションで行き先を変える、pipefail を途中で外す、など作為的な書き方）の指摘は直さず、無いものと
  して打ち切りを決める（hook は境界でなく、塞ぐたびに次の反例が出て周が終わらない。抜け道はモデルの判断と人のレビューに任せる）。
  普通の形を誤って止める指摘は直す

- 直す（blocking）:
  - (b) ADR の凍結する部分の事実の誤りで、一次情報で確かめたもの（マージの前に直す理由は上の項と同じ）

- 依頼の「前提」の欄の行と、欄の外（ほかの欄、案、欄の無い依頼）に事実として書かれ判断の前提にする値と前提を、作業の前に
  出典で確かめる（出典の種類は頼む側の「前提」の項）。出典が添えてなければ探して確かめ、その行と見つけた出典を報告の「確認の
  結果」に書く（親が書き漏れに気づくため）。食い違うか出典が見つからなければ、報告に書いて止まる。「確かめていない」と添えた
  値は止まる対象でなく、確かめることが作業の一部で、結果を報告に書く。作業中に誤りと分かり、それに頼った作業が成り立たなく
  なれば、その時点で止まる。欄の外で確かめた事実も、出典とともに「確認の結果」に書く。最初の依頼に「前提」の欄が無ければ、「親が先に読む要点」に「前提の欄が無い」と書く（追加の指示には
  書かない）

- 報告には、直した指摘ごとに「再現: <テスト名 | 手順 | 出典>」を「確かめたこと」に、直さない指摘ごとに「直す指摘の基準」の
  行の形（`再現なし: …`、`再現なし → #…: …`、`前提が誤り: …`、`抜け道: …`）と直さない理由の 1 文を「対応しなかった指摘」に書く
  （親が分け方を確かめるため）。
MD

it "kept-sentences: 縮めたときに字句の変わった 5 件の文を、ファイルと行つきで出して 1 で終わる"
out=$("$KEPT" --old "$D/old.md" --new "$D/slim.md" 2>&1)
assert_eq 1 $?
assert_contains "$out" "CHANGED: $D/slim.md:1: rebase の前に、古い土台"
assert_contains "$out" "判断に使い、rebase の後では取れない）。"
assert_contains "$out" "\`nix develop -c\` など）でない"
assert_contains "$out" "（マージの前に直す理由は上の項と同じ）"
assert_contains "$out" "CHANGED: $D/slim.md:12: 依頼の「前提」の欄の行と、欄の外"
assert_contains "$out" "\`前提が誤り: …\`、\`抜け道: …\`）と直さない理由の 1 文"

it "kept-sentences: 元の版をそのまま渡せば、何も出さず 0 で終わる"
out=$("$KEPT" --old "$D/old.md" --new "$D/old.md" 2>&1)
assert_eq 0 $?
assert_eq "" "$out"

it "kept-sentences: 文を消しただけ、行の折り返し、箇条書きの印、見出しの段、字下げの違いは数えない"
cat >"$D/a.md" <<'MD'
## 見出し

最初の文。消す文。最後の
文。

- 項目の文は
  折り返す。
MD
cat >"$D/b.md" <<'MD'
# 見出し

最初の文。最後の文。

項目の文は折り返す。
MD
out=$("$KEPT" --old "$D/a.md" --new "$D/b.md" 2>&1)
assert_eq 0 $?

it "kept-sentences: 1 文の中の語を 1 つ落としても見つける"
cat >"$D/c.md" <<'MD'
最初の文。最後の文。

- 項目の文は折り返す。
MD
sed 's/最後の文/最後の/' "$D/c.md" >"$D/c2.md"
out=$("$KEPT" --old "$D/c.md" --new "$D/c2.md" 2>&1)
assert_eq 1 $?
assert_contains "$out" "CHANGED: $D/c2.md:1: 最後の。"
assert_not_contains "$out" "最初の文"

it "kept-sentences: 別の段落の文をつないだ文は、元の版で隣り合っていても見つける"
printf '%s\n' '前の段落の文' '' '次の段落の文。' >"$D/e.md"
printf '%s\n' '前の段落の文次の段落の文。' >"$D/e2.md"
out=$("$KEPT" --old "$D/e.md" --new "$D/e2.md" 2>&1)
assert_eq 1 $?

it "kept-sentences: 今の版を複数のファイルに分けても、元の版の文なら数えない"
printf '%s\n' '- 一つ目の文。' '- 二つ目の文。' >"$D/f.md"
printf '%s\n' '- 一つ目の文。' >"$D/f1.md"
printf '%s\n' '# 移した先' '' '- 二つ目の文。' >"$D/f2.md"
out=$("$KEPT" --old "$D/f.md" --new "$D/f1.md" "$D/f2.md" 2>&1)
assert_eq 1 $?
assert_contains "$out" "CHANGED: $D/f2.md:1: 移した先"
assert_not_contains "$out" "二つ目の文"

it "kept-sentences: コードブロックは行ごとに比べる"
printf '%s\n' '```bash' 'jq -r . a.json' '```' >"$D/g.md"
printf '%s\n' '```bash' 'jq -r .x a.json' '```' >"$D/g2.md"
out=$("$KEPT" --old "$D/g.md" --new "$D/g2.md" 2>&1)
assert_eq 1 $?
assert_contains "$out" "CHANGED: $D/g2.md:2: jq -r .x a.json"

it "kept-sentences: 引数が足りないか読めないファイルは 2 で終わる"
"$KEPT" --old "$D/a.md" >/dev/null 2>&1
assert_eq 2 $?
"$KEPT" --old "$D/none.md" --new "$D/a.md" >/dev/null 2>&1
assert_eq 2 $?

finish
