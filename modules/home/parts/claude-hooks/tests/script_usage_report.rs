//! `claude-hooks script usage-report [--since <日時>] [--until <日時>] [--repo <リポジトリ>]... [TRANSCRIPT]` が、会話の
//! 記録から区間の費用を親・担当・レビュー・振り返りに分けて出すことを確かめる（ADR-0088。`~/.claude/scripts/usage-report.sh`
//! が呼ぶ）。sh から呼ぶ配備の確認は `.claude/tests/scripts/usage-report.sh`

mod common;

use common::forget_git_env;
use serde_json::{Value, json};
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::sync::atomic::{AtomicUsize, Ordering};

const SESSION: &str = "11111111-2222-3333-4444-555555555555";
const RANGE: [&str; 4] = [
    "--since",
    "2026-11-07T00:00+09:00",
    "--until",
    "2026-11-08T00:00+09:00",
];

struct Home {
    root: PathBuf,
}

impl Drop for Home {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.root);
    }
}

fn user_text(text: &str) -> Value {
    json!({"type": "user", "message": {"role": "user", "content": text}})
}

fn usage(i: u64, w5: u64, w1: u64, r: u64, o: u64) -> Value {
    json!({"input_tokens": i, "cache_creation_input_tokens": w5 + w1, "cache_read_input_tokens": r,
        "output_tokens": o, "cache_creation": {"ephemeral_5m_input_tokens": w5, "ephemeral_1h_input_tokens": w1},
        "service_tier": "standard", "inference_geo": "global", "speed": "standard"})
}

/// 応答の 1 行
fn reply(id: &str, ts: &str, model: &str, tokens: [u64; 5]) -> Value {
    let [i, w5, w1, r, o] = tokens;
    json!({"type": "assistant", "timestamp": ts,
        "message": {"id": id, "model": model, "role": "assistant", "content": [{"type": "text", "text": "x"}],
            "usage": usage(i, w5, w1, r, o)}})
}

/// ツールを呼ぶ応答（使用量は出力 1000 トークン、0.02 ドル）
fn tool_call(id: &str, ts: &str, tool: &str, input: Value) -> Value {
    json!({"type": "assistant", "timestamp": ts,
        "message": {"id": id, "model": "claude-opus-5-5", "role": "assistant",
            "content": [{"type": "tool_use", "id": "u", "name": tool, "input": input}],
            "usage": usage(0, 0, 0, 0, 1000)}})
}

impl Home {
    fn new() -> Home {
        static NEXT: AtomicUsize = AtomicUsize::new(0);
        let root = Path::new(env!("CARGO_TARGET_TMPDIR")).join(format!(
            "usage-report-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::SeqCst)
        ));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        Home { root }
    }

    fn project(&self) -> PathBuf {
        self.root.join(".claude/projects/-home-u-repo")
    }

    fn write_lines(&self, path: &Path, lines: &[Value]) {
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        let text: String = lines.iter().map(|l| format!("{l}\n")).collect();
        std::fs::write(path, text).unwrap();
    }

