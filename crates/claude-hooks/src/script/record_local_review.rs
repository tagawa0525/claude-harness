//! `record-local-review.sh <PR の URL> <SHA> <high> <medium> <low>`: ローカルの /code-review の結果を、PR の head の
//! 記録としてコメントに残す（/gh-pr-review の Step 6）。SHA は /code-review を回したコミットで、PR の head と一致
//! しなければ記録しない（レビューの後に head が動いたなら、その head はレビューしていない）。件数は、その head で
//! 確かめて正しいと判断した指摘の数で、後に Copilot が出した数と比べるために残す。
//!
//! 記録はマージの hook（pre-merge-check）が Copilot のレビューの代わりに数えるので、差分を読んだだけのコミットは
//! 記録しない。記録を数えるのは書き込める人のコメントだけなので、push できないリポジトリ（上流の PR）には付けない。
//!
//! その head で /code-review を呼んだことは、count-review-rounds の hook が clone に書く行（呼んだときの HEAD）で
//! 確かめ、行が無ければ記録しない。確かめる clone は起動したディレクトリのもの。直した後の head と rebase の後の head に、
//! 回さずに前の周の数で記録を付けると、マージの hook の「最後の push を覆うレビューがありません」の知らせが出なくなる。
//! hook が行を書かない回し方（対象に PR の URL を渡した、detached HEAD の worktree、ユーザーが打った /code-review）は、
//! `--unlogged <理由>` で理由を記録のコメントに残して付ける。前の記録が覆う head（rebase だけの head）かは、ここでは
//! 確かめず、拒否の文で decide-next.sh を示す（比べた案は claude-harness の issue 281）

use super::pr_url;
use regex::Regex;
use std::sync::LazyLock;

const USAGE: &str = "Usage: record-local-review.sh <PR の URL> <レビューした SHA> <high> <medium> <low> [--unlogged <回した記録が clone に無い理由>]";

static SHA: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^[0-9a-f]{7,40}$").unwrap());
/// 読む側は `high=0 ` の字面で比べるので、先頭の 0 を許さない
static COUNT: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^(0|[1-9][0-9]*)$").unwrap());

