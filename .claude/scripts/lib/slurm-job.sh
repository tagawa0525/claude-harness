# shellcheck shell=bash
# slurm-job.sh — sacct のジョブの記録から、排他か、コマンド、走った版を読む（slurm-history.sh --job が source する）
#
# slurm_job_main [--repo <本体のリポジトリ>] <ジョブ番号>...
#   ジョブごとに次の行を出し、ジョブの間を空行で区切る。
#     JOB: <番号> <名前>
#     STATE: / ELAPSED: / TIMES: submit=<Submit> start=<Start> end=<End> / WORKDIR:   sacct の欄のまま
#     EXCLUSIVE: yes | no (--cpus-per-task=N) | unknown (<sbatch の直後の語>)
#     BRANCH: / COMMIT:   SubmitLine の --comment の連絡先のブランチと commit=（-dirty を含む）。無ければ -
#     REPO: <本体のリポジトリ> | -
#     VERSION: <完全な hash> | <hash>（reflog の <ブランチ>@{<時刻>}） | -
#     REASON: <版を決められない理由>   VERSION が - のとき、1 行に 1 つ
#     NOTE: <版を決めたが確かめていないこと>
#     COMMAND: <--wrap の後の 1 行目>、COMMAND+ <2 行目以降>
#   sacct に無い番号は ERROR を出し、ほかの番号を出した後に 1 を返す。
#
# 版の決め方:
#   commit= があれば、そのコミット（本体に無ければ、-dirty なら決めない）。版を取った時点は Submit。
#   無ければ、ブランチの reflog の Start 以前の最後の項のコミット。版を取った時点は Start。
#   版を取った時点から End（走っている間は今）までに、ブランチの reflog か、残っている worktree の HEAD の reflog に、
#   版と違うコミットへ移る項があれば決めない。
# 本体のリポジトリは --repo、残っている WorkDir の git の共通のディレクトリ、worktree-add.sh の名前
# （<本体>-<ブランチの / を - に>）の順に探す。worktree は後片付けで消えるが、reflog は本体の共通の git dir に残る。

# sacct の記録が持つのは、この順の欄。SubmitLine は改行と | を含みうるので最後に置き、区切りに使わない文字で分ける
SLURM_JOB_FIELDS=JobID,JobName,State,Elapsed,Submit,Start,End,WorkDir,SubmitLine
SLURM_JOB_SEP=$'\x1f'
# gc.reflogExpireUnreachable の既定。rebase で外れたコミットの reflog の項はこれを過ぎると黙って消える
SLURM_JOB_REFLOG_DAYS=30
SLURM_JOB_TIME_RE='^[0-9]{4}-[0-9]{2}-[0-9]{2}T[0-9]{2}:[0-9]{2}:[0-9]{2}$'

