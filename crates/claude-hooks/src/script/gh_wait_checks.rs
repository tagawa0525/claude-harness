//! `gh-wait-checks.sh <PR の URL> [<SHA>]`: PR の head の check（check run とコミットの status）が登録され、全部が
//! 終わるまで、間隔を延ばしながら待つ。見るものはマージの hook（pre-merge-check）と同じ（実行中の check があれば
//! マージは止まる）。
//!
//! `gh pr checks --watch` でなくこれを使うのは、push の直後は check がまだ登録されておらず、--watch が「check が無い」で
//! すぐ終わるため。push の直後に起きることを、次のように扱う。
//!
//! - PR の head がまだ前のコミットのまま: SHA（push したコミット）を渡せば、PR の head がそれになるまで待つ（なった後の
//!   push の head は追う）。渡さなければそのときの head を見る（前の head の結果を返しうる）
//! - check が 1 件も無い: 登録を待つ間（約 1 分）待ち、それでも無ければ CI の無い head として `CHECKS: none` で終える。
//!   head が変わったら、登録を待つ時間を初めから数える
//! - 先に登録された check だけが終わっている: 全部が通って見えても、少し置いて（GH_WAIT_CHECKS_SETTLE 秒）もう 1 度
//!   見て、同じなら通ったとする（それより後に登録される check は見落とす）
//!
//! 落ちた check（failure、timed_out）があれば、ほかの check の終わりを待たずに `CHECKS: failed` で終える。状態を
//! 取れない回は、通ったとも無いとも数えず、5 回続けば止まる（権限の無いトークンなど、待っても変わらない失敗）。Copilot のレビューの実行の
//! 失敗は、コードの問題でないので失敗に数えない（マージの hook と同じ）。
//!
//! 待つ間隔（秒）は GH_WAIT_CHECKS_INTERVALS、登録を待つ間隔は GH_WAIT_CHECKS_REGISTER_INTERVALS で上書きできる
//! （テスト用）。
//!
//! 出力: `HEAD: <SHA>`、`CHECKS: passed（N 件、待機 S 秒） | failed | none`、失敗した check ごとに
//! `FAILED: <名前> (<結論>)`。
//! 終了コード: 0 = 全部が通った（か check が無い）、1 = 時間切れ（約 20 分）、3 = PR が見つからない、
//! 4 = gh が使えないか、check の状態を取れない、5 = 引数の誤り、6 = 失敗した check がある（2 は、このスクリプトを
//! 知らない古い claude-hooks が返すので使わない）

use super::pr_url;
use serde_json::Value;
use std::time::Duration;

/// 15+30+45+60×4+90×4+120×4 = 1170 秒（約 20 分。このリポジトリの CI は 4 分ほどで、Windows の job が長い）
const INTERVALS: &str = "15 30 45 60 60 60 60 90 90 90 90 120 120 120 120";
/// push から check の登録までは数秒から十数秒（2026-11-17 の PR で、push の直後の gh pr checks が「check が無い」を返した）
const REGISTER_INTERVALS: &str = "5 10 15 30";
/// 全部が通って見えてから、もう 1 度見るまでの秒数（後から登録される check を待つ。登録を待つ間隔の最後と同じ）
const SETTLE: &str = "30";
/// 通って見えた後に、件数が変わって見直す回数の上限
const MAX_SETTLES: u32 = 10;
/// 状態を続けて取れなかったら止まる回数（最初の間隔は 15、30、45、60 秒なので、2 分半の不調までは待つ）
const MAX_UNKNOWN: u32 = 5;

/// head の check の状態
enum State {
    /// 状態を取れない
    Unknown,
    /// PR の head が、待つコミットでない（今の head）
    OtherHead(String),
    /// check が 1 件も無い
    Empty,
    /// 終わっていない check の名前（落ちた check は無い）
    Running(Vec<String>),
    /// 落ちた check の「名前 (結論)」（ほかに実行中の check があってもよい）
    Failed(Vec<String>),
    /// 全部が終わって通った。件数
    Passed(usize),
}

fn short(sha: &str) -> &str {
    sha.get(..7).unwrap_or(sha)
}

