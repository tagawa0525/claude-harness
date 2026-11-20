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
//! 0 件の記録は、行に書かれたセッションの /code-review のサブエージェントの記録（その head の周の行の直後に始まったもの）
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
                    "NEXT: /code-review {} {} 焦点: {} の差分 を回し、その head にもう一度記録する（この head の周は、このファイルの追加行を読んでいない）",
                    raised(&missed.effort),
                    missed.target,
                    missed.files.join("、")
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
    /// 依頼文の対象（読めなければ None）
    target: Option<String>,
    /// tool の結果の本文をつないだもの
    text: String,
}

/// 比べる行の先頭の文字数。Read は 2000 文字を超える行を切って返すので、長い行の後ろの欠けを読んでいないと数えない
const LINE_CHARS: usize = 200;

/// 周の行の時刻から、その周の /code-review のサブエージェントの記録が始まるまでの上限（秒）。2026-11-20 に、記録 199 件の
/// 始まりが周の行の 1 秒以内だった。同じセッションで後に回したほかの PR の周を数えないための窓
const START_WINDOW: i64 = 60;

/// 子のエージェントを起動せず、自分の tool の結果で差分を読む /code-review のレベル。xhigh と max は子に読ませ、子の記録は
/// `.forked-skill.json` を持たないので数えない
const SELF_READING: [&str; 3] = ["low", "medium", "high"];