slurm_job_main() {
  local repo_opt="" ids=() id rc=0 first=1
  while (($# > 0)); do
    case "$1" in
      --repo)
        if (($# < 2)); then
          echo "ERROR: --repo に本体のリポジトリのパスを渡す" >&2
          return 1
        fi
        repo_opt=$2
        shift 2
        ;;
      -*)
        echo "ERROR: 不明なオプション: $1" >&2
        return 1
        ;;
      *)
        ids+=("$1")
        shift
        ;;
    esac
  done
  if ((${#ids[@]} == 0)); then
    echo "ERROR: --job にジョブ番号を 1 つ以上渡す" >&2
    return 1
  fi
  for id in "${ids[@]}"; do
    if [[ ! $id =~ ^[0-9]+$ ]]; then
      echo "ERROR: ジョブ番号の形でない: $id" >&2
      rc=1
      continue
    fi
    ((first)) || echo
    if _slurm_job_one "$id" "$repo_opt"; then
      first=0
    else
      rc=1
    fi
  done
  return "$rc"
}

# _slurm_job_reflog <git の引数…> -- <ref>: reflog を新しい順に「<時刻>\t<hash>\t<件名>」で出す（時刻は sacct と同じ
# ローカル時刻の書式）。版は項を時刻で比べて選ぶ。`git log --before=<時刻>` はコミットの日付で選ぶので rebase や amend の
# 後は走った版でないコミットを返し、`rev-parse '<ブランチ>@{<時刻>}'` は reflog がその時刻まで届かなくても警告だけで
# 別のコミットを返す
_slurm_job_reflog() {
  local args=()
  while [[ $1 != -- ]]; do
    args+=("$1")
    shift
  done
  shift
  git "${args[@]}" log -g --date=format-local:%Y-%m-%dT%H:%M:%S --format='%gd%x09%H%x09%gs' "$1" 2>/dev/null |
    sed -E 's/^[^\t]*@\{([^}]*)\}\t/\1\t/'
}

# _slurm_job_reaches <reflog> <時刻>: reflog の最も古い項がその時刻以前なら 0
_slurm_job_reaches() {
  local oldest
  [[ -n $1 ]] || return 1
  oldest=$(tail -n 1 <<<"$1")
  [[ ! ${oldest%%$'\t'*} > $2 ]]
}

# _slurm_job_moves <reflog> <から> <まで> <版>: その間の、版と違うコミットへ移る項を「<時刻> <件名>」で出す
_slurm_job_moves() {
  local t h s
  while IFS=$'\t' read -r t h s; do
    [[ -n $t && ! $t < $2 && ! $t > $3 && $h != "$4" ]] || continue
    printf '%s %s\n' "$t" "$s"
  done <<<"$1"
}

_slurm_job_one() {
  local id=$1 repo_opt=$2 raw
  if ! raw=$(sacct -j "$id" -X -P -n --delimiter="$SLURM_JOB_SEP" -o "$SLURM_JOB_FIELDS"); then
    echo "ERROR: sacct がジョブ $id を読めなかった" >&2
    return 1
  fi
  if [[ -z $raw ]]; then
    echo "ERROR: sacct にジョブ $id が無い（番号の誤りか、sacct の記録より前のジョブ）" >&2
    return 1
  fi
  local seps=${raw//[^$SLURM_JOB_SEP]/}
  if ((${#seps} != 8)); then
    echo "ERROR: sacct のジョブ $id の記録の欄の数が違う（区切りが ${#seps} 個）" >&2
    return 1
  fi
  local f=() rest=$raw i
  for ((i = 0; i < 8; i++)); do
    f+=("${rest%%"$SLURM_JOB_SEP"*}")
    rest=${rest#*"$SLURM_JOB_SEP"}
  done
  local jobid=${f[0]} name=${f[1]} state=${f[2]} elapsed=${f[3]} submit=${f[4]} start=${f[5]} end=${f[6]} workdir=${f[7]}
  local submitline=$rest
  if [[ $jobid != "$id" ]]; then
    echo "ERROR: sacct がジョブ $id の代わりに $jobid を返した" >&2
    return 1
  fi

  # 排他は sbatch の直後の語で読む（slurm-run.sh は排他のときだけそこに --exclusive を置く。AllocCPUS は -c 32 でも 32）
  local first_arg=- exclusive
  [[ $submitline != *"sbatch "* ]] || first_arg=${submitline#*sbatch }
  first_arg=${first_arg%%[[:space:]]*}
  case "$first_arg" in
    --exclusive) exclusive=yes ;;
    --cpus-per-task=*) exclusive="no ($first_arg)" ;;
    *) exclusive="unknown ($first_arg)" ;;
  esac

  # 連絡先は SubmitLine の --comment から読む（sacct の Comment の欄は空。r995 の Slurm はコメントを記録に残さない）。
  # slurm-run.sh のコメントは「<区分> | <参照> | <目的> | <連絡先>」で、参照と目的は | を含まないが「 --wrap 」を
  # 含みうるので、3 つの「 | 」の後から次の「 --wrap 」までを連絡先とする。区分の無い古いコメントは連絡先だけ
  local command=- comment="" contact="" branch="" commit="" word words=()
  if [[ $submitline == *" --comment "* ]]; then
    comment=${submitline#* --comment }
    if [[ $comment =~ ^(timed|record|decision|trial)\ \|\  ]]; then
      for i in 1 2 3; do comment=${comment#* | }; done
    fi
    contact=${comment%%" --wrap "*}
    [[ $comment != *" --wrap "* ]] || command=${comment#* --wrap }
  elif [[ $submitline == *" --wrap "* ]]; then
    command=${submitline#* --wrap }
  fi
  # <ブランチ> <$PWD> <キー=値>…
  read -r -a words <<<"$contact"
  if ((${#words[@]} > 0)) && [[ ${words[0]} != /* && ${words[0]} != *=* ]]; then
    branch=${words[0]}
  fi
  for word in "${words[@]}"; do
    [[ $word != commit=* ]] || commit=${word#commit=}
  done

  local git_dir="" worktree="" cand dash p base
  if [[ -n $repo_opt ]]; then
    git_dir=$(git -C "$repo_opt" rev-parse --path-format=absolute --git-common-dir 2>/dev/null) || git_dir=""
  elif [[ -d $workdir ]] && git -C "$workdir" rev-parse --git-dir >/dev/null 2>&1; then
    git_dir=$(git -C "$workdir" rev-parse --path-format=absolute --git-common-dir)
  elif [[ -n $branch ]]; then
    dash=${branch//\//-}
    p=$workdir
    while [[ $p == /?* ]]; do
      base=${p##*/}
      if [[ $base == ?*-"$dash" ]]; then
        cand="${p%/*}/${base%-"$dash"}"
        if git -C "$cand" rev-parse --git-dir >/dev/null 2>&1; then
          git_dir=$(git -C "$cand" rev-parse --path-format=absolute --git-common-dir)
          break
        fi
      fi
      p=${p%/*}
    done
  fi
  # 同じパスに後から別のリポジトリが置かれていれば、その reflog はこのジョブのものでない
  if [[ -n $git_dir && -d $workdir ]] &&
    [[ $(git -C "$workdir" rev-parse --path-format=absolute --git-common-dir 2>/dev/null) == "$git_dir" ]]; then
    worktree=$(git -C "$workdir" rev-parse --show-toplevel)
  fi
  # rev-parse --show-toplevel は --git-dir を渡しても cwd の作業ツリーを返すので、共通の git dir の名前から読む
  local repo=${git_dir:--}
  [[ $repo != */.git ]] || repo=${repo%/.git}

  local reasons=() notes=() version="" version_at="" since="" until=$end branch_log="" head_log="" moves line
  [[ $until =~ $SLURM_JOB_TIME_RE ]] || until=$(date +%Y-%m-%dT%H:%M:%S)
  local cutoff oldest
  cutoff=$(date -d "$SLURM_JOB_REFLOG_DAYS days ago" +%Y-%m-%dT%H:%M:%S)
  if [[ -n $git_dir && -n $branch ]] && git --git-dir="$git_dir" reflog exists "refs/heads/$branch"; then
    branch_log=$(_slurm_job_reflog --git-dir="$git_dir" -- "refs/heads/$branch")
  fi
  [[ -z $worktree ]] || head_log=$(_slurm_job_reflog -C "$worktree" -- HEAD)

  if [[ -n $commit ]]; then
    since=$submit
    [[ $commit != *-dirty ]] || reasons+=("コミットしていない変更で走った（commit= の -dirty）")
    if [[ -z $git_dir ]]; then
      reasons+=("本体のリポジトリが分からない（WorkDir が消えた。--repo で渡す）")
    elif ! version=$(git --git-dir="$git_dir" rev-parse -q --verify "${commit%-dirty}^{commit}"); then
      reasons+=("commit= のコミット ${commit%-dirty} が本体のリポジトリに無い（rebase で外れ、gc で消えた）")
      version=""
    fi
    if [[ -z $branch_log && -z $head_log ]]; then
      notes+=("投げた後の変化は確かめていない（ブランチと worktree の reflog が無い）")
    elif [[ $since < $cutoff ]]; then
      notes+=("投げた後の変化は確かめていない（版を取った時点が $SLURM_JOB_REFLOG_DAYS 日より前で、rebase で外れたコミットの reflog の項は黙って消えうる）")
    elif ! _slurm_job_reaches "$branch_log" "$since" && ! _slurm_job_reaches "$head_log" "$since"; then
      notes+=("投げた後の変化は確かめていない（reflog が版を取った時点まで届かない）")
    else
      notes+=("投げた後のコミットしない編集は確かめていない（reflog はコミットしない編集を記録せず、-dirty は投げた時点しか示さない）")
    fi
  elif [[ -z $branch ]]; then
    reasons+=("commit= もブランチも無い（HEAD を切り離して投げたか、git の外から投げた）")
  elif [[ -z $git_dir ]]; then
    reasons+=("本体のリポジトリが分からない（WorkDir が消えた。--repo で渡す）")
  elif [[ -z $branch_log ]]; then
    reasons+=("ブランチ $branch か、その reflog が無い（消えた）")
  elif [[ ! $start =~ $SLURM_JOB_TIME_RE ]]; then
    reasons+=("Start が無い（走り始めていない）")
  else
    since=$start
    if [[ $start < $cutoff ]]; then
      reasons+=("Start が $SLURM_JOB_REFLOG_DAYS 日より前（rebase で外れたコミットの reflog の項は黙って消えうる）")
    fi
    local t h subject
    oldest=$(tail -n 1 <<<"$branch_log")
    oldest=${oldest%%$'\t'*}
    if [[ $oldest > $start ]]; then
      reasons+=("reflog が Start まで届かない（最も古い項は $oldest）")
    else
      while IFS=$'\t' read -r t h _; do
        if [[ ! $t > $start ]]; then
          version=$h
          version_at=$t
          break
        fi
      done <<<"$branch_log"
      # ブランチの版でも、投げてから Start までに worktree が別のコミットへ移っていれば、ジョブはそちらを読む
      while IFS=$'\t' read -r t h subject; do
        if [[ -n $t && ! $t > $start ]]; then
          [[ $h == "$version" ]] || reasons+=("Start の時点の worktree の HEAD がブランチの版と違う（$t $subject）")
          break
        fi
      done <<<"$head_log"
    fi
    notes+=("コミットしていない変更の有無は確かめていない（nix を通らないジョブには dirty の印が出ない）")
  fi

  if [[ -n $version && -n $since ]]; then
    moves=$(
      _slurm_job_moves "$branch_log" "$since" "$until" "$version"
      _slurm_job_moves "$head_log" "$since" "$until" "$version"
    )
    while IFS= read -r line; do
      [[ -z $line ]] || reasons+=("版を取った時点（$since）から End までに reflog の項がある: $line")
    done < <(sort -u <<<"$moves")
  fi

  echo "JOB: $jobid $name"
  echo "STATE: $state"
  echo "ELAPSED: $elapsed"
  echo "TIMES: submit=$submit start=$start end=$end"
  echo "EXCLUSIVE: $exclusive"
  echo "WORKDIR: $workdir"
  echo "BRANCH: ${branch:--}"
  echo "COMMIT: ${commit:--}"
  echo "REPO: $repo"
  if ((${#reasons[@]} > 0)); then
    echo "VERSION: -"
    printf 'REASON: %s\n' "${reasons[@]}"
  else
    if [[ -n $commit ]]; then
      echo "VERSION: $version"
    else
      echo "VERSION: $version（reflog の $branch@{$version_at}）"
    fi
    printf 'NOTE: %s\n' "${notes[@]}"
  fi
  local n=0
  while IFS= read -r line; do
    if ((n++ == 0)); then
      echo "COMMAND: $line"
    else
      echo "COMMAND+ $line"
    fi
  done <<<"$command"
}
