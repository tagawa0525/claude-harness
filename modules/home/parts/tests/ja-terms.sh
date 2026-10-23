#!/usr/bin/env bash
# =============================================================================
# git の pre-commit と commit-msg hook が、崩れた日本語の語を辞書（../ja-terms/prh.yml）で止めることを
# 検証する（ADR-0014）
# =============================================================================
# pre-commit はステージした中身（作業ツリーではなく）の Markdown、テキスト、コードを、commit-msg は
# Claude Code のセッションのコミットの本文を textlint と prh で検査する。辞書の項目は、名詞の用法を
# 止め、動詞や別の語を通すことも確かめる。
#
# hook のファイルを nix build で作る（hook が store のパスで呼ぶ textlint と設定も一緒にビルドされる）。
#
# 使い方（リポジトリルートで実行）:
#   ./modules/home/parts/tests/ja-terms.sh

set -euo pipefail

HOST="${1:-r995}"
ROOT=$(git rev-parse --show-toplevel)

WORK=$(mktemp -d)
trap 'rm -rf "$WORK"' EXIT

HOOKS="$WORK/hooks"
mkdir -p "$HOOKS"
for hook in pre-commit commit-msg; do
  echo "==> $hook hook をビルドする（nixosConfigurations.${HOST}）"
  built=$(nix build --no-link --print-out-paths --option warn-dirty false \
    "$ROOT#nixosConfigurations.${HOST}.config.home-manager.users.tagawa.xdg.configFile.\"git/hooks/$hook\".source")
  cp "$built" "$HOOKS/$hook"
  chmod +x "$HOOKS/$hook"
done

export HOME="$WORK/home"
mkdir -p "$HOME"
export GIT_CONFIG_GLOBAL="$HOME/.gitconfig"
export GIT_CONFIG_NOSYSTEM=1
git config --global user.name test
git config --global user.email test@example.com
git config --global init.defaultBranch main
git config --global core.hooksPath "$HOOKS"
# GitHub リモートが無いので main ガードは効かない。commit-msg の検査は Claude Code のセッションだけ
unset CLAUDECODE

PASS=0
FAIL=0

ok() { PASS=$((PASS + 1)); echo "✓ $1"; }
ng() { FAIL=$((FAIL + 1)); echo "✗ $1"; sed 's/^/    /' "$WORK/out"; }

# new_repo <dir>: main に初期コミットを持つ repo を作る
new_repo() {
  git init -q -b main "$1"
  echo init > "$1/README.md"
  git -C "$1" add README.md
  git -C "$1" commit -q --no-verify -m "chore: init"
}

# commits <repo> <path> <text>: ファイルに text を書いてステージし、コミットできれば 0
commits() {
  printf '%s\n' "$3" > "$1/$2"
  git -C "$1" add -- "$2"
  git -C "$1" commit -q -m "docs: add" >"$WORK/out" 2>&1
}

# --- 1. 辞書の項目: 名詞の用法を止め、動詞や別の語を通す ---------------------------------------------
# 本文の例は Markdown の文書としても正しい形にする（markdownlint に落ちないよう、見出しの後に 1 段落）
while IFS='|' read -r expect text; do
  [ -n "$expect" ] || continue
  REPO="$WORK/dict-$PASS-$FAIL"
  new_repo "$REPO"
  if commits "$REPO" doc.md "$(printf '# t\n\n%s\n' "$text")"; then
    result=pass
  else
    result=stop
  fi
  if [ "$result" = "$expect" ]; then
    ok "辞書: 「$text」を $expect"
  else
    ng "辞書: 「$text」を $expect のはずが $result"
  fi