pub fn main(args: &[String]) -> i32 {
    let (url, want) = match args {
        [url] => (url, None),
        [url, sha]
            if (7..=40).contains(&sha.len()) && sha.bytes().all(|b| b.is_ascii_hexdigit()) =>
        {
            (url, Some(sha.to_ascii_lowercase()))
        }
        _ => {
            println!(
                "Usage: gh-wait-checks.sh <PR の URL> [<待つコミットの SHA（7〜40 桁の 16 進）>]"
            );
            return 5;
        }
    };
    let pr = match pr_url::parse(url) {
        Ok(pr) => pr,
        Err(why) => {
            eprintln!("{why}");
            return 5;
        }
    };
    match crate::gh::quiet_status_for_repo(&pr.repo, &["auth", "status"]) {
        Err(_) => {
            println!("ERROR: gh コマンドが見つかりません");
            return 4;
        }
        Ok(status) if !status.success() => {
            println!("ERROR: gh が未認証です。gh auth login を実行してください");
            return 4;
        }
        Ok(_) => {}
    }
    // 待つ間の取得の失敗は待ちの続きとして扱うので、PR が在るかはここで 1 度だけ確かめる
    if crate::gh::quiet_output_for_repo(&pr.repo, &["pr", "view", &pr.number, "--json", "number"])
        .is_none()
    {
        println!(
            "ERROR: PR #{} が見つからないか、アクセスできません",
            pr.number
        );
        return 3;
    }

    let settle = intervals("GH_WAIT_CHECKS_SETTLE", SETTLE)
        .first()
        .copied()
        .unwrap_or(0);
    let mut want = want;
    let mut register =
        intervals("GH_WAIT_CHECKS_REGISTER_INTERVALS", REGISTER_INTERVALS).into_iter();
    let mut wait = intervals("GH_WAIT_CHECKS_INTERVALS", INTERVALS).into_iter();
    let mut elapsed = 0u64;
    let mut shown_head = String::new();
    let mut unknown = 0u32;
    let mut settles = 0u32;
    // 全部が通って見えた head と件数（次に見て同じなら通ったとする）
    let mut settled: Option<(String, usize)> = None;
    loop {
        let (head, state) = state(&pr, want.as_deref());
        if !head.is_empty() && head != shown_head && !matches!(state, State::OtherHead(_)) {
            println!("HEAD: {head}");
            // 新しい head には、check の登録と終わりを待つ時間を初めから与える（渡したコミットが head になるのを待った
            // 時間も、その head の check を待つ時間から引かない）
            register =
                intervals("GH_WAIT_CHECKS_REGISTER_INTERVALS", REGISTER_INTERVALS).into_iter();
            wait = intervals("GH_WAIT_CHECKS_INTERVALS", INTERVALS).into_iter();
            settles = 0;
            shown_head = head.clone();
            // 渡したコミットが head になった後は、その後の push の head を追う（前のコミットに戻るのを待たない）
            want = None;
        }
        unknown = if matches!(state, State::Unknown) {
            unknown + 1
        } else {
            0
        };
        if !matches!(state, State::Passed(_)) {
            settled = None;
        }
        // 次に待つ秒数。待つ間隔が尽きたら時間切れ
        let next = match &state {
            State::Passed(count) => {
                if settled.as_ref() == Some(&(head.clone(), *count)) {
                    println!("CHECKS: passed（{count} 件、待機 {elapsed} 秒）");
                    return 0;
                }
                settled = Some((head.clone(), *count));
                // 件数が変わり続けても終わるよう、見直す回数を限る（待つ間隔は使わない。最後の間隔で通った check を
                // 時間切れにしない）
                settles += 1;
                if settles > MAX_SETTLES {
                    println!("TIMEOUT: check の数が定まりません（今は {count} 件）");
                    None
                } else {
                    Some(settle)
                }
            }
            State::Failed(failed) => {
                println!("CHECKS: failed");
                for name in failed {
                    println!("FAILED: {name}");
                }
                return 6;
            }
            State::Empty => match register.next() {
                Some(interval) => {
                    println!(
                        "待機中: check がまだ登録されていません（経過 {elapsed} 秒）。{interval} 秒後に再確認します"
                    );
                    Some(interval)
                }
                None => {
                    println!("CHECKS: none");
                    return 0;
                }
            },
            State::OtherHead(now) => {
                let interval = wait.next();
                if let Some(interval) = interval {
                    println!(
                        "待機中: PR の head がまだ {} になっていません（今は {}）（経過 {elapsed} 秒）。{interval} 秒後に再確認します",
                        short(want.as_deref().unwrap_or("")),
                        short(now)
                    );
                } else {
                    println!(
                        "TIMEOUT: PR の head が {} になりません（今は {}）",
                        short(want.as_deref().unwrap_or("")),
                        short(now)
                    );
                }
                interval
            }
            State::Running(names) => {
                let interval = wait.next();
                if let Some(interval) = interval {
                    println!(
                        "待機中: 実行中の check が {} 件（{}）（経過 {elapsed} 秒）。{interval} 秒後に再確認します",
                        names.len(),
                        names.join(", ")
                    );
                } else {
                    println!(
                        "TIMEOUT: 実行中の check が残っています（{}）",
                        names.join(", ")
                    );
                }
                interval
            }
            State::Unknown => {
                if unknown >= MAX_UNKNOWN {
                    if head.is_empty() {
                        println!(
                            "ERROR: check の状態を {MAX_UNKNOWN} 回続けて取得できません。理由は gh pr view {} -R {} --json headRefOid で確かめてください",
                            pr.number, pr.repo
                        );
                    } else {
                        println!(
                            "ERROR: check の状態を {MAX_UNKNOWN} 回続けて取得できません。理由は gh api repos/{repo}/commits/{head}/check-runs と gh api repos/{repo}/commits/{head}/status で確かめてください",
                            repo = pr.repo
                        );
                    }
                    return 4;
                }
                let interval = wait.next();
                if let Some(interval) = interval {
                    println!(
                        "待機中: check の状態を取得できません（経過 {elapsed} 秒）。{interval} 秒後に再確認します"
                    );
                } else {
                    println!("TIMEOUT: check の状態を取得できません");
                }
                interval
            }
        };
        let Some(next) = next else {
            return 1;
        };
        std::thread::sleep(Duration::from_secs(next));
        elapsed += next;
    }
}

