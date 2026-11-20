//! `record-local-review.sh <PR の URL> <SHA> <high> <medium> <low> [--finding <要旨>]...`: ローカルの /code-review の結果を、PR の head の
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
//!
//! 0 件の記録は、行に書かれたセッションの /code-review のサブエージェントの記録（その head の最初の行より後に始まったもの）
//! の tool の結果に、PR の差分の追加行が入っているときだけ付ける。入っていなければ付けず、レベルを上げて欠けたファイルを
//! 焦点にした周を NEXT: で示す。記録か差分を引けなければ、`--unlogged <理由>` で理由を残して付ける
//!
//! 指摘は、1 件ごとに要旨を `--finding <1 行>` で受けて、記録のコメントに書く。件数だけの記録では、対応しなかった指摘の
//! 中身が会話にしか残らず、セッションが替わるとマージの本文に書けない。数が件数の合計と合わなければ記録しない。
//! 要旨をファイルで受けない理由は、このスクリプトが確認なしで通る許可を持ち、任意のファイルを PR に出す経路になるため
//! （claude-harness の issue 346）。読む側の hook はコメントの 1 行目だけを見るので、要旨は印の形を変えない

use super::pr_url;
use super::time::parse_time;
use regex::Regex;
use serde_json::Value;
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::sync::LazyLock;

const USAGE: &str = "Usage: record-local-review.sh <PR の URL> <レビューした SHA> <high> <medium> <low> [--finding <指摘 1 件の要旨（1 行）>]... [--unlogged <回した記録を確かめられない理由>]";

static SHA: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^[0-9a-f]{7,40}$").unwrap());
/// 読む側は `high=0 ` の字面で比べるので、先頭の 0 を許さない
static COUNT: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^(0|[1-9][0-9]*)$").unwrap());

pub fn main(args: &[String]) -> i32 {
    let [url, reviewed, counts @ ..] = args else {
        eprintln!("{USAGE}");
        return 1;
    };
    let [h, m, l, options @ ..] = counts else {
        eprintln!("{USAGE}");
        return 1;
    };
    let counts = [h, m, l];
    let mut unlogged = None;
    let mut findings = Vec::new();
    for option in options.chunks(2) {
        match option {
            [flag, reason]
                if flag == "--unlogged" && unlogged.is_none() && !reason.trim().is_empty() =>
            {
                unlogged = Some(reason.trim())
            }
            // 1 件を 1 行で読めるように、改行（CR だけのものも Markdown では改行）などの制御文字を含む値は受けない
            [flag, finding] if flag == "--finding" => {
                if finding.trim().is_empty() || finding.chars().any(char::is_control) {
                    eprintln!("{USAGE}（--finding の値は、空でない 1 行: {finding:?}）");
                    return 1;
                }
                findings.push(finding.trim())
            }
            _ => {
                eprintln!("{USAGE}");
                return 1;
            }
        }
    }
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
    // 足すと u64 を超える件数を、0 件に見せない
    let total = high.saturating_add(medium).saturating_add(low);
    if u64::try_from(findings.len()).ok() != Some(total) {
        eprintln!(
            "ERROR: 指摘が {total} 件なのに、要旨（--finding）が {} 件です（記録を付けていません）。指摘 1 件ごとに、末尾に --finding '<重大度 ファイル:行 要旨（直すか `再現なし` か）>' を付けてください（記録のコメントに書く。件数だけでは、セッションが替わると対応しなかった指摘をマージの本文に書けない）",
            findings.len()
        );
        return 1;
    }

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
             - この head で回したのに hook が行を書かない回し方だったなら（対象に PR の URL を渡した、detached HEAD の worktree、ユーザーが打った /code-review）、末尾に --unlogged '<その回し方>' を付ける（記録のコメントに残る。--unlogged の記録は今のディレクトリに頼らないので、cd <worktree> && は要らない）\n\
             - 回し直していない、base への rebase だけの head なら、記録を足さない（前の head の記録で足りるかは、PR の worktree で {scripts}/decide-next.sh {canonical} を実行すると LOCAL_REVIEWED: の行に出る）"
        );
        return 1;
    }
    // 0 件の記録は、指摘ゼロの周と何も読まずに返した周（大きな出力のファイルを開かない、テストだけの差分でテストを外す）を
    // 返答からは見分けられないので、その周の記録の tool の結果に差分が入ったかで確かめる（ADR-0103）
    let mut unverified = false;
    if total == 0
        && logged
        && let Ok(dir) = cwd.as_deref()
    {
        match read_check(&pr, &head, dir) {
            Read::Yes => {}
            Read::No(missed) => {
                eprintln!(
                    "ERROR: {} で回した /code-review の記録（{} 件。最後は {}）の tool の結果に、PR の差分の追加行が {} 行のうち {} 行入っていません（{}。記録を付けていません）。0 件の返答は、差分を読んだ周でなければ未実施です",
                    short(&head),
                    missed.transcripts,
                    missed.latest.display(),
                    missed.required,
                    missed.lines,
                    missed.files.join("、")
                );
                println!(
                    "NEXT: /code-review {} {} 焦点: {} の差分 を回し、その head にもう一度記録する（{} の周までに、このファイルの追加行を読んでいない）",
                    raised(&missed.effort),
                    missed.target,
                    missed.files.join("、"),
                    missed.effort
                );
                return 1;
            }
            Read::Unknown(why) => {
                if unlogged.is_none() {
                    eprintln!(
                        "ERROR: {why}ので、{} で回した /code-review が差分を読んだかを確かめられません（記録を付けていません）。別の機械で回した、記録が消えたなど、確かめられない理由が分かっているなら、末尾に --unlogged '<その理由>' を付ける（記録のコメントに残る）",
                        short(&head)
                    );
                    return 1;
                }
                unverified = true;
            }
        }
    }
    let mut body = format!(
        "{}\nローカルの /code-review で {} を確かめた: High {high} / Medium {medium} / Low {low}",
        crate::rules::local_review_mark(&head, high, medium, low),
        short(&head)
    );
    if let Some(reason) = unlogged.filter(|_| !logged || unverified) {
        body.push_str(&format!("\n回した記録を確かめていない理由: {reason}"));
    }
    if !findings.is_empty() {
        body.push_str("\n\n指摘:");
        for finding in &findings {
            body.push_str(&format!("\n- {finding}"));
        }
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
    // 差分を読んだかを確かめていない 0 件の記録（--unlogged）は、返答を読んだ人にしか未実施と見分けられない
    if total == 0 && (!logged || unverified) {
        println!(
            "NOTE: 0 件の記録が正しいのは、/code-review の返答に見たファイルや確かめた点が書かれているときだけ。対象が見つからない（「ブランチが存在しない」など）、エラー、`[]` だけの返答は未実施なので、対象を直して回し直し、同じ head にもう一度記録する（最後の記録で決まる）"
        );
    }
    0
}