done <<'CASES'
stop|確かめの結果を書く。
stop|確かめが通らない。
pass|結果を確かめる。
pass|結果を確かめた。
pass|観測者にも確かめにくい。
pass|結果は確かめがたい。
pass|結果を確かめがちだ。
pass|散歩がてら確かめがてら書く。
pass|2 つは比べがたい。
pass|結果を確かめもせずに書く。
pass|結果を確かめはしたが違う。
pass|結果を確かめ Slurm に投げる。
pass|結果を確かめに行く。
stop|比べ合わせの結果。
pass|2 つを比べ合わせると近い。
stop|2 つの比べでは差が無い。
pass|2 つを比べると差が無い。
pass|設定の効きを確かめる。
pass|目が効きます。
stop|件の数えが 3 件。
pass|二つを、比べはしたが違う。
stop|端のケースを足す。
stop|作業木を作る。
stop|掃引の範囲。
stop|点断面積を読む。
stop|前付けの検査を確かめる。
stop|コマンドの包みのほどき方。
stop|包みを剥がす。
pass|荷物を包み込む。
stop|panic を拾って何も出さない。
stop|拾った panic も判定に入れる。
pass|音を拾って波形にする。
stop|判定に抜けが続いた。
stop|抜けを塞ぐ。
pass|隙間を抜けると外に出る。
pass|hook のすり抜けを塞ぐ。
pass|前の台帳と突き合わせ、同じなら除く。
pass|抜けがちな項目を足す。
stop|突き合わせが止める。
stop|突き合わせでずれを見つける。
pass|2 つを突き合わせると一致する。
stop|jobs の始まりの見込みがずれる。
stop|署名の始まりの印を探す。
pass|始まりの時刻を読む。
stop|雛形はスクリプトで作る。
stop|全てのコミットを移す。
stop|テストが全て通る。
pass|すべてのコミットを移す。
stop|パス付きの指定を使う。
stop|テスト付きに直す。
stop|見出し付きで出力する。
stop|日付付きの追記を足す。
pass|言語が付き、閉じる。
pass|付き合いが長い。
pass|日付つきの追記を足す。
pass|設定に気付きの機会を設ける。
stop|シンボリックリンクを作る。
stop|ファイルを写す。
stop|設定を写した。
stop|ブランチを GitHub に写して保つ。
stop|権限を写さず、作り直す。
stop|コードの挙動が文書に写される。
stop|書き写すと費用になる。
stop|バックアップは作業用の写しにする。
stop|panic を拾い、記録する。
stop|別のファイルは写さない。
pass|設定を写し、再起動する。
pass|参照が結び付きます。
pass|ログが貼り付きます。
pass|壁の突き抜けを防ぐ。
pass|レースの追い抜けが起きる。
stop|デフォルト設定の写し（Normal を変更）。
stop|別のファイルは書き写さない。
pass|挙動を描写する。
pass|挙動を描写した。
pass|名前付けの規則を決める。
pass|参照が紐付きます。
pass|底抜けの楽観を戒める。
pass|以前付けた名前を使う。
pass|顔が写らない部分がある。
stop|書き写しを残す。
pass|写しがたい構造だ。
pass|引き抜けが起きた。
pass|読み抜けを防ぐ。
stop|同じ内容を写し続ける。
stop|panic を拾う経路を足す。
stop|拾った panic も数える。
stop|ルールが panic を拾った。
stop|panic を拾わず、止まる。
pass|近付きがたい雰囲気だ。
pass|小包みの中身を見る。
pass|包みがちな荷物を解く。
stop|致命的な抜けが残った。
pass|panic を拾い上げる。
pass|追い付きを待つ。
pass|噛み付きを防ぐ。
stop|全部書き写す。
stop|同じ内容を書き写し続ける。
stop|書き写されている。
stop|包みごとの意味を読む。
stop|包み（env、sudo）を読む。
stop|包みと bash の文字列を見る。
stop|sudo などの包み、を足す。
pass|ケーキを包み直す。
pass|紙で包みます。
pass|紅葉が色付き始める。
pass|紙で包みました。
pass|荷物を包みたい。
pass|包みやすい形にする。
pass|包み始める。
pass|包み方を決める。
pass|写真を撮る。
pass|画面に映す。
stop|終端子を探す。
stop|汚い電源断で壊れる。
stop|依存物を入れる。
pass|symlink を作る。
stop|検証の道具を作る。
pass|小道具を用意する。
stop|燃焼鎖を読む。
stop|崩壊鎖を読む。
stop|鎖の核種を並べる。
pass|連鎖が起きる。
pass|ポートを閉鎖する。
stop|正味の流れを求める。
stop|走行の記録を残す。
stop|ジョブを走らせる。
stop|ジョブを走らせた。
stop|ジョブを走らせて待つ。
stop|ジョブを走らせない。
stop|ジョブを走らせず待つ。
stop|ジョブを走らせれば分かる。
stop|走らせ方を決める。
stop|ジョブを走らせ直す。
stop|ジョブを走らせ直した。
stop|ジョブを走らせ直して待つ。
stop|ジョブを走らせ直さない。
stop|走らせ直しの手順。
stop|走り直しを避ける。
stop|流し直しの手順。
stop|計算を流し直す。
pass|車を並べて走り抜ける。
stop|設定の使い回しを避ける。
stop|設定を使い回す。
stop|設定を使い回した。
stop|設定を使い回して書く。
stop|設定を使い回さない。
stop|乱数の種を固定する。
pass|花の種をまく。
stop|閉包を取る。
stop|確率表を読む。
stop|測り直しが要る。
stop|断面積を測り直す。
stop|断面積を測り直した。
stop|断面積を測り直して比べる。
stop|直し方を書く。
stop|リンクの行き先を示す。
stop|ジョブを見張る。
stop|ジョブを見張って待つ。
stop|ジョブを見張った。
stop|ジョブを見張らない。
stop|ジョブを見張らず待つ。
stop|ジョブが見張られる。
stop|ジョブの見張りを置く。
stop|燃料を燃やす。
stop|燃料を燃やした。
stop|燃料を燃やして測る。
stop|燃料を燃やさない。
stop|燃料を燃やせる。
stop|燃料を燃やせない。
stop|燃料が燃やされる。
stop|燃やし、測る。
pass|燃え残りを数える。
stop|捨てる世代を決める。
stop|数える世代を決める。
stop|中性子の釣り合いを取る。
stop|釣り合いが取れる。
stop|釣り合いを見る。
stop|物質と群の釣り合い、反応率を測る。
stop|数値が釣り合いや保存則で閉じる。
stop|釣り合いと保存則を確かめる。
stop|反応率の和（釣り合い）を見る。
stop|外のビンは、釣り合い（すべての群）を見る。
stop|収支と釣り合い。
pass|二つが釣り合い、かつ安定する。
pass|二つが釣り合いやすい。
pass|左右の釣り合いにくさを見る。
pass|力の釣り合いにくい配置を使う。
pass|二つが釣り合いません。
pass|不釣り合いや遅延を見る。
pass|反応率（不釣り合い）を見る。
pass|不釣り合いな実行時間を直す。
pass|二つが釣り合う。
pass|二つが釣り合わない。
stop|格子の枠なしを使う。
stop|枠ありの配置を使う。
stop|枠のリングを足す。
stop|枠の物質を指定する。
stop|枠を入れる。
stop|枠による差を見る。
stop|枠のある配置を使う。
stop|枠を足す。
pass|Copilot の枠を使い切る。
pass|使用量の枠を足す。
stop|DVC の段階を流す。
pass|2 段階の手法を使う。
pass|段階的に進める。
pass|乱数の種類を選ぶ。
pass|確率表現を確認する。
pass|二つが釣り合いにくい。
pass|断面積を測り直しにくい。
pass|DVC の段階的な実行を確認する。
stop|16 スレッドの壁時計は 63 秒。
stop|壁時計の時間で測る。
stop|壁時計時間で 63 秒。
stop|壁時計で測った時間は 63 秒。
pass|壁時計の時刻を読む。
pass|壁時計が遅れている。
pass|壁時計の針を合わせる。
pass|壁時計を見る。
pass|壁時計の遅れを直す。
stop|壁時計の時間を見る。
stop|xs-nuclide の段階を流す。
stop|重い段階を 1 本ずつ流す。
stop|上流の段階の deps が変わる。
stop|core の段階は走らない。
stop|OpenMC の段階が再実行する。
stop|xs の段階の md5 を比べる。
pass|次の段階を実行する。
pass|前の段階の出力を入力にする。
pass|2026-10-22 の段階では未対応。
pass|PR の段階で気づく。
pass|2 段階の出力を比べる。
pass|2段階の出力を比べる。
pass|二段階の出力を比べる。
pass|2 つの段階の出力を比べる。
pass|最終段階を投げる。
pass|設計の上流の段階で決める。
stop|背景断面積 σ0 で遮蔽する。
stop|背景の R 行列を足す。
stop|負の背景を足す。
stop|背景σ0 を変える。
stop|チェックを背景で回す。
stop|背景で回さない。
stop|チェックが背景で走る。
stop|背景のジョブを待つ。
pass|ADR の背景では付けてよいとした。
pass|決定の背景である。
pass|この決定の背景に流れる考え。
pass|背景で回避した。
pass|背景に投げかける問い。
pass|背景なしでは読めない。
pass|勝負の背景を書く。
pass|ADR の背景が負担になる。
pass|背景の担当者に聞く。
pass|道具立てを確認する。
pass|燃料を燃やしきる。
stop|ジョブを走らせられる。
stop|ジョブを走らせられない。
stop|ジョブを走らせられません。
stop|ジョブを走らせ、結果を読む。
stop|ジョブを走らせ直せる。
stop|設定が使い回される。
stop|設定を使い回しうる。
stop|測り直しになる。
CASES

