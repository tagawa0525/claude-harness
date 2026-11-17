//! `gh-wait-checks.sh <PR の URL>`: PR の head の check（check run とコミットの status）が登録され、全部が終わるまで、
//! 間隔を延ばしながら待つ。見るものはマージの hook（pre-merge-check）と同じ（実行中の check があればマージは止まる）。
//!
//! `gh pr checks --watch` でなくこれを使うのは、push の直後は check がまだ登録されておらず、--watch が「check が無い」で
//! すぐ終わるため。head は待つ間の push で変わるので、毎回取り直す。check が 1 件も無いまま登録を待つ間（約 1 分）が
//! 過ぎたら、CI の無い head として `CHECKS: none` で終える。状態を取れない回は、通ったとも無いとも数えずに待ち続ける。
//! Copilot のレビューの実行の失敗は、コードの問題でないので失敗に数えない（マージの hook と同じ）。
//!
//! 待つ間隔（秒）は GH_WAIT_CHECKS_INTERVALS、登録を待つ間隔は GH_WAIT_CHECKS_REGISTER_INTERVALS で上書きできる
//! （テスト用）。
//!
//! 出力: `HEAD: <SHA>`、`CHECKS: passed（N 件、待機 S 秒） | failed | none`、失敗した check ごとに
//! `FAILED: <名前> (<結論>)`。
//! 終了コード: 0 = 全部が通った（か check が無い）、1 = 時間切れ（約 20 分）、2 = 失敗した check がある、
//! 3 = PR が見つからない、4 = gh が使えない、5 = 引数の誤り

use super::pr_url;
use serde_json::Value;
use std::time::Duration;

/// 15+30+45+60×4+90×4+120×4 = 1170 秒（約 20 分。このリポジトリの CI は 4 分ほどで、Windows の job が長い）
const INTERVALS: &str = "15 30 45 60 60 60 60 90 90 90 90 120 120 120 120";
/// push から check の登録までは数秒から十数秒（2026-11-17 の PR で、push の直後の gh pr checks が「check が無い」を返した）
const REGISTER_INTERVALS: &str = "5 10 15 30";

/// head の check の状態
enum State {
    /// 状態を取れない
    Unknown,
    /// check が 1 件も無い
    Empty,
    /// 終わっていない check の名前
    Running(Vec<String>),
    /// 全部が終わった。件数と、失敗した check の「名前 (結論)」
    Done(usize, Vec<String>),
}

pub fn main(args: &[String]) -> i32 {
    let [url] = args else {
        println!("Usage: gh-wait-checks.sh <PR の URL>");
        return 5;
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

    let register = intervals("GH_WAIT_CHECKS_REGISTER_INTERVALS", REGISTER_INTERVALS);
    let wait = intervals("GH_WAIT_CHECKS_INTERVALS", INTERVALS);
    let mut register = register.into_iter();
    let mut wait = wait.into_iter();
    let mut elapsed = 0u64;
    let mut shown_head = String::new();
    loop {
        let (head, state) = state(&pr);
        if !head.is_empty() && head != shown_head {
            println!("HEAD: {head}");
            shown_head = head;
        }
        // 次に待つ秒数。check が無い間は登録を待つ間隔、ほかは終わりを待つ間隔から取る。尽きたら結果を出して終える
        let next = match &state {
            State::Done(count, failed) if failed.is_empty() => {
                println!("CHECKS: passed（{count} 件、待機 {elapsed} 秒）");
                return 0;
            }
            State::Done(_, failed) => {
                println!("CHECKS: failed");
                for name in failed {
                    println!("FAILED: {name}");
                }
                return 2;
            }
            State::Empty => match register.next() {
                Some(interval) => {
                    println!(
                        "待機中: check がまだ登録されていません（経過 {elapsed} 秒）。{interval} 秒後に再確認します"
                    );
                    interval
                }
                None => {
                    println!("CHECKS: none");
                    return 0;
                }
            },
            State::Running(names) => match wait.next() {
                Some(interval) => {
                    println!(
                        "待機中: 実行中の check が {} 件（{}）（経過 {elapsed} 秒）。{interval} 秒後に再確認します",
                        names.len(),
                        names.join(", ")
                    );
                    interval
                }
                None => {
                    println!(
                        "TIMEOUT: 実行中の check が残っています（{}）",
                        names.join(", ")
                    );
                    return 1;
                }
            },
            State::Unknown => match wait.next() {
                Some(interval) => {
                    println!(
                        "待機中: check の状態を取得できません（経過 {elapsed} 秒）。{interval} 秒後に再確認します"
                    );
                    interval
                }
                None => {
                    println!("TIMEOUT: check の状態を取得できません");
                    return 1;
                }
            },
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

/// PR の今の head と、その check の状態。head か check を取れなければ Unknown
fn state(pr: &pr_url::Pr) -> (String, State) {
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
    let mut running: Vec<String> = all
        .iter()
        .filter(|c| c["status"] != "completed")
        .map(name)
        .collect();
    if !running.is_empty() {
        running.sort();
        running.dedup();
        return (head, State::Running(running));
    }
    let mut failed: Vec<String> = all
        .iter()
        .filter(|c| {
            !matches!(
                c["conclusion"].as_str(),
                Some("success" | "neutral" | "skipped")
            )
        })
        .filter(|c| !name(c).to_ascii_lowercase().contains("copilot"))
        .map(|c| {
            format!(
                "{} ({})",
                name(c),
                c["conclusion"].as_str().unwrap_or("null")
            )
        })
        .collect();
    failed.sort();
    failed.dedup();
    (head, State::Done(all.len(), failed))
}