fn short(sha: &str) -> &str {
    sha.get(..7).unwrap_or(sha)
}

/// 0 件の記録の前に確かめた、その周の /code-review が差分を読んだか
enum Read {
    /// その head の周の /code-review の記録の tool の結果に、求める追加行が全部ある
    Yes,
    /// 記録はあるが、その head の周を合わせても追加行が欠ける
    No(Missed),
    /// 記録か差分を引けない（理由。拒否の文で「ので」を続ける）
    Unknown(String),
}

struct Missed {
    /// 見た記録の数と、最後に始まった記録（レベルと対象は、この記録のもの）
    transcripts: usize,
    latest: PathBuf,
    effort: String,
    target: String,
    /// 求めた追加行の数と、そのうち欠けた数
    required: usize,
    lines: usize,
    /// 欠けた行のあるファイル
    files: Vec<String>,
}

/// サブエージェントの記録（`<設定>/projects/<プロジェクト>/<セッション>/subagents/agent-<id>.jsonl`）のうち、
/// /code-review が起動したもの（隣の `.forked-skill.json` の skillName が `code-review`）
struct Transcript {
    path: PathBuf,
    /// 最初の行の時刻（UNIX のマイクロ秒）
    started: i64,
    effort: String,
    target: String,
    /// tool の結果の本文をつないだもの
    text: String,
}

/// 比べる行の先頭の文字数。Read は 2000 文字を超える行を切って返すので、長い行の後ろの欠けを読んでいないと数えない
const LINE_CHARS: usize = 200;