# --- 2. コードのコメントも検査する --------------------------------------------------------------------
REPO="$WORK/code"
new_repo "$REPO"
if commits "$REPO" lib.rs '// 端のケースを扱う'; then
  ng "Rust のコメントの崩れた語で止まらない"
else
  if grep -q "エッジケース" "$WORK/out"; then
    ok "Rust のコメントの崩れた語で止め、直す語を示す"
  else
    ng "Rust のコメントで止めたが、直す語を示さない"
  fi
fi

# --- 3. ステージした中身を見る（作業ツリーの、ステージしていない編集は見ない）--------------------------
# Markdown の自動修正（pre-commit-partial-stage.sh が確かめる）と切り分けるため、テキストのファイルで確かめる
REPO="$WORK/staged"
new_repo "$REPO"
printf '結果を確かめる。\n' > "$REPO/doc.txt"
git -C "$REPO" add doc.txt
printf '確かめの結果。\n' > "$REPO/doc.txt"
if git -C "$REPO" commit -q -m "docs: add" >"$WORK/out" 2>&1; then
  ok "ステージしていない編集の崩れた語では止めない"
else
  ng "ステージしていない編集の崩れた語で止めた"
fi
REPO="$WORK/staged2"
new_repo "$REPO"
printf '確かめの結果。\n' > "$REPO/doc.txt"
git -C "$REPO" add doc.txt
printf '結果を確かめる。\n' > "$REPO/doc.txt"
if git -C "$REPO" commit -q -m "docs: add" >"$WORK/out" 2>&1; then
  ng "ステージした中身の崩れた語で止めない（作業ツリーを見ている）"