    /// sh のテストと同じ記録（親、レビュー、振り返り、担当 2 本、区間の外の担当）を置き、親の記録のパスを返す
    fn first_session(&self) -> PathBuf {
        let main = self.project().join(format!("{SESSION}.jsonl"));
        let sub = self.project().join(SESSION).join("subagents");
        // 親（Fable 5.1: 入力 10、5 分の書き込み 12.5、1 時間の書き込み 20、読み出し 0.25、出力 50 ドル / 100 万トークン）
        self.write_lines(
            &main,
            &[
                user_text("始める"),
                // 1 回の応答が本文と tool_use の 2 行に記録される。2 行目は数えない
                reply("p1", "2026-11-07T01:00:00.000Z", "claude-fable-5-1", [1000, 0, 300000, 0, 200]),
                reply("p1", "2026-11-07T01:00:00.000Z", "claude-fable-5-1", [1000, 0, 300000, 0, 200]),
                // JST の 11-06 23:59:59（区間の外）
                reply("p2", "2026-11-06T14:59:59.000Z", "claude-fable-5-1", [1000000, 0, 0, 0, 0]),
                // JST の 11-07 00:00:00（区間の端。含む）
                reply("p3", "2026-11-06T15:00:00.000Z", "claude-fable-5-1", [0, 0, 0, 1000000, 0]),
                // API の失敗で Claude Code が書く応答（使用量が 0）
                json!({"type": "assistant", "timestamp": "2026-11-07T02:00:00.000Z",
                    "message": {"id": "s1", "model": "<synthetic>", "role": "assistant", "content": [],
                        "usage": {"input_tokens": 0, "cache_creation_input_tokens": 0, "cache_read_input_tokens": 0,
                            "output_tokens": 0,
                            "cache_creation": {"ephemeral_5m_input_tokens": 0, "ephemeral_1h_input_tokens": 0}}}}),
            ],
        );
        // 担当（Opus 5.5: 入力 4、5 分 5、1 時間 8、読み出し 0.20、出力 20）
        self.write_lines(
            &sub.join("agent-review.jsonl"),
            &[
                user_text("Review target: `/w/repo`"),
                reply(
                    "r1",
                    "2026-11-07T03:00:00.000Z",
                    "claude-opus-5-5",
                    [100000, 0, 0, 0, 10000],
                ),
            ],
        );
        self.write_lines(
            &sub.join("agent-retro.jsonl"),
            &[
                user_text("最初に /retro を Skill で呼び、その手順に従って進める。"),
                reply(
                    "t1",
                    "2026-11-07T04:00:00.000Z",
                    "claude-opus-5-5",
                    [0, 400000, 0, 0, 0],
                ),
            ],
        );
        self.write_lines(
            &sub.join("agent-delegate.jsonl"),
            &[
                // 依頼文の置き場の名前に retro を含む /delegate の依頼は担当に数える
                user_text("依頼文は /tmp/s/retro-eve/brief.md にある。最初に /delegate を呼び、その手順に従って進める。"),
                reply("d1", "2026-11-07T05:00:00.000Z", "claude-opus-5-5", [0, 0, 0, 1000000, 50000]),
            ],
        );
        self.write_lines(
            &sub.join("agent-explore.jsonl"),
            &[
                user_text("調べる"),
                // Haiku 5.5 は 10 万トークンを超えるプロンプトで単価が上がる（入力 0.10 → 0.50）
                reply(
                    "h1",
                    "2026-11-07T06:00:00.000Z",
                    "claude-haiku-5-5",
                    [200000, 0, 0, 0, 0],
                ),
                reply(
                    "h2",
                    "2026-11-07T06:01:00.000Z",
                    "claude-haiku-5-5",
                    [100000, 0, 0, 0, 0],
                ),
            ],
        );
        self.write_lines(
            &sub.join("agent-old.jsonl"),
            &[
                user_text("前の日の担当"),
                reply(
                    "o1",
                    "2026-11-05T05:00:00.000Z",
                    "claude-opus-5-5",
                    [1000000, 0, 0, 0, 0],
                ),
            ],
        );
        main
    }

    fn git(&self, dir: &Path, args: &[&str], date: Option<&str>) {
        let mut cmd = Command::new("git");
        forget_git_env(&mut cmd)
            .args(["-c", "user.name=t", "-c", "user.email=t@example.com"])
            .args(args)
            .current_dir(dir)
            .env("GIT_CONFIG_GLOBAL", self.root.join("no-global-config"))
            .env("GIT_CONFIG_NOSYSTEM", "1");
        if let Some(date) = date {
            cmd.env("GIT_COMMITTER_DATE", date)
                .env("GIT_AUTHOR_DATE", date);
        }
        let out = cmd.output().unwrap();
        assert!(
            out.status.success(),
            "git {args:?}: {}",
            String::from_utf8_lossy(&out.stderr)
        );
    }