fn read_check(pr: &pr_url::Pr, head: &str, dir: &Path) -> Read {
    let Some((_, files)) = crate::rules::rounds(dir) else {
        return Read::Unknown("clone の周の置き場を引けない".to_string());
    };
    let mut sessions = BTreeSet::new();
    let mut since: Option<u64> = None;
    let mut previous = BTreeSet::new();
    for file in &files {
        let Some(first) = file.iter().position(|r| r.head == head) else {
            continue;
        };
        if let Some(prev) = file[..first].iter().rev().find(|r| r.head != head) {
            previous.insert(prev.head.clone());
        }
        for round in file.iter().filter(|r| r.head == head) {
            sessions.extend(round.session.clone());
            if let Some(at) = round.at {
                since = Some(since.map_or(at, |s| s.min(at)));
            }
        }
    }
    if sessions.is_empty() {
        return Read::Unknown(format!(
            "{} の周の行に、回したセッションが書かれていない（セッションを書く前の版の hook で回した）",
            short(head)
        ));
    }
    let config = config_dir();
    let since_micros = i64::try_from(since.unwrap_or(0))
        .unwrap_or(i64::MAX)
        .saturating_mul(1_000_000);
    let transcripts = review_transcripts(&config, &sessions, since_micros);
    let Some(latest) = transcripts.last() else {
        return Read::Unknown(format!(
            "{} の周の後に始まった /code-review の記録が {} の下のセッション {} に無い",
            short(head),
            config.join("projects").display(),
            sessions.iter().cloned().collect::<Vec<_>>().join("、")
        ));
    };
    let Ok(diff) =
        crate::gh::output_for_repo(&pr.repo, &["pr", "diff", &pr.number, "--color", "never"])
    else {
        return Read::Unknown(format!("PR #{} の差分を gh で取れない", pr.number));
    };
    let mut changed: Option<BTreeSet<(String, String)>> = None;
    for prev in &previous {
        if let Some(d) = git_diff(dir, prev, head) {
            let set = changed.get_or_insert_with(BTreeSet::new);
            for (file, lines) in added_lines(&d) {
                set.extend(lines.into_iter().map(|l| (file.clone(), l)));
            }
        }
    }
    let required = required_lines(
        added_lines(&String::from_utf8_lossy(&diff)),
        changed.as_ref(),
    );
    let total: usize = required.values().map(BTreeSet::len).sum();
    if total == 0 {
        return Read::Unknown(format!(
            "PR #{} の差分に、読んだかを比べる追加行が無い",
            pr.number
        ));
    }
    // 欠けたファイルを焦点に回し直した周はそのファイルだけを読むので、その head の周が合わせて読んだ差分で数える
    let read: String = transcripts.iter().map(|t| t.text.as_str()).collect();
    let missed: Vec<(&String, &String)> = required
        .iter()
        .flat_map(|(file, lines)| lines.iter().map(move |l| (file, l)))
        .filter(|(_, l)| !read.contains(l.as_str()))
        .collect();
    if missed.is_empty() {
        return Read::Yes;
    }
    let files: BTreeSet<String> = missed.iter().map(|(f, _)| (*f).clone()).collect();
    Read::No(Missed {
        transcripts: transcripts.len(),
        latest: latest.path.clone(),
        effort: latest.effort.clone(),
        target: latest.target.clone(),
        required: total,
        lines: missed.len(),
        files: files.into_iter().collect(),
    })
}

/// 読んだかを比べる追加行（ファイルごと）。前の周の head から変わった行があればそれに絞る（直した後の周は、直した差分を
/// 焦点に読む）。lock のファイルは読まないので外す。テストのファイル（/code-review の low が外す）は、ほかのファイルの行が
/// あれば外す。テストの名前の範囲を広めに取るのは、low が外すファイルを自分で選び（`':!*test*'` を渡した周があった）、
/// 狭い範囲では外して読まなかったテストの行を欠けと数えるため
fn required_lines(
    pr: BTreeMap<String, BTreeSet<String>>,
    changed: Option<&BTreeSet<(String, String)>>,
) -> BTreeMap<String, BTreeSet<String>> {
    let pr: BTreeMap<_, _> = pr.into_iter().filter(|(f, _)| !is_lock(f)).collect();
    let narrowed: BTreeMap<String, BTreeSet<String>> = match changed {
        Some(changed) => pr
            .iter()
            .map(|(f, lines)| {
                let kept = lines
                    .iter()
                    .filter(|l| changed.contains(&(f.clone(), (*l).clone())))
                    .cloned()
                    .collect::<BTreeSet<_>>();
                (f.clone(), kept)
            })
            .filter(|(_, lines)| !lines.is_empty())
            .collect(),
        None => BTreeMap::new(),
    };
    // 前の周から変わった行が無い（rebase だけ、消すだけの修正）なら、PR の差分の全部を求める
    let lines = if narrowed.is_empty() { pr } else { narrowed };
    let body: BTreeMap<_, _> = lines
        .iter()
        .filter(|(f, _)| !is_test(f))
        .map(|(f, l)| (f.clone(), l.clone()))
        .collect();
    if body.is_empty() { lines } else { body }
}

