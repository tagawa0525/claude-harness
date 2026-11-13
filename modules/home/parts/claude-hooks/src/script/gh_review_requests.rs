//! `gh-review-requests.sh <PR の URL>`: PR の Copilot 宛てのレビューの要求（review_requested のイベント）の
//! created_at（ISO 8601）を、古い順に 1 行ずつ出す。要求が無ければ何も出さずに 0、取得に失敗したら gh の終了コード。
//! 「レビューを待つ」「何周目か」「再レビューが登録されたか」はどれもこの列から決まるので、取得を 1 か所にまとめる

use super::pr_url;

const USAGE: &str = "Usage: gh-review-requests.sh <PR の URL>";

/// レビュアーのログインは API により Copilot と copilot-pull-request-reviewer[bot] に揺れるので、Bot の種別で login に
/// copilot を含むものを数える
const COPILOT_REQUESTS: &str = r#".[] | select(.event == "review_requested"
                     and (.requested_reviewer.type? // "") == "Bot"
                     and ((.requested_reviewer.login? // "") | ascii_downcase | test("copilot")))
           | .created_at"#;

pub fn main(args: &[String]) -> i32 {
    let Some(pr) = pr_url::or_usage(
        pr_url::parse(args.first().map(String::as_str).unwrap_or("")),
        USAGE,
    ) else {
        return 1;
    };
    if args.len() != 1 {
        eprintln!("{USAGE}");
        return 1;
    }
    crate::gh::run_for_repo(
        &pr.repo,
        &[
            "api",
            "--paginate",
            &format!(
                "repos/{{owner}}/{{repo}}/issues/{}/timeline?per_page=100",
                pr.number
            ),
            "--jq",
            COPILOT_REQUESTS,
        ],
    )
}