    /// 区間の中に 2 本、外に 1 本のマージを持つリポジトリ
    fn repo(&self) -> PathBuf {
        let repo = self.root.join("repo");
        std::fs::create_dir_all(&repo).unwrap();
        self.git(&repo, &["init", "-q", "-b", "main"], None);
        std::fs::write(repo.join("README.md"), "x\n").unwrap();
        self.git(&repo, &["add", "README.md"], None);
        self.git(&repo, &["commit", "-q", "-m", "init"], None);
        for (branch, date) in [
            ("a", "2026-11-06T23:00:00+09:00"),
            ("b", "2026-11-07T10:00:00+09:00"),
            ("c", "2026-11-07T20:00:00+09:00"),
        ] {
            self.git(&repo, &["switch", "-q", "-c", branch], None);
            std::fs::write(repo.join(format!("{branch}.txt")), "x\n").unwrap();
            self.git(&repo, &["add", &format!("{branch}.txt")], None);
            self.git(
                &repo,
                &["commit", "-q", "-m", &format!("feat: {branch}")],
                None,
            );
            self.git(&repo, &["switch", "-q", "main"], None);
            let message = format!("Merge {branch}");
            self.git(
                &repo,
                &["merge", "-q", "--no-ff", "-m", &message, branch],
                Some(date),
            );
        }
        repo
    }

    fn report(&self, args: &[&str], session: Option<&str>) -> Output {
        let mut cmd = Command::new(env!("CARGO_BIN_EXE_claude-hooks"));
        forget_git_env(&mut cmd)
            .args(["script", "usage-report"])
            .args(args)
            .env("HOME", &self.root)
            .env("GIT_CONFIG_GLOBAL", self.root.join("no-global-config"))
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .env_remove("CLAUDE_CODE_SESSION_ID");
        if let Some(session) = session {
            cmd.env("CLAUDE_CODE_SESSION_ID", session);
        }
        cmd.output().unwrap()
    }
}

fn stdout(out: &Output) -> String {
    assert_eq!(
        out.status.code(),
        Some(0),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8(out.stdout.clone()).unwrap()
}

fn lines_with<'a>(out: &'a str, prefix: &str) -> Vec<&'a str> {
    out.lines().filter(|l| l.starts_with(prefix)).collect()
}

#[test]
fn the_report_splits_the_cost_of_the_range_by_kind() {
    let home = Home::new();
    let main = home.first_session();
    let repo = home.repo();
    let main_s = main.display().to_string();
    let repo_s = repo.display().to_string();
    let mut args = RANGE.to_vec();
    args.extend(["--repo", &repo_s, &main_s]);
    let out = stdout(&home.report(&args, None));
    let head: Vec<&str> = out.lines().take(14).collect();
    assert_eq!(
        head,
        [
            format!("TRANSCRIPT: {main_s}").as_str(),
            "SINCE: 2026-11-07T00:00:00+09:00",
            "UNTIL: 2026-11-08T00:00:00+09:00",
            "TOTAL: $10.18",
            // 親は 1 時間の書き込みを 2 倍の単価で数え、同じ message.id の 2 行目を数えない
            "COST: 親 1 本 $6.27 61.6% 書き直し 1 回 $6.00",
            // 依頼文のパスに retro を含むだけの依頼と、ほかのサブエージェントは担当に数え、区間に行の無い記録は数えない
            "COST: 担当 2 本 $1.31 12.9% 書き直し 0 回 $0.00",
            // Review target: で始まる依頼はレビュー、/retro を呼ぶ依頼は振り返り
            "COST: レビュー 1 本 $0.60 5.9% 書き直し 0 回 $0.00",
            "COST: 振り返り 1 本 $2.00 19.6% 書き直し 1 回 $2.00",
            "TOKENS: 親 input=1000 write_5m=0 write_1h=300000 read=1000000 output=200",
            "TOKENS: 担当 input=300000 write_5m=0 write_1h=0 read=1000000 output=50000",
            "TOKENS: レビュー input=100000 write_5m=0 write_1h=0 read=0 output=10000",
            "TOKENS: 振り返り input=0 write_5m=400000 write_1h=0 read=0 output=0",
            // 文脈の書き直し（25 万トークンを超える書き込み）の回数と費用
            "REWRITES: 2 回 $8.00",
            // 区間の中のマージを --first-parent で数える
            format!("MERGES: 2 {repo_s} main").as_str(),
        ]
    );
    assert_eq!(
        lines_with(&out, "PER_MERGE: "),
        ["PER_MERGE: $5.09（マージ 2 本）"]
    );
    // 費用の大きいサブエージェントを、費用の順に依頼文の頭とともに並べる
    assert_eq!(
        lines_with(&out, "TOP: "),
        [
            "TOP: $2.00 振り返り 1 回 文脈の最大 400,000 11-07 13:00-13:00 最初に /retro を Skill で呼び、その手順に従って進める。",
            "TOP: $1.20 担当 1 回 文脈の最大 1,000,000 11-07 14:00-14:00 依頼文は /tmp/s/retro-eve/brief.md にある。最初に /delegate を呼び、その手順に従って進める。",
            "TOP: $0.60 レビュー 1 回 文脈の最大 100,000 11-07 12:00-12:00 Review target: `/w/repo`",
            "TOP: $0.11 担当 2 回 文脈の最大 200,000 11-07 15:00-15:01 調べる",
        ]
    );
}