else
  ok "ステージした中身の崩れた語で止める"
fi

# --- 3b. 行を折り返した動詞は止めない ----------------------------------------------------------------
REPO="$WORK/wrapped"
new_repo "$REPO"
if commits "$REPO" doc.md "$(printf '# t\n\n計算の結果を確かめ\nる。\n')"; then
  ok "行を折り返した動詞は止めない"
else
  ng "行を折り返した動詞で止めた"
fi

REPO="$WORK/wrapped-tsutsumi"
new_repo "$REPO"
if commits "$REPO" doc.md "$(printf '# t\n\n荷物を包み\n込む。\n')"; then
  ok "行を折り返した包み込むは止めない"
else
  ng "行を折り返した包み込むで止めた"
fi

# --- 3c. グロブの文字を含む名前、大文字の拡張子、シンボリックリンク -------------------------------------
REPO="$WORK/names"
new_repo "$REPO"
if commits "$REPO" "[draft].md" "$(printf '# t\n\n結果を確かめる。\n')"; then
  ok "グロブの文字を含む名前の、崩れていないファイルは通す"
else
  ng "グロブの文字を含む名前の、崩れていないファイルで止めた"
fi
if commits "$REPO" "[draft2].md" "$(printf '# t\n\n確かめの結果。\n')"; then
  ng "グロブの文字を含む名前の崩れた語で止まらない"