fn is_test(path: &str) -> bool {
    let lower = path.to_lowercase();
    lower.contains("test") || lower.contains("fixture") || lower.split('/').any(|c| c == "spec")
}

fn is_lock(path: &str) -> bool {
    let name = path.rsplit('/').next().unwrap_or(path);
    name.ends_with(".lock") || name.ends_with("-lock.json") || name.ends_with("-lock.yaml")
}

/// unified diff の追加行（前後の空白を除き、空の行を除いた先頭の [`LINE_CHARS`] 文字）を、追加した側のパスごとに
fn added_lines(diff: &str) -> BTreeMap<String, BTreeSet<String>> {
    let mut out: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
    let mut file: Option<String> = None;
    let mut in_hunk = false;
    for line in diff.lines() {
        if line.starts_with("diff --git ") {
            file = None;
            in_hunk = false;
        } else if !in_hunk && let Some(path) = line.strip_prefix("+++ ") {
            let path = path.trim_matches('"');
            file = path.strip_prefix("b/").map(str::to_string);
        } else if line.starts_with("@@") {
            in_hunk = true;
        } else if in_hunk && let (Some(f), Some(text)) = (&file, line.strip_prefix('+')) {
            let text: String = text.trim().chars().take(LINE_CHARS).collect();
            if !text.is_empty() {
                out.entry(f.clone()).or_default().insert(text);
            }
        }
    }
    out
}

/// `git diff from to`。起動したディレクトリの clone で引く（hook の中で渡る GIT_DIR に向かない）。接頭辞は、利用者の
/// diff.noprefix に依らず a/ と b/ にする
fn git_diff(dir: &Path, from: &str, to: &str) -> Option<String> {
    let out = std::process::Command::new("git")
        .args([
            "diff",
            "--no-color",
            "--no-ext-diff",
            "--src-prefix=a/",
            "--dst-prefix=b/",
            "-U0",
            from,
            to,
        ])
        .current_dir(dir)
        .env_remove("GIT_DIR")
        .env_remove("GIT_WORK_TREE")
        .env_remove("GIT_COMMON_DIR")
        .stderr(std::process::Stdio::null())
        .output()
        .ok()?;
    out.status
        .success()
        .then(|| String::from_utf8_lossy(&out.stdout).into_owned())
}

/// Claude Code の設定の場所（`CLAUDE_CONFIG_DIR`、無ければ `~/.claude`）
fn config_dir() -> PathBuf {
    std::env::var_os("CLAUDE_CONFIG_DIR")
        .filter(|d| !d.is_empty())
        .map(PathBuf::from)
        .unwrap_or_else(|| crate::shell::home().join(".claude"))
}

/// sessions のサブエージェントの記録のうち、/code-review が起動し、since（UNIX のマイクロ秒）より後に始まったもの
/// （始まった順）。記録の形は Claude Code の公開の仕様ではないので、読めない行と記録は飛ばす（飛ばした結果、記録が
/// 無ければ、確かめられないとして理由を求める）
fn review_transcripts(config: &Path, sessions: &BTreeSet<String>, since: i64) -> Vec<Transcript> {
    let mut found = Vec::new();
    let Ok(projects) = std::fs::read_dir(config.join("projects")) else {
        return found;
    };
    for project in projects.flatten() {
        for session in sessions {
            let Ok(entries) = std::fs::read_dir(project.path().join(session).join("subagents"))
            else {
                continue;
            };
            for entry in entries.flatten() {
                let path = entry.path();
                let Some(stem) = path
                    .to_str()
                    .and_then(|p| p.strip_suffix(".forked-skill.json"))
                else {
                    continue;
                };
                let Some(meta) = std::fs::read_to_string(&path)
                    .ok()
                    .and_then(|t| serde_json::from_str::<Value>(&t).ok())
                else {
                    continue;
                };
                if meta.get("skillName").and_then(Value::as_str) != Some("code-review") {
                    continue;
                }
                let jsonl = PathBuf::from(format!("{stem}.jsonl"));
                let Some(mut t) = read_transcript(&jsonl) else {
                    continue;
                };
                if t.started < since {
                    continue;
                }
                t.effort = meta
                    .get("effort")
                    .and_then(Value::as_str)
                    .unwrap_or_default()
                    .to_string();
                found.push(t);
            }
        }
    }
    found.sort_by_key(|t| t.started);
    found
}