fn intervals(var: &str, default: &str) -> Vec<u64> {
    std::env::var(var)
        .unwrap_or_else(|_| default.to_string())
        .split_whitespace()
        .filter_map(|s| s.parse().ok())
        .collect()
}

/// PR の今の head と、その check の状態。head か check を取れなければ Unknown。want があって head がそれでなければ
/// OtherHead（check は見ない）
fn state(pr: &pr_url::Pr, want: Option<&str>) -> (String, State) {
    let head = crate::gh::quiet_output_for_repo(
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
    )
    .map(|out| String::from_utf8_lossy(&out).trim().to_string())
    .unwrap_or_default();
    if head.is_empty() {
        return (head, State::Unknown);
    }
    if want.is_some_and(|sha| !head.starts_with(sha)) {
        return (head.clone(), State::OtherHead(head));
    }
    // check の多い PR で取りこぼさないよう全ページを見る。--paginate に配列を返す --jq を渡すと不正な JSON に
    // なるので、1 件ずつ出させる
    let runs = crate::gh::quiet_output_for_repo(
        &pr.repo,
        &[
            "api",
            "--paginate",
            &format!("repos/{{owner}}/{{repo}}/commits/{head}/check-runs?per_page=100"),
            "--jq",
            ".check_runs[] | {name, status, conclusion}",
        ],
    )
    .and_then(|out| {
        serde_json::Deserializer::from_slice(&out)
            .into_iter::<Value>()
            .collect::<Result<Vec<_>, _>>()
            .ok()
    });
    // コミットの status（外部の CI）。pending のほかは終わったものとして、state を結論にする
    let statuses = crate::gh::quiet_output_for_repo(
        &pr.repo,
        &[
            "api",
            &format!("repos/{{owner}}/{{repo}}/commits/{head}/status"),
            "--jq",
            "[.statuses[] | {name: .context, status: (if .state == \"pending\" then \"in_progress\" else \"completed\" end), conclusion: .state}]",
        ],
    )
    .and_then(|out| serde_json::from_slice::<Value>(&out).ok())
    .and_then(|v| v.as_array().cloned());
    let (Some(runs), Some(statuses)) = (runs, statuses) else {
        return (head, State::Unknown);
    };
    let all: Vec<Value> = runs.into_iter().chain(statuses).collect();
    if all.is_empty() {
        return (head, State::Empty);
    }
    let name = |c: &Value| c["name"].as_str().unwrap_or("").to_string();
    let done = |c: &&Value| c["status"] == "completed";
    let mut running: Vec<String> = all.iter().filter(|c| !done(c)).map(name).collect();
    // 実行中の check がある間は、failure と timed_out だけを落ちたと数える（取り消された run は、同じ head の次の run に
    // 置き換わることがある）。全部が終わったら、通っていない結論をすべて数える（マージの hook と同じ）
    let counts_as_failed = |conclusion: Option<&str>| {
        if running.is_empty() {
            !matches!(conclusion, Some("success" | "neutral" | "skipped"))
        } else {
            matches!(conclusion, Some("failure" | "timed_out"))
        }
    };
    let mut failed: Vec<String> = all
        .iter()
        .filter(done)
        .filter(|c| counts_as_failed(c["conclusion"].as_str()))
        .filter(|c| !name(c).to_ascii_lowercase().contains("copilot"))
        .map(|c| {
            format!(
                "{} ({})",
                name(c),
                c["conclusion"].as_str().unwrap_or("null")
            )
        })
        .collect();
    if !failed.is_empty() {
        failed.sort();
        failed.dedup();
        return (head, State::Failed(failed));
    }
    if !running.is_empty() {
        running.sort();
        running.dedup();
        return (head, State::Running(running));
    }
    (head, State::Passed(all.len()))
}