else
  if grep -qF "[draft2].md" "$WORK/out"; then
    ok "グロブの文字を含む名前の崩れた語で止め、元のパスを示す"
  else
    ng "グロブの文字を含む名前で止めたが、元のパスを示さない"
  fi
fi
# 止めた [draft2].md はステージに残るので、外してから確かめる
git -C "$REPO" reset -q
rm -f "$REPO/[draft2].md"
if commits "$REPO" "UPPER.MD" "$(printf '# t\n\n確かめの結果。\n')"; then
  ng "大文字の拡張子の崩れた語で止まらない"
else
  ok "大文字の拡張子の崩れた語で止める"
fi
# 行き先は崩れた語を含むが、検査を通さずにコミットしておく。リンクだけをコミットして、行き先を読まないことを見る
printf '確かめの結果。\n' > "$REPO/target.txt"
git -C "$REPO" add target.txt
git -C "$REPO" commit -q --no-verify -m "chore: target"
ln -s target.txt "$REPO/link.txt"
git -C "$REPO" add link.txt
if git -C "$REPO" commit -q -m "docs: link" >"$WORK/out" 2>&1; then
  ok "シンボリックリンクは検査しない"
else
  ng "シンボリックリンクで止めた"
fi

# --- 3d. 行を折り返した「数え」、名前を変えたファイル -----------------------------------------------------
REPO="$WORK/renames"
new_repo "$REPO"
if commits "$REPO" doc.md "$(printf '# t\n\n行の数え\nる。\n')"; then
  ok "行を折り返した動詞の数えは止めない"
else
  ng "行を折り返した動詞の数えで止めた"
fi
printf '# t\n\nok\n' > "$REPO/a.md"
git -C "$REPO" add a.md
git -C "$REPO" commit -q --no-verify -m "chore: a"
git -C "$REPO" mv a.md b.md
printf '# t\n\n確かめの結果。\n' > "$REPO/z.md"
git -C "$REPO" add z.md
if git -C "$REPO" commit -q -m "docs: rename" >"$WORK/out" 2>&1; then
  ng "名前を変えたファイルと一緒にステージした崩れた語で止まらない"
else
  if grep -q "z.md" "$WORK/out" && grep -q "確認" "$WORK/out"; then
    ok "名前を変えたファイルと一緒でも、ほかのファイルの崩れた語で止める"
  else
    ng "名前を変えたファイルと一緒のときに、崩れた語ではない理由で止めた"
  fi
fi

# --- 3e. サブモジュール（gitlink）は検査しない -----------------------------------------------------------
# Markdown の自動修正は作業ツリーに無いファイルをステージから消すので、テキストの拡張子の名前で確かめる
REPO="$WORK/gitlink"
new_repo "$REPO"
git -C "$REPO" update-index --add --cacheinfo "160000,$(git -C "$REPO" rev-parse HEAD),sub.txt"
if git -C "$REPO" commit -q -m "chore: gitlink" >"$WORK/out" 2>&1; then
  ok "サブモジュール（gitlink）は検査しない"
else
  ng "サブモジュール（gitlink）で止めた"
fi

# --- 3f. 種類の変更（シンボリックリンクから普通のファイル）だけのコミットも見る ----------------------------
REPO="$WORK/typechange"
new_repo "$REPO"
echo target > "$REPO/target.txt"
ln -s target.txt "$REPO/note.txt"
git -C "$REPO" add target.txt note.txt
git -C "$REPO" commit -q --no-verify -m "chore: link"
rm "$REPO/note.txt"
printf '確かめの結果。\n' > "$REPO/note.txt"
git -C "$REPO" add note.txt
if git -C "$REPO" commit -q -m "docs: note" >"$WORK/out" 2>&1; then
  ng "種類の変更だけのコミットの崩れた語で止まらない"
