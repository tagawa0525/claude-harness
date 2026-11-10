#!/usr/bin/env bash
# .claude/scripts/slurm-history.sh --job（sacct のジョブの記録から、排他か、コマンド、走った版を読む）のテスト
#
# 実行: bash .claude/tests/scripts/slurm-history-job.sh（まとめて実行: bash .claude/tests/scripts.sh）

source "$(dirname "$0")/../lib.sh"

# sacct の偽物。引数を記録し、-j の番号の記録（$FAKE_SACCT/<番号>）を出す。無ければ何も出さずに 0（本物と同じ）
FAKE_BIN="$TEST_ROOT/bin"
FAKE_SACCT="$TEST_ROOT/sacct"
mkdir -p "$FAKE_BIN" "$FAKE_SACCT"
cat >"$FAKE_BIN/sacct" <<'EOF'
#!/usr/bin/env bash
printf '%s\n' "$@" >"$TEST_ROOT/sacct.args"
while (($#)); do
  if [[ $1 == -j ]]; then
    [[ ! -f $FAKE_SACCT/$2 ]] || cat "$FAKE_SACCT/$2"
    exit 0
  fi
  shift
done
exit 1
EOF
chmod +x "$FAKE_BIN"/*
export PATH="$FAKE_BIN:$PATH" TEST_ROOT FAKE_SACCT

HISTORY="$SCRIPTS_DIR/slurm-history.sh"
# 30 日の reflog の期限に掛からない日付で書く
DAY=$(date -d '2 days ago' +%F)
OLD_DAY=$(date -d '40 days ago' +%F)

# record <番号> <状態> <Submit> <Start> <End> <WorkDir> <SubmitLine>: sacct の 1 件（区切りは \x1f）
record() {
  local us=$'\x1f'
  printf '%s\n' "$1${us}job-$1${us}$2${us}00:10:00${us}$3${us}$4${us}$5${us}$6${us}$7" >"$FAKE_SACCT/$1"
}

# submit_line <資源の引数> <連絡先> <コマンド>: slurm-run.sh が投げた形の SubmitLine
submit_line() {
  printf '%s' "/nix/store/x-slurm/bin/sbatch $1 --partition=main --wait --export=ALL -J j -o /logs/%x-%j.out --time 10 --comment decision | #1 | 結果で方式を決める | $2 --wrap $3"
}

# at <時刻> <コマンド…>: reflog の項の時刻（committer の日付）を決めて git を実行する
at() {
  local t="$DAY"T$1
  shift
  GIT_COMMITTER_DATE="$t" GIT_AUTHOR_DATE="$t" "$@"
}

# setup_repo: 本体 $REPO（main に 09:00 のコミット）と、09:30 に作った worktree $WT（feat/x、09:40 のコミット $C1）
setup_repo() {
  rm -rf "$TEST_ROOT/proj" "$TEST_ROOT/proj-feat-x"
  REPO="$TEST_ROOT/proj"
  WT="$TEST_ROOT/proj-feat-x"
  mkdir -p "$REPO"
  git -C "$REPO" init -q -b main
  echo a >"$REPO/input.toml"
  git -C "$REPO" add input.toml
  at 09:00:00 git -C "$REPO" commit -q -m init
  at 09:30:00 git -C "$REPO" worktree add -q -b feat/x "$WT"
  echo b >"$WT/input.toml"
  at 09:40:00 git -C "$WT" commit -q -am b
  C1=$(git -C "$WT" rev-parse HEAD)
  C1_SHORT=$(git -C "$WT" rev-parse --short=9 HEAD)
}

setup_repo
record 101 COMPLETED "$DAY"T10:00:00 "$DAY"T10:00:00 "$DAY"T11:00:00 "$WT" \
  "$(submit_line --exclusive "feat/x $WT commit=$C1_SHORT claude=s" 'lattice input.toml > out.txt')"

it "--job: sacct を番号ごとに、親の行だけ、見出しなしで、区切りの文字と欄の順を決めて読む"
out=$("$HISTORY" --job 101)
assert_eq 0 $?
args=$(cat "$TEST_ROOT/sacct.args")
assert_contains "$args" $'-j\n101'
assert_contains "$args" "-X"
assert_contains "$args" "-n"
assert_contains "$args" $'-o\nJobID,JobName,State,Elapsed,Submit,Start,End,WorkDir,SubmitLine'

it "--job: 状態、経過時間、投げた・始まった・終わった時刻、作業ディレクトリを出す"
assert_contains "$out" "JOB: 101 job-101"
assert_contains "$out" "STATE: COMPLETED"
assert_contains "$out" "ELAPSED: 00:10:00"
assert_contains "$out" "TIMES: submit=${DAY}T10:00:00 start=${DAY}T10:00:00 end=${DAY}T11:00:00"
assert_contains "$out" "WORKDIR: $WT"

it "--job: sbatch の直後が --exclusive のジョブは排他と出す"
assert_contains "$out" "EXCLUSIVE: yes"

it "--job: コマンドは SubmitLine の --wrap の後を出す"
assert_contains "$out" "COMMAND: lattice input.toml > out.txt"

it "--job: 連絡先のブランチと commit= を出す"
assert_contains "$out" "BRANCH: feat/x"
assert_contains "$out" "COMMIT: $C1_SHORT"

it "--job: commit= のコミットが本体にあり、投げてから終わるまで reflog の項が無ければ、その完全な hash を走った版と出す"
assert_contains "$out" "VERSION: $C1"
assert_not_contains "$out" "REASON:"
assert_contains "$out" "NOTE: 投げた後のコミットしない編集は確かめていない"

it "--job: -c のジョブは排他でないと出し、--cpus-per-task の値を添える"
record 102 COMPLETED "$DAY"T10:00:00 "$DAY"T10:00:00 "$DAY"T11:00:00 "$WT" \
  "$(submit_line --cpus-per-task=8 "feat/x $WT commit=$C1_SHORT shared-cpus=8" 'lattice input.toml')"
out=$("$HISTORY" --job 102)
assert_contains "$out" "EXCLUSIVE: no (--cpus-per-task=8)"

it "--job: commit= の -dirty は印のまま出し、コミットしていない変更で走ったので版を決められないと出す"
record 103 COMPLETED "$DAY"T10:00:00 "$DAY"T10:00:00 "$DAY"T11:00:00 "$WT" \
  "$(submit_line --exclusive "feat/x $WT commit=$C1_SHORT-dirty" 'lattice input.toml')"
out=$("$HISTORY" --job 103)
assert_contains "$out" "COMMIT: $C1_SHORT-dirty"
assert_contains "$out" "VERSION: -"
assert_contains "$out" "REASON: コミットしていない変更で走った"

it "--job: commit= のコミットが本体のリポジトリに無ければ、理由つきで版を決められないと出す"
record 104 COMPLETED "$DAY"T10:00:00 "$DAY"T10:00:00 "$DAY"T11:00:00 "$WT" \
  "$(submit_line --exclusive "feat/x $WT commit=0123456789" 'lattice input.toml')"
out=$("$HISTORY" --job 104)
assert_contains "$out" "VERSION: -"
assert_contains "$out" "REASON: commit= のコミット 0123456789 が本体のリポジトリに無い"

it "--job: commit= もブランチも無ければ（HEAD を切り離して投げた）、理由つきで版を決められないと出す"
record 105 COMPLETED "$DAY"T10:00:00 "$DAY"T10:00:00 "$DAY"T11:00:00 "$WT" \
  "$(submit_line --exclusive "$WT claude=s" 'lattice input.toml')"
out=$("$HISTORY" --job 105)
assert_contains "$out" "BRANCH: -"
assert_contains "$out" "COMMIT: -"
assert_contains "$out" "VERSION: -"
assert_contains "$out" "REASON: commit= もブランチも無い"

it "--job: 改行を含むコマンドを 1 つのジョブとして読み、2 行目以降を COMMAND+ で続ける"
record 106 COMPLETED "$DAY"T10:00:00 "$DAY"T10:00:00 "$DAY"T11:00:00 "$WT" \
  "$(submit_line --exclusive "feat/x $WT commit=$C1_SHORT" $'python3 -c "\n107|x|COMPLETED|print(1)"')"
out=$("$HISTORY" --job 106)
assert_eq 0 $?
assert_contains "$out" 'COMMAND: python3 -c "'
assert_contains "$out" 'COMMAND+ 107|x|COMPLETED|print(1)"'
assert_eq 1 "$(grep -c '^JOB:' <<<"$out")"
assert_contains "$out" "VERSION: $C1"

it "--job: sacct に無いジョブ番号は、番号を添えたエラーで exit 1（ほかの番号は出す）"
out=$("$HISTORY" --job 999 101 2>&1)
assert_eq 1 $?
assert_contains "$out" "ERROR: sacct にジョブ 999 が無い"
assert_contains "$out" "JOB: 101"

it "--job: 番号の形でない引数と、番号の無い呼び出しは exit 1"
out=$("$HISTORY" --job 12a 2>&1)
assert_eq 1 $?
assert_contains "$out" "ERROR"
out=$("$HISTORY" --job 2>&1)
assert_eq 1 $?
assert_contains "$out" "ERROR"

it "--job: commit= のジョブで、投げてから終わるまでにブランチの reflog の項があれば、版を決められないと出す"
setup_repo
echo c >"$WT/input.toml"
at 10:30:00 git -C "$WT" commit -q -am c
out=$("$HISTORY" --job 101)
assert_contains "$out" "VERSION: -"
assert_contains "$out" "REASON: 版を取った時点（${DAY}T10:00:00）から End までに reflog の項がある: ${DAY}T10:30:00"

it "--job: commit= のジョブで、worktree の HEAD の reflog の項（ブランチの切り替え）も見る"
setup_repo
at 10:20:00 git -C "$WT" checkout -q --detach HEAD~1
out=$("$HISTORY" --job 101)
assert_contains "$out" "VERSION: -"
assert_contains "$out" "REASON: 版を取った時点（${DAY}T10:00:00）から End までに reflog の項がある: ${DAY}T10:20:00"

it "--job: commit= のジョブで、ブランチも worktree も消えていれば、commit= を版とし、投げた後の変化は確かめていないと添える"
setup_repo
git -C "$REPO" worktree remove --force "$WT"
git -C "$REPO" branch -q -D feat/x
out=$("$HISTORY" --job 101)
assert_contains "$out" "VERSION: $C1"
assert_contains "$out" "NOTE: 投げた後の変化は確かめていない"

it "--job: worktree が消えていても、worktree-add.sh の名前（<本体>-<ブランチの / を - に>）から本体を見つける"
assert_contains "$out" "REPO: $REPO"

it "--job: --repo で本体のリポジトリを渡せる"
mv "$REPO" "$TEST_ROOT/moved"
out=$("$HISTORY" --job --repo "$TEST_ROOT/moved" 101)
assert_contains "$out" "VERSION: $C1"
out=$("$HISTORY" --job 101)
assert_contains "$out" "REASON: 本体のリポジトリが分からない"

it "--job: commit= の無いジョブは、ブランチの reflog の Start の前の最後の項のコミットを版とする"
setup_repo
echo c >"$WT/input.toml"
at 11:30:00 git -C "$WT" commit -q -am c
record 110 COMPLETED "$DAY"T10:00:00 "$DAY"T10:05:00 "$DAY"T11:00:00 "$WT" \
  "$(submit_line --exclusive "feat/x $WT claude=s" 'lattice input.toml')"
out=$("$HISTORY" --job 110)
assert_contains "$out" "COMMIT: -"
assert_contains "$out" "VERSION: $C1（reflog の feat/x@{${DAY}T09:40:00}）"
assert_contains "$out" "NOTE: コミットしていない変更の有無は確かめていない"

it "--job: commit= の無いジョブで、Start から End までに reflog の項があれば、版を決められないと出す"
record 111 COMPLETED "$DAY"T11:00:00 "$DAY"T11:00:00 "$DAY"T12:00:00 "$WT" \
  "$(submit_line --exclusive "feat/x $WT claude=s" 'lattice input.toml')"
out=$("$HISTORY" --job 111)
assert_contains "$out" "VERSION: -"
assert_contains "$out" "REASON: 版を取った時点（${DAY}T11:00:00）から End までに reflog の項がある: ${DAY}T11:30:00"

it "--job: commit= の無いジョブで、reflog の最も古い項が Start より後なら、版を決められないと出す"
record 112 COMPLETED "$DAY"T08:00:00 "$DAY"T08:00:00 "$DAY"T08:30:00 "$WT" \
  "$(submit_line --exclusive "feat/x $WT claude=s" 'lattice input.toml')"
out=$("$HISTORY" --job 112)
assert_contains "$out" "VERSION: -"
assert_contains "$out" "REASON: reflog が Start まで届かない"

it "--job: commit= の無いジョブで、ブランチが消えていれば、版を決められないと出す"
git -C "$REPO" worktree remove --force "$WT"
git -C "$REPO" branch -q -D feat/x
out=$("$HISTORY" --job 110)
assert_contains "$out" "VERSION: -"
assert_contains "$out" "REASON: ブランチ feat/x か、その reflog が無い"

it "--job: commit= の無いジョブで、Start が 30 日より前なら（外れたコミットの reflog の項は消えうる）、版を決められないと出す"
setup_repo
record 113 COMPLETED "$OLD_DAY"T10:00:00 "$OLD_DAY"T10:00:00 "$OLD_DAY"T11:00:00 "$WT" \
  "$(submit_line --exclusive "feat/x $WT claude=s" 'lattice input.toml')"
out=$("$HISTORY" --job 113)
assert_contains "$out" "VERSION: -"
assert_contains "$out" "REASON: Start が 30 日より前"

it "--job: 連絡先の前に区分と目的の無い古いコメント（連絡先だけ）も読む"
record 114 COMPLETED "$DAY"T10:00:00 "$DAY"T10:00:00 "$DAY"T11:00:00 "$WT" \
  "/nix/store/x-slurm/bin/sbatch --cpus-per-task=8 --wait --export=ALL -J j --time 10 --comment feat/x $WT claude=s shared-cpus=8 --wrap lattice input.toml"
out=$("$HISTORY" --job 114)
assert_contains "$out" "BRANCH: feat/x"
assert_contains "$out" "EXCLUSIVE: no (--cpus-per-task=8)"
assert_contains "$out" "COMMAND: lattice input.toml"

it "--job: 番号を複数渡すと、ジョブごとに空行で区切って出す"
out=$("$HISTORY" --job 101 114)
assert_eq 0 $?
assert_eq 2 "$(grep -c '^JOB:' <<<"$out")"
assert_eq 1 "$(grep -c '^$' <<<"$out")"

it "--job: 目的に「 --wrap 」を含んでも、連絡先とコマンドを区分・参照・目的の後から読む"
setup_repo
record 115 COMPLETED "$DAY"T10:00:00 "$DAY"T10:00:00 "$DAY"T11:00:00 "$WT" \
  "/nix/store/x-slurm/bin/sbatch --exclusive --partition=main --wait -J j --time 10 --comment decision | #1 | --wrap の扱いで方式を決める | feat/x $WT commit=$C1_SHORT --wrap lattice input.toml"
out=$("$HISTORY" --job 115)
assert_contains "$out" "BRANCH: feat/x"
assert_contains "$out" "COMMAND: lattice input.toml"
assert_contains "$out" "VERSION: $C1"

it "--job: commit= の無いジョブで、投げてから Start までに worktree が別のコミットへ移っていれば、版を決められないと出す"
setup_repo
at 10:30:00 git -C "$WT" checkout -q --detach main
record 116 COMPLETED "$DAY"T10:00:00 "$DAY"T11:00:00 "$DAY"T11:30:00 "$WT" \
  "$(submit_line --exclusive "feat/x $WT claude=s" 'lattice input.toml')"
out=$("$HISTORY" --job 116)
assert_contains "$out" "VERSION: -"
assert_contains "$out" "REASON: Start の時点の worktree の HEAD がブランチの版と違う"

it "--job: commit= のジョブで、reflog が投げた時点まで届かなければ（ブランチを作り直した）、投げた後の変化は確かめていないと添える"
setup_repo
git -C "$REPO" worktree remove --force "$WT"
git -C "$REPO" branch -q -D feat/x
at 12:00:00 git -C "$REPO" branch feat/x "$C1"
out=$("$HISTORY" --job 101)
assert_contains "$out" "VERSION: $C1"
assert_contains "$out" "NOTE: 投げた後の変化は確かめていない（reflog が版を取った時点まで届かない）"

it "--job: commit= のジョブで、投げた時点が 30 日より前なら、投げた後の変化は確かめていないと添える"
setup_repo
record 117 COMPLETED "$OLD_DAY"T10:00:00 "$OLD_DAY"T10:00:00 "$OLD_DAY"T11:00:00 "$WT" \
  "$(submit_line --exclusive "feat/x $WT commit=$C1_SHORT" 'lattice input.toml')"
out=$("$HISTORY" --job 117)
assert_contains "$out" "VERSION: $C1"
assert_contains "$out" "NOTE: 投げた後の変化は確かめていない（版を取った時点が 30 日より前"

finish