#[test]
fn a_repository_without_a_remote_counts_the_branch_of_the_main_worktree() {
    let home = Home::new();
    let main = home.first_session().display().to_string();
    let repo = home.repo();
    let feature = home.root.join("repo-feature");
    let feature_s = feature.display().to_string();
    home.git(
        &repo,
        &["worktree", "add", "-q", "-b", "feature", &feature_s, "a"],
        None,
    );
    let mut args = RANGE.to_vec();
    args.extend(["--repo", &feature_s, &main]);
    let out = stdout(&home.report(&args, None));
    assert_eq!(
        lines_with(&out, "MERGES: "),
        [format!("MERGES: 2 {feature_s} main")]
    );
}

#[test]
fn a_repository_with_origin_head_counts_its_merges() {
    let home = Home::new();
    let main = home.first_session().display().to_string();
    let repo = home.repo();
    let remote = home.root.join("remote.git");
    let remote_s = remote.display().to_string();
    home.git(&home.root, &["init", "-q", "--bare", &remote_s], None);
    home.git(&repo, &["remote", "add", "origin", &remote_s], None);
    home.git(&repo, &["push", "-q", "origin", "main"], None);
    home.git(&repo, &["remote", "set-head", "origin", "main"], None);
    let repo_s = repo.display().to_string();
    let mut args = RANGE.to_vec();
    args.extend(["--repo", &repo_s, &main]);
    let out = stdout(&home.report(&args, None));
    assert_eq!(
        lines_with(&out, "MERGES: "),
        [format!("MERGES: 2 {repo_s} origin/HEAD")]
    );
}

#[test]
fn without_a_repository_the_cost_per_merge_is_not_given() {
    let home = Home::new();
    let main = home.first_session().display().to_string();
    let mut args = RANGE.to_vec();
    args.push(&main);
    let out = stdout(&home.report(&args, None));
    assert_eq!(
        lines_with(&out, "PER_MERGE: "),
        ["PER_MERGE: -（--repo なし）"]
    );
    assert!(lines_with(&out, "MERGES: ").is_empty(), "{out}");
}

#[test]
fn a_repository_without_merges_in_the_range_has_no_cost_per_merge() {
    let home = Home::new();
    let main = home.first_session().display().to_string();
    let repo = home.repo().display().to_string();
    let out = stdout(&home.report(
        &["--since", "2026-12-01T00:00Z", "--repo", &repo, &main],
        None,
    ));
    assert_eq!(
        lines_with(&out, "PER_MERGE: "),
        ["PER_MERGE: -（マージ 0 本）"]
    );
    assert_eq!(
        lines_with(&out, "SINCE: "),
        ["SINCE: 2026-12-01T09:00:00+09:00"]
    );
    assert!(lines_with(&out, "UNTIL: ").is_empty(), "{out}");
}