elif grep -q "note.txt" "$WORK/out" && grep -q "確認" "$WORK/out"; then
  ok "種類の変更だけのコミットの崩れた語でも止める"
else
  ng "種類の変更だけのコミットで止めたが、崩れた語の場所を示さない"
fi

# --- 4. 空白を含むパス -------------------------------------------------------------------------------
REPO="$WORK/space path"
new_repo "$REPO"
mkdir -p "$REPO/a b"
if commits "$REPO" "a b/doc.md" "$(printf '# t\n\n確かめの結果。\n')"; then
  ng "空白を含むパスの崩れた語で止まらない"
else
  ok "空白を含むパスの崩れた語で止める"
fi

# --- 4b. prh の辞書は誤りを並べるので検査しない ------------------------------------------------------
REPO="$WORK/dictionary"
new_repo "$REPO"
mkdir -p "$REPO/ja-terms"
if commits "$REPO" ja-terms/prh.yml "$(printf 'rules:\n  - expected: 確認\n    pattern: 確かめの\n')"; then
  ok "prh の辞書のファイルは検査しない"
else
  ng "prh の辞書のファイルで止めた"
fi

# --- 4c. リポジトリの .textlintignore に挙げたファイルは検査しない ------------------------------------
REPO="$WORK/ignore"
new_repo "$REPO"
printf 'fixtures/**\n' > "$REPO/.textlintignore"
git -C "$REPO" add .textlintignore
git -C "$REPO" commit -q --no-verify -m "chore: ignore fixtures"
mkdir -p "$REPO/fixtures"
if commits "$REPO" fixtures/case.md "$(printf '# t\n\n確かめの結果。\n')"; then
  ok ".textlintignore に挙げたファイルは検査しない"
else
  ng ".textlintignore に挙げたファイルで止めた"
fi
if commits "$REPO" 'fixtures/[ab].md' "$(printf '# t\n\n確かめの結果。\n')"; then
  ok ".textlintignore に挙げた、[ を含む名前のファイルも検査しない"
else
  ng ".textlintignore に挙げた、[ を含む名前のファイルで止めた"
fi
if commits "$REPO" 'fixtures/a].md' "$(printf '# t\n\n確かめの結果。\n')"; then
  ok ".textlintignore に挙げた、] を含む名前のファイルも検査しない"
else
  ng ".textlintignore に挙げた、] を含む名前のファイルで止めた"
fi

# --- 5. fork では飛ばす ------------------------------------------------------------------------------
REPO="$WORK/fork"
new_repo "$REPO"
git -C "$REPO" remote add upstream https://example.com/upstream.git
if commits "$REPO" doc.md "$(printf '# t\n\n確かめの結果。\n')"; then
  ok "fork では日本語の語を検査しない"
else
  ng "fork で日本語の語を検査した"
fi

# --- 6. コミットの本文（Claude Code のセッション）---------------------------------------------------
REPO="$WORK/message"
new_repo "$REPO"
echo x > "$REPO/x.txt"
git -C "$REPO" add x.txt
if CLAUDECODE=1 git -C "$REPO" commit -q -m "docs: 確かめの結果を書く" >"$WORK/out" 2>&1; then
  ng "コミットの本文の崩れた語で止まらない"
else
  if grep -q "確認" "$WORK/out"; then
    ok "コミットの本文の崩れた語で止め、直す語を示す"
  else
    ng "コミットの本文で止めたが、直す語を示さない"
  fi
fi
# 文書（.md）の検査と同じく、バッククォートで囲んだコードスパンの中は検査しない（識別子やファイル名の語）
echo c > "$REPO/c.txt"
git -C "$REPO" add c.txt
if CLAUDECODE=1 git -C "$REPO" commit -q -m "docs: add" -m '`確かめの結果` を書く' >"$WORK/out" 2>&1; then
  ok "コミットの本文のコードスパンの中の崩れた語は止めない"
else
  ng "コミットの本文のコードスパンの中の崩れた語で止めた"