fn read_check(pr: &pr_url::Pr, head: &str, dir: &Path) -> Read {
    let Some((_, files)) = crate::rules::rounds(dir) else {
        return Read::Unknown("clone の周の置き場を引けない".to_string());
    };
    let mut sessions = BTreeSet::new();
    let mut windows = Vec::new();
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
            if let Some(at) = round.at.and_then(|at| i64::try_from(at).ok()) {
                windows.push((at * 1_000_000, (at + START_WINDOW) * 1_000_000));
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
    let transcripts = review_transcripts(&config, &sessions, &windows);
    let Some(latest) = transcripts.last() else {
        return Read::Unknown(format!(
            "{} の周の行の {START_WINDOW} 秒以内に始まった /code-review の記録が {} の下のセッション {} に無い",
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
    let pr_lines = added_lines(&String::from_utf8_lossy(&diff));
    // 前の周の PR の差分は、PR の base との分かれ目から引く（rebase の後も、base の変更を PR の変更と数えない）
    let base = if previous.is_empty() {
        None
    } else {
        crate::gh::output_for_repo(
            &pr.repo,
            &[
                "pr",
                "view",
                &pr.number,
                "--json",
                "baseRefOid",
                "-q",
                ".baseRefOid",
            ],
        )
        .ok()
        .map(|out| String::from_utf8_lossy(&out).trim().to_string())
        .filter(|b| !b.is_empty())
    };
    let changed = changed_since(dir, &pr_lines, &previous, head, base.as_deref());
    let required = required_lines(pr_lines, changed);
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
    if let Some(t) = transcripts
        .iter()
        .find(|t| !SELF_READING.contains(&t.effort.as_str()))
    {
        return Read::Unknown(format!(
            "{} の周の /code-review は子のエージェントに差分を読ませ、子の記録は数えない",
            if t.effort.is_empty() {
                "レベルの分からない"
            } else {
                &t.effort
            }
        ));
    }
    let files: BTreeSet<String> = missed.iter().map(|(f, _)| (*f).clone()).collect();
    Read::No(Missed {
        transcripts: transcripts.len(),
        latest: latest.path.clone(),
        effort: latest.effort.clone(),
        target: latest
            .target
            .clone()
            .unwrap_or_else(|| dir.display().to_string()),
        required: total,
        lines: missed.len(),
        files: files.into_iter().collect(),
    })
}

/// pr の追加行のうち、前の周の head から変わったもの。前の周が無ければ None。base（PR の base の SHA）を引ければ、前の周の
/// PR の差分（分かれ目から前の head まで）に無い行。引けなければ、前の head が祖先のとき `git diff <前の head> <head>` の
/// 追加行と重なる行で、rebase した head（祖先でない）は None（base の変更と PR の変更を分けられないので、PR の差分の全部を
/// 求める）
fn changed_since(
    dir: &Path,
    pr: &BTreeMap<String, BTreeSet<String>>,
    previous: &BTreeSet<String>,
    head: &str,
    base: Option<&str>,
) -> Option<BTreeMap<String, BTreeSet<String>>> {
    if previous.is_empty() {
        return None;
    }
    let mut out: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
    for prev in previous {
        let old = base
            .and_then(|b| git_out(dir, &["merge-base", prev, b]))
            .and_then(|fork| git_diff(dir, fork.trim(), prev));
        let (since, keep_if_found) = match old {
            Some(old) => (added_lines(&old), false),
            None if git_out(dir, &["merge-base", "--is-ancestor", prev, head]).is_some() => {
                (added_lines(&git_diff(dir, prev, head)?), true)
            }
            None => return None,
        };
        for (file, lines) in pr {
            let seen = since.get(file);
            let kept = lines
                .iter()
                .filter(|l| seen.is_some_and(|s| s.contains(*l)) == keep_if_found);
            out.entry(file.clone()).or_default().extend(kept.cloned());
        }
    }
    out.retain(|_, lines| !lines.is_empty());
    Some(out)
}

/// 読んだかを比べる追加行（ファイルごと）。前の周から変わった行（changed）があればそれに絞る（直した後の周は、直した差分を
/// 焦点に読む）。lock のファイルは読まないので外す。テストのファイル（/code-review の low が外す）は、ほかのファイルの行が
/// あれば外す
fn required_lines(
    pr: BTreeMap<String, BTreeSet<String>>,
    changed: Option<BTreeMap<String, BTreeSet<String>>>,
) -> BTreeMap<String, BTreeSet<String>> {
    let not_lock = |m: BTreeMap<String, BTreeSet<String>>| -> BTreeMap<String, BTreeSet<String>> {
        m.into_iter().filter(|(f, _)| !is_lock(f)).collect()
    };
    let narrowed = changed.map(not_lock).unwrap_or_default();
    // 前の周から変わった行が無い（rebase だけ、消すだけの修正）なら、PR の差分の全部を求める
    let lines = if narrowed.is_empty() {
        not_lock(pr)
    } else {
        narrowed
    };
    let body: BTreeMap<_, _> = lines
        .iter()
        .filter(|(f, _)| !is_test(f))
        .map(|(f, l)| (f.clone(), l.clone()))
        .collect();
    if body.is_empty() { lines } else { body }
}

/// /code-review の low が外すと書くテストの形（`test/`、`spec/`、`__tests__/`、`*_test.*`、`*.test.*`、`fixtures/`、
/// `testdata/`）に、`tests/` と `test_*` を足したもの。名前に test を含むだけの本体（`latest.rs`）は外さない（外すと、
/// 本体を読まない周を通す。low が `':!*test*'` で外して読まなかったなら、読んでいないとして回し直す）
fn is_test(path: &str) -> bool {
    let mut parts: Vec<&str> = path.split('/').collect();
    let name = parts.pop().unwrap_or_default();
    parts
        .iter()
        .any(|d| ["test", "tests", "spec", "__tests__", "fixtures", "testdata"].contains(d))
        || name.starts_with("test_")
        || name.contains("_test.")
        || name.contains(".test.")
        || name.contains("_spec.")
}

fn is_lock(path: &str) -> bool {
    let name = path.rsplit('/').next().unwrap_or(path);
    name.ends_with(".lock") || name.ends_with("-lock.json") || name.ends_with("-lock.yaml")
}

/// unified diff の追加行（前後の空白を除き、先頭の [`LINE_CHARS`] 文字）を、追加した側のパスごとに。英数字（漢字と
/// かなを含む）が 3 字に満たない行（`}`、`Ok(())`）は、ほとんどの tool の結果に入り、読んだことを示さないので比べない
fn added_lines(diff: &str) -> BTreeMap<String, BTreeSet<String>> {
    let mut out: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
    let mut file: Option<String> = None;
    let mut in_hunk = false;
    for line in diff.lines() {
        if line.starts_with("diff --git ") {
            file = None;
            in_hunk = false;
        } else if !in_hunk && let Some(path) = line.strip_prefix("+++ ") {
            file = unquote(path).strip_prefix("b/").map(str::to_string);
        } else if line.starts_with("@@") {
            in_hunk = true;
        } else if in_hunk && let (Some(f), Some(text)) = (&file, line.strip_prefix('+')) {
            let text: String = text.trim().chars().take(LINE_CHARS).collect();
            if text.chars().filter(|c| c.is_alphanumeric()).count() >= 3 {
                out.entry(f.clone()).or_default().insert(text);
            }
        }
    }
    out
}

/// git が引用したパス（`"b/\346\227\245.md"`。C の文字列の形で、ASCII でないバイトは 8 進）を元の字面に戻す。引用の無い
/// パスはそのまま
fn unquote(path: &str) -> String {
    let Some(inner) = path.strip_prefix('"').and_then(|p| p.strip_suffix('"')) else {
        return path.to_string();
    };
    let mut bytes = Vec::new();
    let mut chars = inner.chars().peekable();
    while let Some(c) = chars.next() {
        if c != '\\' {
            let mut buf = [0; 4];
            bytes.extend_from_slice(c.encode_utf8(&mut buf).as_bytes());
            continue;
        }
        match chars.next() {
            Some(d @ '0'..='7') => {
                let mut value = d.to_digit(8).unwrap_or(0);
                for _ in 0..2 {
                    if let Some(next) = chars.peek().and_then(|c| c.to_digit(8)) {
                        value = value * 8 + next;
                        chars.next();
                    }
                }
                bytes.push(u8::try_from(value).unwrap_or(b'?'));
            }
            Some('t') => bytes.push(b'\t'),
            Some('n') => bytes.push(b'\n'),
            Some(other) => {
                let mut buf = [0; 4];
                bytes.extend_from_slice(other.encode_utf8(&mut buf).as_bytes());
            }
            None => {}
        }
    }
    String::from_utf8_lossy(&bytes).into_owned()
}

/// `git <args>` を、起動したディレクトリの clone で実行する（hook の中で渡る GIT_DIR に向かない）。成功なら標準出力
fn git_out(dir: &Path, args: &[&str]) -> Option<String> {
    let out = std::process::Command::new("git")
        .args(args)
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

/// `git diff from to`。接頭辞は利用者の diff.noprefix に依らず a/ と b/ にし、パスは引用しても [`unquote`] で戻す
fn git_diff(dir: &Path, from: &str, to: &str) -> Option<String> {
    git_out(
        dir,
        &[
            "diff",
            "--no-color",
            "--no-ext-diff",
            "--src-prefix=a/",
            "--dst-prefix=b/",
            "-U0",
            from,
            to,
        ],
    )
}

/// Claude Code の設定の場所（`CLAUDE_CONFIG_DIR`、無ければ `~/.claude`）
fn config_dir() -> PathBuf {
    std::env::var_os("CLAUDE_CONFIG_DIR")
        .filter(|d| !d.is_empty())
        .map(PathBuf::from)
        .unwrap_or_else(|| crate::shell::home().join(".claude"))
}

/// sessions のサブエージェントの記録のうち、/code-review が起動し、windows（UNIX のマイクロ秒の [始まり, 終わり)）の
/// どれかの中で始まったもの（始まった順）。記録の形は Claude Code の公開の仕様ではないので、読めない行と記録は飛ばす
/// （飛ばした結果、記録が無ければ、確かめられないとして理由を求める）
fn review_transcripts(
    config: &Path,
    sessions: &BTreeSet<String>,
    windows: &[(i64, i64)],
) -> Vec<Transcript> {
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
                let Some(mut t) = read_transcript(&jsonl, windows) else {
                    continue;
                };
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

/// 記録を読む。最初の行の時刻が windows のどれにも入らなければ、残りの行を読まずに None
fn read_transcript(path: &Path, windows: &[(i64, i64)]) -> Option<Transcript> {
    let text = std::fs::read_to_string(path).ok()?;
    let mut lines = text.lines();
    let started = lines
        .next()
        .and_then(|l| serde_json::from_str::<Value>(l).ok())
        .and_then(|v| {
            v.get("timestamp")
                .and_then(Value::as_str)
                .and_then(parse_time)
        })
        .map(|t| t.micros)?;
    if !windows
        .iter()
        .any(|(from, to)| (*from..*to).contains(&started))
    {
        return None;
    }
    let mut target = None;
    let mut results = String::new();
    for line in text.lines() {
        let Ok(v) = serde_json::from_str::<Value>(line) else {
            continue;
        };
        if v.get("type").and_then(Value::as_str) != Some("user") {
            continue;
        }
        match v.pointer("/message/content") {
            // 最初の依頼文（/code-review が渡す `Review target: `<対象> <焦点>``）から対象を取る
            Some(Value::String(s)) if target.is_none() => target = review_target(s),
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
        started,
        effort: String::new(),
        target,
        text: results,
    })
}

fn review_target(prompt: &str) -> Option<String> {
    prompt
        .split_once("Review target: `")
        .and_then(|(_, rest)| rest.split('`').next())
        .and_then(|t| t.split_whitespace().next())
        .map(str::to_string)
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
    if effort == "low" { "medium" } else { "high" }
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
            "web/a.spec.ts",
            "App/Tests/A.cs",
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
        // C の文字列の形のほかの文字（`\r` など）も戻す
        assert_eq!(unquote(r#""b/a\rb\\c""#), "b/a\rb\\c");
    }
}
