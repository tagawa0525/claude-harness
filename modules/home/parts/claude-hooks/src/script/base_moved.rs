//! `base-moved.sh [branch] [--base BASE]`: 比較元が分岐点から進み、ブランチと同じパスを触っていないかを知らせる
//! （/gh-pr-create の「比較元の進みの確認」）。branch を省けば今のブランチ、BASE を省けば origin の既定のブランチ
//! （origin/HEAD → origin/main → origin/master）。積み重なったブランチは土台を origin/<土台のブランチ> で渡す（fetch が
//! 更新するのは origin/* だけで、ローカルのブランチは古いまま比べることになる）。
//!
//! GitHub のリモートと origin が揃う clone では、比べる前に origin を fetch する（失敗したら WARN を出し、手元の追跡
//! ブランチで比べる）。揃わない clone では、--base を渡されたときだけ fetch せずに比べ（リモートの無いリポジトリでは
//! --base main を渡す）、--base がローカルのブランチでなければ WARN を出す。渡されなければ何もしない。WARN が出たときは
//! 比較が済んでいない（出力が空でも「進みなし」ではない）。ほかのセッションが同じファイルを main に入れていると、マージの
//! 直前の rebase まで気付けない。知らせるだけで失敗にはしない（同じパスを触っても、衝突するとは限らない）。
//!
//! 出力（BASE が分岐点から進み、進んだ変更とブランチの変更が同じパスを触るときだけ。それ以外は何も出さない）:
//! - `BASE_MOVED: <BASE> が分岐点から <N> commits 進んでいる（先頭 <hash>、分岐点 <hash>）`
//! - `COMMON: <両方が触ったパス>`（パスごとに 1 行）

use crate::git::git;
use std::collections::BTreeSet;
use std::path::Path;

pub fn main(args: &[String]) -> i32 {
    let mut base: Option<String> = None;
    let mut target: Option<String> = None;
    let mut rest = args.iter();
    while let Some(arg) = rest.next() {
        match arg.as_str() {
            "--base" => match rest.next().filter(|v| !v.is_empty()) {
                Some(v) => base = Some(v.clone()),
                None => {
                    eprintln!("ERROR: --base には比較元が必要です");
                    return 1;
                }
            },
            a if a.starts_with('-') => {
                eprintln!("ERROR: 不明なオプション: {a}");
                return 1;
            }
            a => target = Some(a.to_string()),
        }
    }
    let dir = Path::new(".");
    let github_origin =
        crate::git::has_github_remote(dir) && git(dir, &["remote", "get-url", "origin"]).is_some();
    if github_origin {
        // origin を明示する。引数なしの fetch は今のブランチの上流のリモートへ行き、上流が別のリモートだと、BASE の
        // origin/* が更新されないまま古い比較になる
        let fetched = crate::git::run(dir, &["fetch", "-q", "origin"], None).is_some_and(|out| {
            eprint!("{}", String::from_utf8_lossy(&out.stderr));
            out.status.success()
        });
        if !fetched {
            eprintln!(
                "WARN: fetch に失敗しました。比較は未確認です（手元の追跡ブランチは古い可能性があります）"
            );
        }
    } else if let Some(base) = &base {
        // GitHub でないリモートは fetch しない。ローカルのブランチでないもの（追跡ブランチ、@{u} や origin/main~1 の
        // ような書き方）は、fetch していない追跡ブランチを指しうるので未確認と知らせる（書き方から追跡ブランチかを確実には
        // 決められない）
        if git(
            dir,
            &[
                "show-ref",
                "--verify",
                "--quiet",
                &format!("refs/heads/{base}"),
            ],
        )
        .is_none()
        {
            eprintln!(
                "WARN: {base} はローカルのブランチでなく、fetch していません。比較は未確認です（GitHub の origin だけを fetch します）"
            );
        }
    } else {
        // 比べる先の既定は GitHub の origin の既定のブランチなので、GitHub のリモートか origin が無ければ決まらない
        return 0;
    }

    let base = match base {
        Some(base) => base,
        None => {
            let found = git(
                dir,
                &[
                    "symbolic-ref",
                    "--quiet",
                    "--short",
                    "refs/remotes/origin/HEAD",
                ],
            )
            .or_else(|| {
                ["origin/main", "origin/master"]
                    .into_iter()
                    .find(|r| {
                        git(
                            dir,
                            &["rev-parse", "--verify", "-q", &format!("refs/remotes/{r}")],
                        )
                        .is_some()
                    })
                    .map(str::to_string)
            });
            let Some(found) = found else {
                return 0;
            };
            found
        }
    };
    let target = target.unwrap_or_else(|| crate::git::current_branch(dir));
    if target.is_empty() {
        eprintln!("ERROR: ブランチ名を渡してください（HEAD がブランチではありません）");
        return 1;
    }
    for r in [&base, &target] {
        if git(
            dir,
            &["rev-parse", "--verify", "-q", &format!("{r}^{{commit}}")],
        )
        .is_none()
        {
            eprintln!("ERROR: {r} を解決できません");
            return 1;
        }
    }

    // merge-base は共通の祖先が無いとき 1、それ以外の失敗（壊れたリポジトリなど）はほかの終了コード
    let Some(out) = crate::git::run(dir, &["merge-base", &base, &target], None) else {
        eprintln!("WARN: git merge-base が失敗しました（起動できません）。比較は未確認です");
        return 0;
    };
    match out.status.code() {
        Some(0) => {}
        Some(1) => {
            eprintln!(
                "WARN: {base} と {target} の分岐点が見つかりません（履歴が共通でない、または shallow clone）。比較は未確認です"
            );
            return 0;
        }
        code => {
            eprintln!(
                "WARN: git merge-base が失敗しました（終了コード {}）。比較は未確認です: {}",
                code.map_or("なし".to_string(), |c| c.to_string()),
                String::from_utf8_lossy(&out.stderr).trim_end()
            );
            return 0;
        }
    }
    let fork = String::from_utf8_lossy(&out.stdout).trim().to_string();
    // マージコミットで入る PR は 1 件と数える（--first-parent）
    let moved: u64 = git(
        dir,
        &[
            "rev-list",
            "--first-parent",
            "--count",
            &format!("{fork}..{base}"),
        ],
    )
    .and_then(|n| n.trim().parse().ok())
    .unwrap_or(0);
    if moved == 0 {
        return 0;
    }
    // 移動は前のパスも数える（--no-renames）。失敗を空の出力（進みなし）と区別する
    let changed = |range: String| {
        git(
            dir,
            &[
                "-c",
                "core.quotePath=false",
                "diff",
                "--no-renames",
                "--name-only",
                &range,
            ],
        )
        .map(|s| {
            s.lines()
                .filter(|l| !l.is_empty())
                .map(str::to_string)
                .collect::<BTreeSet<_>>()
        })
    };
    let (Some(base_files), Some(branch_files)) = (
        changed(format!("{fork}..{base}")),
        changed(format!("{fork}..{target}")),
    ) else {
        eprintln!("WARN: git diff が失敗しました。比較は未確認です");
        return 0;
    };
    let common: Vec<&String> = base_files.intersection(&branch_files).collect();
    if common.is_empty() {
        return 0;
    }
    let short = |r: &str| git(dir, &["rev-parse", "--short", r]).unwrap_or_default();
    println!(
        "BASE_MOVED: {base} が分岐点から {moved} commits 進んでいる（先頭 {}、分岐点 {}）",
        short(&base),
        short(&fork)
    );
    for path in common {
        println!("COMMON: {path}");
    }
    0
}
