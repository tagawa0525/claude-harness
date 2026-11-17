//! `base-moved.sh [branch] [--base BASE]`: 比較元が分岐点から進み、ブランチと同じパスを触っていないかを知らせる
//! （/gh-pr-create の「比較元の進みの確認」）。branch を省けば今のブランチ、BASE を省けば origin の既定のブランチ
//! （origin/HEAD → origin/main → origin/master）。積み重なったブランチは土台を origin/<土台のブランチ> で渡す（fetch が
//! 更新するのは origin/* だけで、ローカルのブランチは古いまま比べることになる。GitHub の無い clone では origin/<土台> は
//! fetch されない追跡ブランチなので、ローカルのブランチ名で渡す）。
//!
//! GitHub のリモートと origin が揃う clone では、比べる前に origin だけを fetch する（失敗したら WARN を出し、手元の追跡
//! ブランチで比べる。--base が origin 以外のリモートの追跡ブランチなら fetch せず WARN を出す）。揃わない clone では、--base を渡されたときだけ fetch せずに比べ
//! （リモートの無いリポジトリでは --base main を渡す）、--base がローカルのブランチでなければ WARN を出す。渡されなければ比べずに
//! UNVERIFIED を出す。ほかのセッションが同じファイルを main に入れていると、マージの
//! 直前の rebase まで気付けない。知らせるだけで失敗にはしない（同じパスを触っても、衝突するとは限らない）。
//!
//! 出力:
//! - BASE が分岐点から進み、進んだ変更とブランチの変更が同じパスを触るときだけ:
//!   - `BASE_MOVED: <BASE> が分岐点から <N> commits 進んでいる（先頭 <hash>、分岐点 <hash>）`（マージを 1 件と数える）
//!   - `COMMON: <両方が触ったパス>`（パスごとに 1 行）
//!   - `NEXT: git diff <分岐点>..<先頭> -- <パス>… で …`（読む差分と、読んだ後の進め方）
//! - 比較が済んだら `BEHIND: <N>`: ブランチにまだ入っていない BASE のコミットの数（マージで入ったコミットも数える。
//!   rebase が要るかの目安）
//! - 比較が済んでいなければ（WARN を出した、比較元が決まらない）、BEHIND の代わりに `UNVERIFIED: …`（重なりと件数の
//!   どちらも未確認。修正方法を添える）。ERROR（終了コード 1）のときはどちらも出さない

use crate::git::git;
use std::collections::BTreeSet;
use std::path::Path;

/// WARN を出した後の行（BEHIND の代わり）
const UNVERIFIED: &str = "UNVERIFIED: 重なりと遅れの件数は未確認（WARN: の原因を直して再実行するか、未確認のまま進めると PR 本文（/delegate の担当は報告）に書く）";