fi
echo d > "$REPO/d.txt"
git -C "$REPO" add d.txt
if CLAUDECODE=1 git -C "$REPO" commit -q -m "docs: add" -m '`foo` と確かめの結果を書く' >"$WORK/out" 2>&1; then
  ng "コミットの本文のコードスパンの外の崩れた語で止まらない"
elif grep -q "確認" "$WORK/out"; then
  ok "コミットの本文のコードスパンの外の崩れた語は止め、直す語を示す"
else
  ng "コードスパンの外の崩れた語で止めたが、直す語を示さない"
fi
echo z > "$REPO/z.txt"
git -C "$REPO" add z.txt
if CLAUDECODE=1 git -C "$REPO" commit -q -m "squash! docs: add" -m "確かめの結果を書く" >"$WORK/out" 2>&1; then
  ng "squash! のコミットの本文の崩れた語で止まらない"
else
  ok "squash! のコミットの本文の崩れた語でも止める"
fi
echo y > "$REPO/y.txt"
git -C "$REPO" add y.txt
printf 'docs: 結果を確かめる\n\n本文\n# ------------------------ >8 ------------------------\n確かめの差分\n' > "$WORK/msg"
if CLAUDECODE=1 git -C "$REPO" commit -q -F "$WORK/msg" --cleanup=scissors >"$WORK/out" 2>&1; then
  ok "コミットの本文の scissors の行から後は検査しない"
else
  ng "コミットの本文の scissors の後で止めた"
fi
echo w > "$REPO/w.txt"
git -C "$REPO" add w.txt
if CLAUDECODE=1 git -C "$REPO" commit -q -m "docs: add" -m "#290 の確かめの結果" >"$WORK/out" 2>&1; then
  ng "-m で残る # で始まる行の崩れた語で止まらない"
else
  ok "-m で残る # で始まる行の崩れた語でも止める"
fi
echo v > "$REPO/v.txt"
git -C "$REPO" add v.txt
if CLAUDECODE=1 git -C "$REPO" commit -q -m "fixup! docs: 確かめの結果を書く" >"$WORK/out" 2>&1; then
  ok "fixup! の件名（元のコミットから写したもの）は検査しない"
else
  ng "fixup! の件名で止めた"
fi

# --- 6b. 検査する本文が空のときは textlint を呼ばない（空の標準入力だと使い方の全文を出す）---------------
echo u > "$REPO/u.txt"
git -C "$REPO" add u.txt
if CLAUDECODE=1 git -C "$REPO" commit -q -m "fixup! docs: add" >"$WORK/out" 2>&1; then
  if [ -s "$WORK/out" ] && grep -q "textlint \[options\]" "$WORK/out"; then
    ng "本文が空の fixup! のコミットが textlint の使い方を出す"
  else
    ok "本文が空の fixup! のコミットは textlint を呼ばず、何も出さない"
  fi
else
  ng "本文が空の fixup! のコミットが止まった"
fi
echo s > "$REPO/s.txt"
git -C "$REPO" add s.txt
printf 'Merge branch x\n\n  \n\n' > "$WORK/msg"
if CLAUDECODE=1 git -C "$REPO" commit -q -F "$WORK/msg" --cleanup=verbatim >"$WORK/out" 2>&1 \
  && ! grep -q "textlint \[options\]" "$WORK/out"; then
  ok "空白だけの本文の Merge のコミットは textlint を呼ばない"
else
  ng "空白だけの本文の Merge のコミットが止まる、または textlint の使い方を出す"
fi
echo t > "$REPO/t.txt"
git -C "$REPO" add t.txt
if CLAUDECODE=1 git -C "$REPO" commit -q -m "fixup! docs: add" -m "確かめの結果を書く" >"$WORK/out" 2>&1; then
  ng "fixup! の本文の崩れた語で止まらない（空の本文の分岐が本文のある場合を飛ばした）"
else
  ok "fixup! の本文のある場合は今までどおり検査する"
fi

echo
echo "passed: $PASS, failed: $FAIL"
(( FAIL == 0 ))