#[test]
fn without_a_range_the_report_covers_today_in_jst() {
    let home = Home::new();
    let main = home.first_session().display().to_string();
    let out = stdout(&home.report(&[&main], None));
    let since = lines_with(&out, "SINCE: ");
    let until = lines_with(&out, "UNTIL: ");
    assert_eq!(since.len(), 1, "{out}");
    assert!(since[0].ends_with("T00:00:00+09:00"), "{out}");
    assert!(until[0].ends_with("T00:00:00+09:00"), "{out}");
    // 区間の端の日付は JST の今日と明日（実行の時刻で日が変わる境目を避け、日付の差で確かめる）
    let day = |line: &str| line.split_once(": ").unwrap().1[..10].to_string();
    assert_ne!(day(since[0]), day(until[0]));
}

#[test]
fn without_a_transcript_the_session_of_claude_code_session_id_is_read() {
    let home = Home::new();
    let main = home.first_session().display().to_string();
    let out = stdout(&home.report(&RANGE, Some(SESSION)));
    assert_eq!(
        lines_with(&out, "TRANSCRIPT: "),
        [format!("TRANSCRIPT: {main}")]
    );
    assert_eq!(lines_with(&out, "TOTAL: "), ["TOTAL: $10.18"]);
}

#[test]
fn the_kind_comes_from_the_first_skill_and_the_meta_name() {
    let home = Home::new();
    let session = "22222222-3333-4444-5555-666666666666";
    let main = home.project().join(format!("{session}.jsonl"));
    let sub = home.project().join(session).join("subagents");
    let ts = "2026-11-07T02:00:00.000Z";
    home.write_lines(
        &main,
        &[
            user_text("始める"),
            reply(
                "q1",
                "2026-11-07T01:00:00.000Z",
                "claude-fable-5-1",
                [0, 0, 0, 0, 0],
            ),
        ],
    );
    home.write_lines(
        &sub.join("agent-retro-skill.jsonl"),
        &[
            user_text("最初に Skill ツールで `retro` を呼び、その手順に従って進める。"),
            tool_call("k1", ts, "Skill", json!({"skill": "retro"})),
        ],
    );
    home.write_lines(
        &sub.join("agent-retro-cat.jsonl"),
        &[
            // 依頼文が「/<スキル> を」の形でなく、SKILL.md を Bash で読む振り返り
            user_text("/home/u/.claude/skills/retro/SKILL.md の手順に従って、振り返り（/retro）を行ってください。"),
            tool_call("k2", ts, "Bash", json!({"command": "cat ~/.claude/skills/retro/SKILL.md"})),
        ],
    );
    home.write_lines(
        &sub.join("agent-delegate-skill.jsonl"),
        &[
            // /delegate を呼ぶ依頼文の後ろに /retro の語があっても担当
            user_text("最初に /delegate を呼び、その手順に従って進める。/retro の手順を直す。"),
            tool_call("k3", ts, "Skill", json!({"skill": "delegate"})),
        ],
    );
    home.write_lines(
        &sub.join("agent-topic-first.jsonl"),
        &[
            // 最初に別のスキルを呼び、後で retro の SKILL.md を読む担当
            user_text("retro のスキルを直す"),
            tool_call("k4", ts, "Skill", json!({"skill": "topic-start"})),
            tool_call(
                "k5",
                "2026-11-07T02:01:00.000Z",
                "Read",
                json!({"file_path": "/home/u/.claude/skills/retro/SKILL.md"}),
            ),
        ],
    );
    home.write_lines(
        &sub.join("agent-fix-review.jsonl"),
        &[
            // description が /code-review で始まるだけの担当
            user_text("最初に /delegate を呼び、その手順に従って進める。"),
            tool_call("k6", ts, "Skill", json!({"skill": "delegate"})),
        ],
    );
    std::fs::write(
        sub.join("agent-fix-review.meta.json"),
        r#"{"agentType":"general-purpose","description":"/code-review の指摘を直す"}"#,
    )
    .unwrap();
    // 依頼文が Review target: で始まらない /code-review（meta の name で見分ける）
    for (name, id, meta_name) in [
        ("agent-old-review", "k7", "code-review"),
        ("agent-old-review-2", "k8", "code-review-2"),
    ] {
        home.write_lines(
            &sub.join(format!("{name}.jsonl")),
            &[
                user_text("`high effort → 8 inline angles`"),
                reply(id, ts, "claude-opus-5-5", [0, 0, 0, 0, 1000]),
            ],
        );
        std::fs::write(
            sub.join(format!("{name}.meta.json")),
            json!({"agentType": "general-purpose", "description": "review", "name": meta_name})
                .to_string(),
        )
        .unwrap();
    }
    let main_s = main.display().to_string();
    let mut args = RANGE.to_vec();
    args.push(&main_s);
    let out = stdout(&home.report(&args, None));
    let costs = lines_with(&out, "COST: ");
    assert_eq!(
        costs,
        [
            "COST: 親 1 本 $0.00 0.0% 書き直し 0 回 $0.00",
            "COST: 担当 3 本 $0.08 50.0% 書き直し 0 回 $0.00",
            "COST: レビュー 2 本 $0.04 25.0% 書き直し 0 回 $0.00",
            "COST: 振り返り 2 本 $0.04 25.0% 書き直し 0 回 $0.00",
        ]
    );
}