fn read_transcript(path: &Path) -> Option<Transcript> {
    let text = std::fs::read_to_string(path).ok()?;
    let mut started = None;
    let mut target = None;
    let mut results = String::new();
    for line in text.lines() {
        let Ok(v) = serde_json::from_str::<Value>(line) else {
            continue;
        };
        if started.is_none() {
            started = v
                .get("timestamp")
                .and_then(Value::as_str)
                .and_then(parse_time)
                .map(|t| t.micros);
        }
        if v.get("type").and_then(Value::as_str) != Some("user") {
            continue;
        }
        let content = v.pointer("/message/content");
        match content {
            // 最初の依頼文（/code-review が渡す `Review target: `<対象> <焦点>``）から対象を取る
            Some(Value::String(s)) if target.is_none() => target = Some(review_target(s)),
            Some(Value::Array(items)) => {
                for item in items {
                    if item.get("type").and_then(Value::as_str) == Some("tool_result") {
                        push_text(item.get("content"), &mut results);
                    }
                }
            }
            _ => {}
        }
    }
    Some(Transcript {
        path: path.to_path_buf(),
        started: started?,
        effort: String::new(),
        target: target.unwrap_or_else(|| "<WORKTREE>".to_string()),
        text: results,
    })
}

fn review_target(prompt: &str) -> String {
    prompt
        .split_once("Review target: `")
        .and_then(|(_, rest)| rest.split('`').next())
        .and_then(|t| t.split_whitespace().next())
        .unwrap_or("<WORKTREE>")
        .to_string()
}

/// tool の結果の本文（文字列か、text の項目の配列）
fn push_text(content: Option<&Value>, out: &mut String) {
    match content {
        Some(Value::String(s)) => {
            out.push_str(s);
            out.push('\n');
        }
        Some(Value::Array(items)) => {
            for item in items {
                if let Some(s) = item.get("text").and_then(Value::as_str) {
                    out.push_str(s);
                    out.push('\n');
                }
            }
        }
        _ => {}
    }
}

/// 読んでいなかった周の次のレベル（xhigh と max は頼まれたときだけ回すので、high の上に上げない）
fn raised(effort: &str) -> &str {
    match effort {
        "low" => "medium",
        "medium" | "high" | "" => "high",
        other => other,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_files_are_named_by_the_patterns_code_review_skips() {
        // /code-review の low が外すと書く形（test/、spec/、__tests__/、*_test.*、*.test.*、fixtures/、testdata/）と tests/、
        // test_*。名前に test を含むだけのファイルは本体として求める（外すと、本体を読まない周を通す）
        for path in [
            "tests/a.rs",
            "crates/x/tests/common/mod.rs",
            "claude/tests/scripts/x.sh",
            "src/__tests__/a.js",
            "spec/a_spec.rb",
            "pkg/a_test.go",
            "web/a.test.ts",
            "test_a.py",
            "data/fixtures/a.json",
            "x/testdata/a.txt",
        ] {
            assert!(is_test(path), "{path}");
        }
        for path in [
            "src/latest.rs",
            "src/attest.rs",
            "contest/a.rs",
            "docs/testimony.md",
        ] {
            assert!(!is_test(path), "{path}");
        }
    }

    #[test]
    fn lines_without_three_letters_or_digits_are_not_compared() {
        // `}` や `Ok(())` はほとんどの tool の結果に入り、読んだことを示さない
        let diff = "diff --git a/a.rs b/a.rs\n+++ b/a.rs\n@@ -1 +1,4 @@\n+}\n+Ok(())\n+    ];\n+fn ab() {}\n";
        let lines = added_lines(diff);
        assert_eq!(
            lines
                .get("a.rs")
                .map(|l| l.iter().cloned().collect::<Vec<_>>()),
            Some(vec!["fn ab() {}".to_string()])
        );
    }

    #[test]
    fn quoted_paths_are_read_as_utf8() {
        // git は core.quotePath で ASCII でないパスを 8 進で引用する。gh の差分のパスと同じ字面にそろえる
        let diff = "diff --git \"a/docs/\\346\\227\\245.md\" \"b/docs/\\346\\227\\245.md\"\n+++ \"b/docs/\\346\\227\\245.md\"\n@@ -1 +1 @@\n+本文の行\n";
        assert!(
            added_lines(diff).contains_key("docs/日.md"),
            "{:?}",
            added_lines(diff)
        );
    }
}