/// 比較元が決まらず比べなかったときの行
const NO_BASE: &str = "UNVERIFIED: 比較元が決まらないので比べていない（GitHub の origin か、その既定のブランチが無い）。--base <比較元> を渡して再実行する";

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
    // WARN を出したら、比べても BEHIND の代わりに UNVERIFIED を出す（手元の追跡ブランチが古いかもしれない）
    let mut warned = false;
    let github_origin =
        crate::git::has_github_remote(dir) && git(dir, &["remote", "get-url", "origin"]).is_some();
    // --base の完全な名前（refs/heads/…、refs/remotes/<リモート>/…）。origin/main~1 や @{u} のような書き方は、
    // 追跡ブランチかを字面から決められず空になる
    let base_ref = base.as_deref().and_then(|b| {
        git(dir, &["rev-parse", "--symbolic-full-name", b]).filter(|r| !r.is_empty())
    });
    let base_ref = base_ref.as_deref().unwrap_or("");
    if github_origin {
        // origin を明示する。引数なしの fetch は今のブランチの上流のリモートへ行き、上流が別のリモートだと、BASE の
        // origin/* が更新されないまま古い比較になる
        let fetched = crate::git::run(dir, &["fetch", "-q", "origin"], None).is_some_and(|out| {
            eprint!("{}", String::from_utf8_lossy(&out.stderr));
            out.status.success()
        });
        if !fetched {
            warned = true;
            eprintln!(
                "WARN: fetch に失敗しました。比較は未確認です（手元の追跡ブランチは古い可能性があります）"
            );
        }
        // origin 以外のリモートは fetch しない（ADR-0038: 比べる先は origin の既定のブランチに限る）。その追跡
        // ブランチが指すと分かるときは、古いまま比べるので未確認と知らせる（origin/main~1 のような書き方は決められない）
        if let Some(base) = &base
            && base_ref.starts_with("refs/remotes/")
            && !base_ref.starts_with("refs/remotes/origin/")
        {
            warned = true;
            eprintln!(
                "WARN: {base} は origin 以外のリモートの追跡ブランチで、fetch していません。比較は未確認です（fetch するのは origin だけです）"
            );
        }
    } else if let Some(base) = &base {
        // GitHub でないリモートは fetch しない。ローカルのブランチでないもの（追跡ブランチ、@{u} や origin/main~1 の
        // ような書き方）は、fetch していない追跡ブランチを指しうるので未確認と知らせる（書き方から追跡ブランチかを確実には
        // 決められない）
        if !base_ref.starts_with("refs/heads/") {
            warned = true;
            eprintln!(
                "WARN: {base} はローカルのブランチでなく、fetch していません。比較は未確認です（GitHub の origin だけを fetch します）"
            );
        }
    } else {
        // 比べる先の既定は GitHub の origin の既定のブランチなので、GitHub のリモートか origin が無ければ決まらない
        println!("{NO_BASE}");
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
                println!("{NO_BASE}");
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
        println!("{UNVERIFIED}");
        return 0;
    };
    match out.status.code() {
        Some(0) => {}
        Some(1) => {
            eprintln!(
                "WARN: {base} と {target} の分岐点が見つかりません（履歴が共通でない、または shallow clone）。比較は未確認です"
            );
            println!("{UNVERIFIED}");
            return 0;
        }
        code => {
            eprintln!(
                "WARN: git merge-base が失敗しました（終了コード {}）。比較は未確認です: {}",
                code.map_or("なし".to_string(), |c| c.to_string()),
                String::from_utf8_lossy(&out.stderr).trim_end()
            );
            println!("{UNVERIFIED}");
            return 0;
        }
    }
    let fork = String::from_utf8_lossy(&out.stdout).trim().to_string();
    // 遅れはマージで入ったコミットも数える（rebase で取り込む数。BASE_MOVED の件数とは数え方が違う）
    let Some(behind) = git(dir, &["rev-list", "--count", &format!("{target}..{base}")])
        .and_then(|n| n.trim().parse::<u64>().ok())
    else {
        eprintln!("WARN: git rev-list が失敗しました。比較は未確認です");
        println!("{UNVERIFIED}");
        return 0;
    };
    let finish = |warned: bool| {
        if warned {
            println!("{UNVERIFIED}");
        } else {
            println!("BEHIND: {behind}");
        }
        0
    };
    // マージコミットで入る PR は 1 件と数える（--first-parent）。数えられなければ、進みなしと読まれないよう未確認にする
    let Some(moved) = git(
        dir,
        &[
            "rev-list",
            "--first-parent",
            "--count",
            &format!("{fork}..{base}"),
        ],
    )
    .and_then(|n| n.trim().parse::<u64>().ok()) else {
        eprintln!("WARN: git rev-list が失敗しました。比較は未確認です");
        println!("{UNVERIFIED}");
        return 0;
    };
    if moved == 0 {
        return finish(warned);
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
        println!("{UNVERIFIED}");
        return 0;
    };
    let common: Vec<&String> = base_files.intersection(&branch_files).collect();
    if common.is_empty() {
        return finish(warned);
    }
    let short = |r: &str| git(dir, &["rev-parse", "--short", r]).unwrap_or_default();
    let (head, fork) = (short(&base), short(&fork));
    println!(
        "BASE_MOVED: {base} が分岐点から {moved} commits 進んでいる（先頭 {head}、分岐点 {fork}）"
    );
    for path in &common {
        println!("COMMON: {path}");
    }
    let paths: Vec<String> = common.iter().map(|p| super::wait_step::quote(p)).collect();
    println!(
        "NEXT: git diff {fork}..{head} -- {} で {base} の変更を読む。同じ変更の二重なら作業を止め、衝突しうるなら PR 本文の Summary（/delegate の担当は報告）に書いて進める",
        paths.join(" ")
    );
    finish(warned)
}