fn error(out: &Output) -> (i32, String) {
    (
        out.status.code().unwrap(),
        String::from_utf8_lossy(&out.stderr).into_owned(),
    )
}

#[test]
fn a_bound_without_an_offset_fails() {
    let home = Home::new();
    let main = home.first_session().display().to_string();
    let (code, err) = error(&home.report(&["--since", "2026-11-07T00:00", &main], None));
    assert_eq!(code, 1);
    assert!(
        err.starts_with("ERROR: --since は時差つきの ISO 8601"),
        "{err}"
    );
}

#[test]
fn a_range_whose_start_is_not_before_its_end_fails() {
    let home = Home::new();
    let main = home.first_session().display().to_string();
    let (code, err) = error(&home.report(
        &[
            "--since",
            "2026-11-08T00:00+09:00",
            "--until",
            "2026-11-07T15:00Z",
            &main,
        ],
        None,
    ));
    assert_eq!(code, 1);
    assert!(err.starts_with("ERROR: --since（2026-11-08T00:00+09:00）は --until（2026-11-07T15:00Z）より前にしてください"), "{err}");
}

#[test]
fn a_model_missing_from_the_price_table_fails() {
    let home = Home::new();
    let main = home.first_session();
    let explore = home
        .project()
        .join(SESSION)
        .join("subagents/agent-explore.jsonl");
    let mut text = std::fs::read_to_string(&explore).unwrap();
    text.push_str(&format!(
        "{}\n",
        reply(
            "x1",
            "2026-11-07T07:00:00.000Z",
            "claude-unknown-9",
            [1, 0, 0, 0, 0]
        )
    ));
    std::fs::write(&explore, text).unwrap();
    let main_s = main.display().to_string();
    let mut args = RANGE.to_vec();
    args.push(&main_s);
    let (code, err) = error(&home.report(&args, None));
    assert_eq!(code, 1);
    assert!(
        err.starts_with("ERROR: 価格表に無いモデル: claude-unknown-9"),
        "{err}"
    );
}

#[test]
fn a_usage_outside_the_standard_tier_fails() {
    let home = Home::new();
    let main = home.project().join(format!("{SESSION}.jsonl"));
    let mut line = reply(
        "f1",
        "2026-11-07T01:00:00.000Z",
        "claude-opus-5-5",
        [1, 0, 0, 0, 0],
    );
    line["message"]["usage"]["speed"] = json!("fast");
    home.write_lines(&main, &[user_text("始める"), line]);
    let main_s = main.display().to_string();
    let mut args = RANGE.to_vec();
    args.push(&main_s);
    let (code, err) = error(&home.report(&args, None));
    assert_eq!(code, 1);
    assert_eq!(
        err,
        format!("ERROR: usage.speed が 'fast' です（価格表は standard の値だけ）: {main_s}\n")
    );
}

