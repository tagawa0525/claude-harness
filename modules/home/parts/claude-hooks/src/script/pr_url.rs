//! スクリプトが対象の PR を URL で受けるための解析。cwd のリポジトリに依らずその PR を読み書きするため、gh には
//! URL のリポジトリを渡す（[`crate::gh::run_for_repo`]）。番号だけだと cwd のリポジトリの PR として読まれ、cd の付け忘れで別の
//! リポジトリの PR を読み書きする（ADR-0032）

use regex::Regex;
use std::sync::LazyLock;

/// owner と repo は GitHub の名前に使える文字だけを受ける。案内のコマンド（`-R` など）にそのまま書くので、`;` や空白を
/// 通さない。PR の下のページ（`/files`）と通知のクエリの付いた URL も、同じ PR を指すので受ける
static PR_URL: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
        r"^https://github\.com/([A-Za-z0-9._-]+)/([A-Za-z0-9._-]+)/pull/([0-9]+)(/[^?#]*)?(\?[^#]*)?(#(.*))?$",
    )
    .unwrap()
});

pub struct Pr {
    /// `<owner>/<repo>`
    pub repo: String,
    pub number: String,
    /// `#` 以降（`#` を除く）。`#` が無ければ None
    pub fragment: Option<String>,
}

/// `https://github.com/<owner>/<repo>/pull/<番号>[/<ページ>][?<クエリ>]` を解析する。`#` の付いた URL は拒む
/// （コメントの URL を渡した呼び出しが、別に渡す comment_id と食い違っても気付けないため）。受けられなければ、
/// 標準エラーに出す理由を返す
pub fn parse(url: &str) -> Result<Pr, String> {
    let pr = parse_allowing_fragment(url)?;
    if pr.fragment.is_some() {
        return Err(format!(
            "ERROR: PR の URL に # 以降を付けないでください（コメントは別の引数で渡す）: '{url}'"
        ));
    }
    Ok(pr)
}

/// [`parse`] と同じだが、`#` 以降の付いた URL（コメントの URL）も受け、`#` 以降を [`Pr::fragment`] に入れる
/// （sh の `parse_pr_url --allow-fragment`）
pub fn parse_allowing_fragment(url: &str) -> Result<Pr, String> {
    let not_a_pr_url = || {
        format!(
            "ERROR: 対象は PR の URL（https://github.com/<owner>/<repo>/pull/<番号>）で渡してください: '{url}'"
        )
    };
    let Some(caps) = PR_URL.captures(url) else {
        return Err(not_a_pr_url());
    };
    // `.` と `..` は gh api の {owner}/{repo} に入り、API のパスを上に辿る（# の検査より先なのは sh と同じ）
    if [&caps[1], &caps[2]]
        .iter()
        .any(|name| *name == "." || *name == "..")
    {
        return Err(not_a_pr_url());
    }
    Ok(Pr {
        repo: format!("{}/{}", &caps[1], &caps[2]),
        number: caps[3].to_string(),
        fragment: caps.get(7).map(|m| m.as_str().to_string()),
    })
}

/// 解析の結果を受け、受けられなければ理由とスクリプトの使い方の文を標準エラーに出して None を返す
pub fn or_usage(parsed: Result<Pr, String>, usage: &str) -> Option<Pr> {
    match parsed {
        Ok(pr) => Some(pr),
        Err(why) => {
            eprintln!("{why}");
            eprintln!("{usage}");
            None
        }
    }
}
