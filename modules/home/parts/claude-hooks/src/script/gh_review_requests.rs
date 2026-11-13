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
    let args = args_for(&pr.number);
    crate::gh::run_for_repo(&pr.repo, &args.each_ref().map(String::as_str))
}

/// PR の Copilot 宛てのレビューの要求の時刻（古い順）。取得に失敗すれば gh の終了コード（gh の標準エラーは引き継ぐ）
pub fn request_times(pr: &pr_url::Pr) -> Result<Vec<String>, i32> {
    let args = args_for(&pr.number);
    let out = crate::gh::output_for_repo(&pr.repo, &args.each_ref().map(String::as_str))?;
    Ok(String::from_utf8_lossy(&out)
        .lines()
        .filter(|l| !l.is_empty())
        .map(str::to_string)
        .collect())
}

fn args_for(number: &str) -> [String; 5] {
    [
        "api".to_string(),
        "--paginate".to_string(),
        format!("repos/{{owner}}/{{repo}}/issues/{number}/timeline?per_page=100"),
        "--jq".to_string(),
        COPILOT_REQUESTS.to_string(),
    ]
}