#[test]
fn a_write_breakdown_that_does_not_add_up_fails() {
    let home = Home::new();
    let main = home.project().join(format!("{SESSION}.jsonl"));
    let mut line = reply(
        "w1",
        "2026-11-07T01:00:00.000Z",
        "claude-opus-5-5",
        [0, 10, 0, 0, 0],
    );
    line["message"]["usage"]["cache_creation_input_tokens"] = json!(11);
    home.write_lines(&main, &[user_text("始める"), line]);
    let main_s = main.display().to_string();
    let mut args = RANGE.to_vec();
    args.push(&main_s);
    let (code, err) = error(&home.report(&args, None));
    assert_eq!(code, 1);
    assert!(
        err.starts_with("ERROR: 書き込みの内訳（cache_creation）の和が"),
        "{err}"
    );
}

#[test]
fn a_broken_last_line_of_a_running_session_is_skipped() {
    let home = Home::new();
    let main = home.project().join(format!("{SESSION}.jsonl"));
    home.write_lines(
        &main,
        &[
            user_text("始める"),
            reply(
                "b1",
                "2026-11-07T01:00:00.000Z",
                "claude-opus-5-5",
                [0, 0, 0, 0, 1000],
            ),
        ],
    );
    let mut text = std::fs::read_to_string(&main).unwrap();
    text.push_str("{\"type\": \"assistant\", \"timest");
    std::fs::write(&main, text).unwrap();
    let main_s = main.display().to_string();
    let mut args = RANGE.to_vec();
    args.push(&main_s);
    let out = stdout(&home.report(&args, None));
    assert_eq!(lines_with(&out, "TOTAL: "), ["TOTAL: $0.02"]);
}

#[test]
fn a_missing_session_fails() {
    let home = Home::new();
    let (code, err) = error(&home.report(&RANGE, None));
    assert_eq!(code, 1);
    assert!(
        err.starts_with("ERROR: 記録のパスを渡すか、CLAUDE_CODE_SESSION_ID を設定してください"),
        "{err}"
    );
    let (code, err) = error(&home.report(&RANGE, Some("no-such-session")));
    assert_eq!(code, 1);
    assert!(
        err.starts_with(
            "ERROR: セッション no-such-session の記録が 0 件見つかりました（1 件のはず）"
        ),
        "{err}"
    );
}

#[test]
fn an_empty_bound_is_an_open_end() {
    let home = Home::new();
    let main = home.first_session().display().to_string();
    let out = stdout(&home.report(
        &["--since", "", "--until", "2026-11-08T00:00+09:00", &main],
        None,
    ));
    assert!(lines_with(&out, "SINCE: ").is_empty(), "{out}");
    assert_eq!(
        lines_with(&out, "UNTIL: "),
        ["UNTIL: 2026-11-08T00:00:00+09:00"]
    );
}

#[test]
fn a_transcript_that_is_not_utf8_fails() {
    let home = Home::new();
    let main = home.project().join(format!("{SESSION}.jsonl"));
    home.write_lines(&main, &[user_text("始める")]);
    let mut bytes = std::fs::read(&main).unwrap();
    bytes.extend_from_slice(b"\xff\xfe\n");
    std::fs::write(&main, bytes).unwrap();
    let main_s = main.display().to_string();
    let mut args = RANGE.to_vec();
    args.push(&main_s);
    let (code, err) = error(&home.report(&args, None));
    assert_eq!(code, 1);
    assert!(
        err.starts_with(&format!("ERROR: 記録を UTF-8 として読めません: {main_s}")),
        "{err}"
    );
}