pub fn main(args: &[String]) -> i32 {
    let [url, reviewed, counts @ ..] = args else {
        eprintln!("{USAGE}");
        return 1;
    };
    let (counts, unlogged) = match counts {
        [h, m, l] => ([h, m, l], None),
        [h, m, l, flag, reason] if flag == "--unlogged" && !reason.trim().is_empty() => {
            ([h, m, l], Some(reason.trim()))
        }
        _ => {
            eprintln!("{USAGE}");
            return 1;
        }
    };
    let Some(pr) = pr_url::or_usage(pr_url::parse(url), USAGE) else {
        return 1;
    };
    if !SHA.is_match(reviewed) {
        eprintln!("{USAGE}（SHA は 7〜40 桁の 16 進: '{reviewed}'）");
        return 1;
    }
    let mut n = [0u64; 3];
    for (slot, count) in n.iter_mut().zip(counts) {
        match COUNT.is_match(count).then(|| count.parse().ok()).flatten() {
            Some(value) => *slot = value,
            None => {
                eprintln!("{USAGE}（件数は 0 以上の整数で、先頭に 0 を付けない: '{count}'）");
                return 1;
            }
        }
    }
    let [high, medium, low] = n;

    let head = match crate::gh::output_for_repo(
        &pr.repo,
        &[
            "pr",
            "view",
            &pr.number,
            "--json",
            "headRefOid",
            "-q",
            ".headRefOid",
        ],
    ) {
        Ok(out) => String::from_utf8_lossy(&out)
            .trim_end_matches('\n')
            .to_string(),
        Err(_) => String::new(),
    };
    // 空の SHA は、記録の無い出力の空行と一致して、ローカルレビュー済みに見える
    if head.is_empty() {
        eprintln!(
            "ERROR: PR #{} の head を取得できませんでした（記録を付けていません）",
            pr.number
        );
        return 1;
    }
    let Ok(can_push) = crate::gh::output_for_repo(
        &pr.repo,
        &["api", "repos/{owner}/{repo}", "--jq", ".permissions.push"],
    ) else {
        eprintln!(
            "ERROR: {} の権限を取得できませんでした（記録を付けていません）",
            pr.repo
        );
        return 1;
    };
    if String::from_utf8_lossy(&can_push).trim_end_matches('\n') != "true" {
        eprintln!(
            "ERROR: {} に push できないので、記録を付けても数えられません（記録を付けていません）",
            pr.repo
        );
        return 1;
    }
    if !head.starts_with(reviewed.as_str()) {
        eprintln!(
            "ERROR: レビューした {} は PR #{} の head（{}）ではありません。head を push してからレビューし直してください（記録を付けていません）",
            short(reviewed),
            pr.number,
            short(&head)
        );
        return 1;
    }
    // 今のディレクトリを取れないときは、clone を引けないときと同じに扱う（--unlogged の記録は cwd に頼らない）。取れない
    // 理由は拒否の文に出す
    let cwd = std::env::current_dir();
    let rounds = cwd
        .as_deref()
        .ok()
        .and_then(|dir| crate::rules::round_ran_at(dir, &head));
    let logged = matches!(rounds, Some((_, true)));
    if !logged && unlogged.is_none() {
        let place = match (&rounds, &cwd) {
            (Some((dir, _)), _) => format!(
                "{} で /code-review を回した記録が {} にありません",
                short(&head),
                dir.display()
            ),
            (None, Ok(dir)) => format!(
                "{} で git の共通のディレクトリを引けないので、{} で /code-review を回した記録を確かめられません",
                dir.display(),
                short(&head)
            ),
            (None, Err(e)) => format!(
                "今のディレクトリを取れないので（{e}）、{} で /code-review を回した記録を確かめられません",
                short(&head)
            ),
        };
        let canonical = format!("https://github.com/{}/pull/{}", pr.repo, pr.number);
        let scripts = super::claude_root()
            .join("skills/gh-pr-review/scripts")
            .display()
            .to_string()
            .replace('\\', "/");
        // 回した人が先に読む順に並べる。rebase の案内を先に置くと、rebase だけの head を回し直して見つけた High を、
        // 記録しないまま進ませる
        eprintln!(
            "ERROR: {place}（記録を付けていません）\n\
             - この head をまだ回していないなら、PR の worktree で /code-review を回してから、その worktree で記録する\n\
             - この head で回したのに hook が行を書かない回し方だったなら（対象に PR の URL を渡した、detached HEAD の worktree、ユーザーが打った /code-review）、末尾に --unlogged '<その回し方>' を付ける（記録のコメントに残る）\n\
             - 回し直していない、base への rebase だけの head なら、記録を足さない（前の head の記録で足りるかは、PR の worktree で {scripts}/decide-next.sh {canonical} を実行すると LOCAL_REVIEWED: の行に出る）"
        );
        return 1;
    }
    let mut body = format!(
        "{}\nローカルの /code-review で {} を確かめた: High {high} / Medium {medium} / Low {low}",
        crate::rules::local_review_mark(&head, high, medium, low),
        short(&head)
    );
    if let Some(reason) = unlogged.filter(|_| !logged) {
        body.push_str(&format!("\n回した記録を確かめていない理由: {reason}"));
    }
    let code = crate::gh::run_for_repo(&pr.repo, &["pr", "comment", &pr.number, "--body", &body]);
    if code != 0 {
        return code;
    }
    println!("RECORDED: {head}");
    if high > 0 {
        println!(
            "NOTE: High の残る記録は、レビュー済みとして数えない。直して push し、回し直して記録する"
        );
    }
    0
}

fn short(sha: &str) -> &str {
    sha.get(..7).unwrap_or(sha)
}