#[test]
fn a_token_count_that_is_not_a_number_fails() {
    let home = Home::new();
    let main = home.project().join(format!("{SESSION}.jsonl"));
    let mut line = reply(
        "n1",
        "2026-11-07T01:00:00.000Z",
        "claude-opus-5-5",
        [0, 0, 0, 0, 0],
    );
    line["message"]["usage"]["output_tokens"] = json!("10");
    home.write_lines(&main, &[user_text("始める"), line]);
    let main_s = main.display().to_string();
    let mut args = RANGE.to_vec();
    args.push(&main_s);
    let (code, err) = error(&home.report(&args, None));
    assert_eq!(code, 1);
    assert_eq!(
        err,
        format!("ERROR: usage.output_tokens が数でありません（\"10\"）: {main_s}\n")
    );
}

#[test]
fn a_last_line_cut_inside_a_character_is_skipped() {
    let home = Home::new();
    let main = home.project().join(format!("{SESSION}.jsonl"));
    home.write_lines(
        &main,
        &[
            user_text("始める"),
            reply(
                "c1",
                "2026-11-07T01:00:00.000Z",
                "claude-opus-5-5",
                [0, 0, 0, 0, 1000],
            ),
        ],
    );
    // 書いている途中の最後の行が、「あ」（E3 81 82）の途中で切れている
    let mut bytes = std::fs::read(&main).unwrap();
    bytes.extend_from_slice(b"{\"type\": \"user\", \"message\": {\"content\": \"\xe3\x81");
    std::fs::write(&main, bytes).unwrap();
    let main_s = main.display().to_string();
    let mut args = RANGE.to_vec();
    args.push(&main_s);
    let out = stdout(&home.report(&args, None));
    assert_eq!(lines_with(&out, "TOTAL: "), ["TOTAL: $0.02"]);
}

#[test]
fn a_skill_file_read_with_windows_separators_is_the_first_skill() {
    let home = Home::new();
    let main = home.project().join(format!("{SESSION}.jsonl"));
    let sub = home.project().join(SESSION).join("subagents");
    home.write_lines(&main, &[user_text("始める")]);
    home.write_lines(
        &sub.join("agent-retro-windows.jsonl"),
        &[
            user_text("振り返りを行ってください。"),
            tool_call(
                "w1",
                "2026-11-07T02:00:00.000Z",
                "Read",
                json!({"file_path": "C:\\Users\\u\\.claude\\skills\\retro\\SKILL.md"}),
            ),
        ],
    );
    let main_s = main.display().to_string();
    let mut args = RANGE.to_vec();
    args.push(&main_s);
    let out = stdout(&home.report(&args, None));
    assert!(
        out.contains("COST: 振り返り 1 本 $0.02 100.0% 書き直し 0 回 $0.00"),
        "{out}"
    );
}

#[test]
fn the_meta_file_is_next_to_a_transcript_whose_name_repeats_the_suffix() {
    let home = Home::new();
    let main = home.project().join(format!("{SESSION}.jsonl"));
    let sub = home.project().join(SESSION).join("subagents");
    home.write_lines(&main, &[user_text("始める")]);
    home.write_lines(
        &sub.join("agent-x.jsonl.jsonl"),
        &[
            user_text("`high effort`"),
            reply(
                "m1",
                "2026-11-07T02:00:00.000Z",
                "claude-opus-5-5",
                [0, 0, 0, 0, 1000],
            ),
        ],
    );
    std::fs::write(
        sub.join("agent-x.jsonl.meta.json"),
        r#"{"name":"code-review"}"#,
    )
    .unwrap();
    let main_s = main.display().to_string();
    let mut args = RANGE.to_vec();
    args.push(&main_s);
    let out = stdout(&home.report(&args, None));
    assert!(out.contains("COST: レビュー 1 本 $0.02"), "{out}");
}

#[test]
fn two_empty_bounds_are_an_open_range_not_today() {
    let home = Home::new();
    let main = home.first_session().display().to_string();
    let out = stdout(&home.report(&["--since", "", "--until", "", &main], None));
    assert!(lines_with(&out, "SINCE: ").is_empty(), "{out}");
    assert!(lines_with(&out, "UNTIL: ").is_empty(), "{out}");
    // 区間の外の応答（11-05 と 11-06 の分）も数える
    assert_eq!(lines_with(&out, "TOTAL: "), ["TOTAL: $24.18"]);
}
