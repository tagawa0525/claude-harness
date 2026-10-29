//! pre-merge-check: gh pr merge 実行前にマージの前提条件を検証する
//!
//! CLAUDE.md の必須ゲートをツールレベルで強制する:
//! 1. マージ方式は --merge のみ（--squash / --rebase は禁止）
//! 2. --delete-branch でマージ後のブランチを削除する
//! 3. マージコミット本文に ## Why / ## What / ## Impact が揃っている
//! 4. CI チェックが未完了・失敗していない
//! 5. reviewDecision が CHANGES_REQUESTED / REVIEW_REQUIRED でない
//! 6. 未解決のレビュースレッドがない
//! 7. head が base（origin/main）より遅れていない（リベースしてからマージコミットする）
//! 8. 最後の push が自動レビューを受けている（bot レビューの commit_id が head と一致）。
//!    リベースだけで hash が変わった head（base からの PR の差分がレビュー済みと同じ。ADR-0021）は
//!    レビュー済みとみなす。
//!    レビュー自体が失敗している（トークン枯渇・内部エラー）ときは、コードの問題では
//!    ないので 4 の CI 失敗としては数えず、ここでレビュー未実施として報告する。
//!    レビューが回らない状況で完全に詰まないよう ALLOW_UNREVIEWED_HEAD=<理由> で外せる（ADR-0023）。
//!    「〜なら外してよい」の条件は文言に列挙しない（どれかに当てはめる読み方を誘う）。
//!    文書（変更ファイルがすべて docs/ 配下。ADR-0004）だけの PR は、一度でも自動レビューに成功
//!    していれば検査しない。文章は細部をいくらでも掘れるので、push ごとに再レビューを求めると収束しない。
//!    方針の指摘は最初のレビューで受け、競合状態やエッジケースは実装の PR でテストとともに詰める。
//!    head にローカルレビューの記録（record-local-review.sh。High なし）があれば、Copilot のレビューの
//!    有無・到着・チェックの失敗を問わず検査しない（ADR-0043。判定は decide-next.sh と同じ）。最後の記録（High なし）の
//!    SHA から rebase だけした head も同じに扱う（ADR-0059。判定は上のリベースだけと同じ）。
//!    Copilot のレビューが来ていれば、その指摘への対応は 6（未解決スレッド）が見る。
//! 9. PR が新しい ADR を 2 件以上加えていない（1 ブランチ 1 トピック。判定はローカルの
//!    git merge の pre-git-merge-check と共有）。1 つの決定を複数の ADR に分けたときと、
//!    決定前の案（status: proposed）の ADR をまとめて加えるときだけ ALLOW_MULTI_TOPIC=<理由> で外す
//!
//! 1〜3 はコマンド文字列だけで判定する。4〜9 は gh で GitHub に問い合わせ、
//! 問い合わせに失敗したら deny する（確認できない状態でマージさせない）。
//! gh の引数列は bash 版と同一に保つ（テストの偽 gh が引数の前方一致で応答する）

use super::pre_git_merge_check::multi_topic_reason;
use super::pre_pr_create_check::{body_text, missing_headings};
use super::{Rule, apply_escape_to_reasons, escape_hint};
use crate::gh;
use crate::git::git;
use crate::input::Input;
use crate::output::Finding;
use crate::shell::{Arg, Cmd, Escape, Shell, has_flag, opt_value};
use crate::unwrap::basename;
use design_records::is_adr_path;
use serde_json::Value;

pub struct PreMergeCheck;

/// 同じコマンドで gh pr merge より前に走るはずだったコマンドを挙げ、分けて投げ直す方法を示す。
/// 4〜9 は実行する前の GitHub とブランチの状態で判定するので、前のコマンド（ローカルレビューの記録、push）が
/// 変える状態は反映されず、拒むとそれらも実行されない。git と gh はサブコマンドまで挙げる（`gh issue` と
/// `gh pr` を区別する）
fn earlier_commands_hint(shell: &Shell, merge: &Cmd) -> String {
    let mut names: Vec<String> = Vec::new();
    for c in shell.commands_before(merge) {
        let program = basename(c.program());
        let name = match (program, c.args.first()) {
            ("git", _) => match c.git_subcommand() {
                Some((sub, _)) => format!("git {sub}"),
                None => program.to_string(),
            },
            ("gh", Some(sub)) => format!("gh {}", sub.text),
            _ => program.to_string(),
        };
        if !names.contains(&name) {
            names.push(name);
        }
    }
    if names.is_empty() {
        return String::new();
    }
    format!(
        "\n同じコマンドで gh pr merge より前にあるコマンド（{}）は、この判定の後に走るので判定に反映されず、\
         拒んだのでどれも実行されていません。それらで状態を変える（ローカルレビューの記録、push など）なら、\
         先に別の呼び出しで実行してから、gh pr merge を前に cd だけを付けた呼び出しで投げ直してください",
        names.join(", ")
    )
}

/// 引数から PR 番号を探す。本文（--body / --subject）より前のトークンから、値を取るフラグの
/// 値を飛ばして最初の数値を採る（`gh pr merge -R owner/repo 12 …` の形にも対応）
fn pr_ref(args: &[Arg]) -> Option<String> {
    let mut skip = false;
    for a in args {
        let t = a.text.as_str();
        if skip {
            skip = false;
            continue;
        }
        if matches!(t, "--body" | "-b" | "--subject" | "-t")
            || t.starts_with("--body=")
            || t.starts_with("--subject=")
        {
            break;
        }
        match t {
            "-R" | "--repo" | "-F" | "--body-file" => skip = true,
            _ if t.starts_with("--repo=") || t.starts_with("--body-file=") => {}
            _ if t.starts_with('-') => {}
            _ if !t.is_empty() && t.chars().all(|c| c.is_ascii_digit()) => {
                return Some(t.to_string());
            }
            _ => {}
        }
    }
    None
}

/// SHA を短縮して読みやすくする（7 文字未満はそのまま）
fn short(sha: &str) -> &str {
    sha.get(..7).unwrap_or(sha)
}

/// jq `@uri` と同じ集合（A-Za-z0-9 -_.~）以外をパーセントエンコードする
fn uri_encode(s: &str) -> String {
    let mut out = String::new();
    for b in s.bytes() {
        if b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.' | b'~') {
            out.push(b as char);
        } else {
            out.push_str(&format!("%{b:02X}"));
        }
    }
    out
}

/// gh の `--jq` 出力（JSON 値の並び）を配列として読む
fn json_stream(s: &str) -> Option<Vec<Value>> {
    serde_json::Deserializer::from_str(s)
        .into_iter::<Value>()
        .collect::<Result<Vec<_>, _>>()
        .ok()
}

/// 文書の置き場所。ここだけを変える PR は最初の自動レビューで足りる（検証 8、ADR-0004）
const DOCS_DIR: &str = "docs/";

/// PR の変更ファイルがすべて文書か。取得できない・1 件も無いときは文書だけとみなさない
fn is_docs_only(dir: &std::path::Path, owner: &str, name: &str, number: &str) -> bool {
    gh::gh(
        dir,
        &[
            "api",
            "--paginate",
            &format!("repos/{owner}/{name}/pulls/{number}/files"),
            "--jq",
            // 移動は旧パスも数える。src/ から docs/ への移動は、filename だけを見ると
            // コードを消す変更が文書だけに見える
            ".[] | .filename, (.previous_filename // empty)",
        ],
    )
    .ok()
    .is_some_and(|s| {
        let files: Vec<&str> = s.lines().map(str::trim).filter(|l| !l.is_empty()).collect();
        !files.is_empty() && files.iter().all(|f| f.starts_with(DOCS_DIR))
    })
}

/// head がローカルレビューの記録（record-local-review.sh。High なし）で覆われているか。head の記録がある（ADR-0043）か、
/// 最後の記録の SHA から rebase だけした head（ADR-0059）なら true。Copilot のレビューの
/// 有無・到着・内容は見ない。High のある記録は数えない（High を直せば head が変わるので、その head には直して
/// いない High が残る）。同じ head に記録が複数あれば、最後の記録で決める（回し直して見つけた High を前の記録で
/// 隠さない）。記録は、書き込める人のコメントだけを数える（第三者が記録を偽ってレビューを外せないように）。
/// `rebase_base` は rebase だけかを比べるときの base（base ブランチの GitHub 上の今の先頭）で、要るときだけ呼ぶ。
/// None なら比べない。記録を取得できなければ Err、比べられないときは false。decide-next.sh と request-rereview.sh も
/// `claude-hooks local-review-covers` からこれを使う（記録の読み方を 1 か所に置く）
fn local_review_covers(
    dir: &std::path::Path,
    owner: &str,
    name: &str,
    number: &str,
    head_sha: &str,
    rebase_base: impl FnOnce() -> Option<String>,
) -> Result<bool, ()> {
    let marks = gh::gh(
        dir,
        &[
            "api",
            "--paginate",
            &format!("repos/{owner}/{name}/issues/{number}/comments?per_page=100"),
            "--jq",
            ".[] | select(.author_association | IN(\"OWNER\", \"MEMBER\", \"COLLABORATOR\")) | select(.body | startswith(\"<!-- local-review \")) | (.body | split(\"\\n\")[0])",
        ],
    )?;
    let prefix = format!("<!-- local-review sha={head_sha} ");
    let mut recorded = false;
    for line in marks.lines() {
        if let Some(rest) = line.strip_prefix(&prefix) {
            recorded = rest.starts_with("high=0 ");
        }
    }
    Ok(recorded
        || local_review_rebase_source(&marks).is_some_and(|old| {
            old != head_sha && rebase_base().is_some_and(|b| same_pr_diff(dir, &b, old, head_sha))
        }))
}

/// リベースだけの判定（[`same_pr_diff`]）の比較元にするローカルレビューの記録の SHA（ADR-0059）。`marks` は記録の
/// 1 行目を古い順に並べたもの。SHA を問わず最後の記録を使い、それに High があれば比較元を返さない。High の無い
/// 記録のうち最後のものを選ばないのは、同じ差分を回し直して見つけた High を前の記録で隠さないため
fn local_review_rebase_source(marks: &str) -> Option<&str> {
    let rest = marks
        .lines()
        .last()?
        .strip_prefix("<!-- local-review sha=")?;
    let (sha, rest) = rest.split_once(' ')?;
    // 形は record-local-review.sh が書く 16 進の小文字。崩れた記録は High ありと同じく比較元にしない
    // （git の引数に渡すので、- で始まる値も通さない）
    (is_hex_sha(sha) && rest.starts_with("high=0 ")).then_some(sha)
}

/// 完全な SHA（SHA-1 の 40 桁か SHA-256 の 64 桁の 16 進の小文字）か。短い SHA は別のコミットに解けうるので受けない
fn is_hex_sha(s: &str) -> bool {
    matches!(s.len(), 40 | 64) && s.bytes().all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f'))
}

/// `claude-hooks local-review-covers <owner>/<repo> <PR 番号> <head の SHA>` の本体（decide-next.sh と request-rereview.sh が
/// hook と同じ判定を使うため）。rebase だけかは hook と同じく dir の clone のコミットで比べ（無ければ origin から取る）、
/// 比べられなければ false。記録を取得できなければ Err
pub fn local_review_covers_for_pr(
    dir: &std::path::Path,
    repo: &str,
    number: &str,
    head_sha: &str,
) -> Result<bool, ()> {
    let (owner, name) = repo.split_once('/').ok_or(())?;
    if !is_hex_sha(head_sha) || number.is_empty() || !number.bytes().all(|b| b.is_ascii_digit()) {
        return Err(());
    }
    local_review_covers(dir, owner, name, number, head_sha, || {
        let base_ref = gh::gh(
            dir,
            &[
                "api",
                &format!("repos/{owner}/{name}/pulls/{number}"),
                "--jq",
                ".base.ref",
            ],
        )
        .ok()?;
        base_tip(dir, owner, name, base_ref.trim())
    })
}

/// PR の base ブランチの今の先頭（GitHub の値）
fn base_tip(dir: &std::path::Path, owner: &str, name: &str, base_ref: &str) -> Option<String> {
    if base_ref.is_empty() {
        return None;
    }
    gh::gh(
        dir,
        &[
            "api",
            &format!("repos/{owner}/{name}/branches/{}", uri_encode(base_ref)),
            "--jq",
            ".commit.sha",
        ],
    )
    .ok()
    .map(|s| s.trim().to_string())
    .filter(|s| !s.is_empty())
}

/// 失敗していない bot レビューがあるか。Copilot はレビューできなかったときも本文だけの
/// レビューを提出するので、レビューの有無では判定できない（判定は get-latest-review.sh と同じ）
fn has_successful_review(dir: &std::path::Path, owner: &str, name: &str, number: &str) -> bool {
    gh::gh(
        dir,
        &[
            "api",
            "--paginate",
            &format!("repos/{owner}/{name}/pulls/{number}/reviews"),
            "--jq",
            "[.[] | select(.user.type == \"Bot\") | select((.body // \"\") | test(\"wasn'?t able to review|unable to review|encountered an error\"; \"i\") | not)] | length",
        ],
    )
    .ok()
    // --paginate はページごとに --jq を適用するので、ページごとの件数を足す
    .is_some_and(|s| s.lines().filter_map(|l| l.trim().parse::<u64>().ok()).sum::<u64>() > 0)
}

/// リベースだけで head の hash が変わったか。レビュー済みの `old` と今の `new` で、base からの PR の差分が
/// 同一なら true（ADR-0021）。`base` は PR の base ブランチの今の先頭の SHA。base 側の変更（main に入った他の PR）は、それぞれの merge-base からの
/// 差分を取ることで含めない。確かめられないとき（コミットが無い、git が失敗）は false
fn same_pr_diff(dir: &std::path::Path, base: &str, old: &str, new: &str) -> bool {
    let have = |sha: &str| git(dir, &["cat-file", "-e", &format!("{sha}^{{commit}}")]).is_some();
    // force push で置き換わった旧 head は、リベースした worktree 以外には無いことがある
    if !have(old) && (git(dir, &["fetch", "-q", "origin", old]).is_none() || !have(old)) {
        return false;
    }
    // base は GitHub が返した今の先頭。手元の origin/<base> は古いことがあり、古い base では
    // main の変更を巻き戻した head も、両側の merge-base が同じになって一致して見える
    if !have(base) && (git(dir, &["fetch", "-q", "origin", base]).is_none() || !have(base)) {
        return false;
    }
    // 新 head も、別の clone から push して手元に無いことがある（local-review-covers は PR の worktree 以外でも走る）
    if !have(new) && (git(dir, &["fetch", "-q", "origin", new]).is_none() || !have(new)) {
        return false;
    }
    let (Some(old_base), Some(new_base)) = (
        git(dir, &["merge-base", base, old]),
        git(dir, &["merge-base", base, new]),
    ) else {
        return false;
    };
    // 1. PR が触るファイルの中身（モードを含む）がそっくり同じ
    let paths = |b: &str, h: &str| {
        git(dir, &["diff", "--no-renames", "--name-only", "-z", b, h]).unwrap_or_default()
    };
    let (old_paths, new_paths) = (paths(&old_base, old), paths(&new_base, new));
    let files: std::collections::BTreeSet<&str> = old_paths
        .split('\0')
        .chain(new_paths.split('\0'))
        .filter(|p| !p.is_empty())
        .collect();
    if files.is_empty() {
        return false;
    }
    // base 側がそのファイルを変えていないことも要る。変えていると、旧 head が触れなかった
    // ファイルを新 head が main の変更ごと元に戻した場合も、中身が一致して見える
    // ファイル名は pathspec ではなくリテラルに渡す
    let entry = |h: &str, p: &str| git(dir, &["--literal-pathspecs", "ls-tree", h, "--", p]);
    if files.iter().all(|p| {
        let (o, n) = (entry(old, p), entry(new, p));
        o.is_some() && o == n && entry(&old_base, p) == entry(&new_base, p)
    }) {
        return true;
    }
    // 2. main も同じファイルを変えていて中身は一致しないが、PR の差分（前後の文脈を含む）が同じ
    fingerprint(dir, &old_base, old).is_some_and(|o| Some(o) == fingerprint(dir, &new_base, new))
}

/// base から head への差分の指紋。パッチの内容（git patch-id。行番号は見ない）と、
/// ファイルごとのモード・状態・パス（patch-id が見ない部分）
fn fingerprint(dir: &std::path::Path, base: &str, head: &str) -> Option<(String, String)> {
    let patch = git_stdout(dir, &["diff", "--no-renames", "--binary", base, head])?;
    if patch.is_empty() {
        return None;
    }
    let id = git_with_stdin(dir, &["patch-id", "--verbatim"], &patch)?;
    let id = id.split_whitespace().next()?.to_string();
    let raw = git(
        dir,
        &["diff", "--no-renames", "--raw", "--no-abbrev", base, head],
    )?;
    let modes = raw
        .lines()
        .map(|l| {
            let (meta, path) = l.split_once('\t').unwrap_or((l, ""));
            let f: Vec<&str> = meta.split_whitespace().collect();
            // :旧モード 新モード 旧hash 新hash 状態。hash は base で変わるので捨てる
            format!(
                "{} {} {} {path}",
                f[0],
                f.get(1).unwrap_or(&""),
                f.get(4).unwrap_or(&"")
            )
        })
        .collect::<Vec<_>>()
        .join("\n");
    Some((id, modes))
}

fn git_stdout(dir: &std::path::Path, args: &[&str]) -> Option<Vec<u8>> {
    let out = crate::git::run(dir, args, None)?;
    out.status.success().then_some(out.stdout)
}

fn git_with_stdin(dir: &std::path::Path, args: &[&str], input: &[u8]) -> Option<String> {
    let out = crate::git::run(dir, args, Some(input))?;
    out.status
        .success()
        .then(|| String::from_utf8_lossy(&out.stdout).to_string())
}

fn is_ok_conclusion(c: &Value) -> bool {
    matches!(c.as_str(), Some("success" | "neutral" | "skipped"))
}

impl Rule for PreMergeCheck {
    fn name(&self) -> &'static str {
        "pre-merge-check"
    }

    fn check(&self, input: &Input, shell: &Shell) -> Vec<Finding> {
        // 2 つの検証（8 と 9）は別々のエスケープで外すので、それぞれの理由に当てる
        for cmd in shell.commands() {
            let Some(args) = cmd.gh_subcommand(&["pr", "merge"]) else {
                continue;
            };
            let mut reasons: Vec<String> = Vec::new();

            let mut dir = shell.target_dir(cmd, &input.cwd);
            if !dir.is_dir() {
                // 手元の cwd に戻して続ける検査は、接続先や対象の利用者の PR に当たらないので混ぜない
                if shell.runs_under_ssh_or_login(cmd) {
                    return vec![Finding::Deny(
                        "ssh や sudo -i の中で走る gh は、手元の場所で検査できないため止めます"
                            .to_string(),
                    )];
                }
                reasons.push(format!(
                    "コマンド中の cd 先に移動できません: {}",
                    dir.display()
                ));
                dir = input.cwd.clone();
            }

            let repo = opt_value(args, &["-R", "--repo"]).map(|a| {
                a.text
                    .strip_prefix("--repo=")
                    .or_else(|| a.text.strip_prefix("-R="))
                    .unwrap_or(&a.text)
                    .to_string()
            });
            let mut repo_args: Vec<&str> = Vec::new();
            if let Some(r) = repo.as_deref() {
                repo_args.extend(["-R", r]);
            }
            let pr_ref = pr_ref(args);

            // --- 1. マージ方式 ---
            if has_flag(args, &["--squash", "-s", "--rebase", "-r"]) {
                reasons.push(
                    "--squash / --rebase は禁止です。--merge（マージコミット方式）を使ってください"
                        .to_string(),
                );
            } else if !has_flag(args, &["--merge", "-m"]) {
                reasons.push(
                    "--merge を明示してください（マージ方式の既定値に依存しない）".to_string(),
                );
            }

            // --- 2. ブランチ削除 ---
            if !has_flag(args, &["--delete-branch", "-d"]) {
                reasons.push(
                    "--delete-branch を付けてください（マージ後のブランチは速やかに削除する）"
                        .to_string(),
                );
            }

            // --- 3. 本文形式 ---
            match body_text(args, &["--body", "-b"], &["--body-file", "-F"]) {
                Err(path) => reasons.push(format!("--body-file のファイルが読めません: {path}")),
                Ok(None) => reasons.push(
                    "--body でマージコミット本文を指定してください（## Why / ## What / ## Impact）"
                        .to_string(),
                ),
                Ok(Some(body)) => {
                    let missing = missing_headings(&body, &["## Why", "## What", "## Impact"]);
                    if !missing.is_empty() {
                        reasons.push(format!(
                            "マージコミット本文に見出しがありません: {}",
                            missing.join(" ")
                        ));
                    }
                }
            }

            // 対象リポジトリ（base）。check-run と review thread はどちらも base 側に紐づく
            let (owner, name) = match repo.as_deref() {
                Some(r) => {
                    let (o, n) = r.split_once('/').unwrap_or((r, ""));
                    (o.to_string(), n.trim_end_matches(".git").to_string())
                }
                None => (
                    gh::gh(
                        &dir,
                        &["repo", "view", "--json", "owner", "--jq", ".owner.login"],
                    )
                    .unwrap_or_default(),
                    gh::gh(&dir, &["repo", "view", "--json", "name", "--jq", ".name"])
                        .unwrap_or_default(),
                ),
            };

            // ここまで（1〜3 と cd 先）はコマンドの文字列だけの判定で、前のコマンドの実行に左右されない
            let static_reasons = reasons.len();

            // --- 4. CI チェック ---
            let mut view_args: Vec<&str> = vec!["pr", "view"];
            view_args.extend(repo_args.iter().copied());
            if let Some(r) = pr_ref.as_deref() {
                view_args.push(r);
            }
            view_args.extend(["--json", "number,headRefOid,reviewDecision,baseRefName"]);
            let pr_meta: Option<Value> = gh::gh(&dir, &view_args)
                .ok()
                .and_then(|s| serde_json::from_str(&s).ok());

            let mut head_sha = String::new();
            let mut copilot_check_failed = false;
            if !owner.is_empty()
                && !name.is_empty()
                && let Some(meta) = pr_meta.as_ref()
            {
                head_sha = meta["headRefOid"].as_str().unwrap_or("").to_string();
                let check_runs = gh::gh(
                    &dir,
                    &[
                        "api",
                        "--paginate",
                        &format!("repos/{owner}/{name}/commits/{head_sha}/check-runs?per_page=100"),
                        "--jq",
                        ".check_runs[] | {name, status, conclusion}",
                    ],
                )
                .ok()
                .and_then(|s| json_stream(&s));
                let statuses = gh::gh(
                    &dir,
                    &[
                        "api",
                        &format!("repos/{owner}/{name}/commits/{head_sha}/status"),
                        "--jq",
                        "[.statuses[] | {name: .context, status: (if .state == \"pending\" then \"in_progress\" else \"completed\" end), conclusion: .state}]",
                    ],
                )
                .ok()
                .and_then(|s| serde_json::from_str::<Value>(&s).ok())
                .and_then(|v| v.as_array().cloned());
                match (check_runs, statuses) {
                    (Some(runs), Some(st)) => {
                        let all: Vec<Value> = runs.into_iter().chain(st).collect();
                        let pending: Vec<&Value> =
                            all.iter().filter(|c| c["status"] != "completed").collect();
                        if !pending.is_empty() {
                            let mut names: Vec<String> = pending
                                .iter()
                                .map(|c| c["name"].as_str().unwrap_or("").to_string())
                                .collect();
                            names.sort();
                            names.dedup();
                            reasons.push(format!(
                                "実行中/待機中のチェックがあります ({}件): {}",
                                pending.len(),
                                names.join(", ")
                            ));
                        }
                        let all_failed: Vec<&Value> = all
                            .iter()
                            .filter(|c| {
                                c["status"] == "completed" && !is_ok_conclusion(&c["conclusion"])
                            })
                            .collect();
                        // Copilot のレビュー実行の失敗はコードの問題ではないので CI 失敗に
                        // 数えず、検証 8 でレビュー未実施として扱う
                        let is_copilot = |c: &&Value| {
                            c["name"]
                                .as_str()
                                .unwrap_or("")
                                .to_ascii_lowercase()
                                .contains("copilot")
                        };
                        copilot_check_failed = all_failed.iter().any(is_copilot);
                        let failed: Vec<&Value> =
                            all_failed.into_iter().filter(|c| !is_copilot(c)).collect();
                        if !failed.is_empty() {
                            let mut names: Vec<String> = failed
                                .iter()
                                .map(|c| {
                                    format!(
                                        "{} ({})",
                                        c["name"].as_str().unwrap_or(""),
                                        c["conclusion"].as_str().unwrap_or("null")
                                    )
                                })
                                .collect();
                            names.sort();
                            names.dedup();
                            reasons.push(format!(
                                "失敗したチェックがあります ({}件): {}",
                                failed.len(),
                                names.join(", ")
                            ));
                        }
                    }
                    _ => reasons.push(
                        "CI チェック状態を取得できません（check-runs / status API が失敗）"
                            .to_string(),
                    ),
                }
            } else {
                reasons.push("PR またはリポジトリの情報を取得できません（gh pr view / gh repo view が失敗。PR番号・リポジトリ指定・認証・ネットワークを確認）".to_string());
            }

            // --- 5. レビュー判定 ---
            let decision = pr_meta
                .as_ref()
                .and_then(|m| m["reviewDecision"].as_str())
                .unwrap_or("")
                .to_string();
            if decision == "CHANGES_REQUESTED" {
                reasons.push("レビューで変更が要求されています (CHANGES_REQUESTED)".to_string());
            }
            if decision == "REVIEW_REQUIRED" {
                reasons.push("必須レビューが未完了です (REVIEW_REQUIRED)".to_string());
            }

            // --- 6. 未解決レビュースレッド ---
            // Copilot のレビューは COMMENTED で提出され reviewDecision を変えないため、
            // 指摘への対応漏れはスレッドの resolve 状態で判定する
            let pr_number = pr_ref.clone().or_else(|| {
                pr_meta
                    .as_ref()
                    .and_then(|m| m["number"].as_u64())
                    .map(|n| n.to_string())
            });
            match pr_number.as_deref() {
                None => reasons.push("対象 PR を特定できません（PR番号を指定するか、PR のあるブランチで実行してください）".to_string()),
                Some(number) => {
                    let unresolved = gh::gh(
                        &dir,
                        &[
                            "api",
                            "graphql",
                            "--paginate",
                            "-F",
                            &format!("owner={owner}"),
                            "-F",
                            &format!("name={name}"),
                            "-F",
                            &format!("number={number}"),
                            "-f",
                            "query=\n    query($owner: String!, $name: String!, $number: Int!, $endCursor: String) {\n      repository(owner: $owner, name: $name) {\n        pullRequest(number: $number) {\n          reviewThreads(first: 100, after: $endCursor) {\n            pageInfo { hasNextPage endCursor }\n            nodes { isResolved path }\n          }\n        }\n      }\n    }",
                            "--jq",
                            "[.data.repository.pullRequest.reviewThreads.nodes[] | select(.isResolved | not) | .path]",
                        ],
                    )
                    .ok()
                    .and_then(|s| json_stream(&s))
                    .and_then(|pages| {
                        // ページごとの配列を 1 つにまとめる。配列以外が混じれば失敗扱い
                        let mut paths: Vec<String> = Vec::new();
                        for p in pages {
                            let arr = p.as_array()?;
                            paths.extend(arr.iter().filter_map(|v| v.as_str().map(str::to_string)));
                        }
                        Some(paths)
                    });
                    match unresolved {
                        None => reasons.push("未解決レビュースレッドを確認できませんでした（gh api graphql が失敗）".to_string()),
                        Some(paths) if !paths.is_empty() => {
                            let mut uniq = paths.clone();
                            uniq.sort();
                            uniq.dedup();
                            reasons.push(format!(
                                "未解決のレビュースレッドがあります ({}件): {}。対応して返信し、resolve-thread.sh で resolve してください",
                                paths.len(),
                                uniq.join(", ")
                            ));
                        }
                        Some(_) => {}
                    }
                }
            }

            let base_ref = pr_meta
                .as_ref()
                .and_then(|m| m["baseRefName"].as_str())
                .unwrap_or("")
                .to_string();

            // --- 7. base からの遅れ ---
            if !owner.is_empty() && !name.is_empty() && pr_meta.is_some() {
                let behind = if base_ref.is_empty() || head_sha.is_empty() {
                    None
                } else {
                    gh::gh(
                        &dir,
                        &[
                            "api",
                            &format!(
                                "repos/{owner}/{name}/compare/{}...{head_sha}",
                                uri_encode(&base_ref)
                            ),
                            "--jq",
                            ".behind_by",
                        ],
                    )
                    .ok()
                    .and_then(|s| s.trim().parse::<u64>().ok())
                };
                match behind {
                    None => reasons.push("base ブランチとの差を確認できません（compare API が失敗）".to_string()),
                    Some(n) if n > 0 => reasons.push(format!(
                        "head が {base_ref} より {n} コミット遅れています。origin/{base_ref} にリベースして --force-with-lease で push し直してからマージしてください"
                    )),
                    Some(_) => {}
                }
            }

            // --- 8. 最後の push がレビュー済みか ---
            // push だけでは再レビューは走らない（requested_reviewers での依頼が要る）。
            // 依頼を忘れるとレビューされていない版をマージできてしまうため、bot が
            // レビューした commit と head SHA の一致で機械的に確かめる。
            // bot レビューが 1 件も無いリポジトリ（自動レビュー未設定）では検査しない
            if !owner.is_empty()
                && !name.is_empty()
                && !head_sha.is_empty()
                && shell.escape("ALLOW_UNREVIEWED_HEAD") != Escape::Given
                && let Some(number) = pr_number.as_deref()
            {
                let mut unreviewed: Vec<String> = Vec::new();
                // base の先頭（API）は、bot のレビューと記録のどちらの比較元にも使うので 1 回だけ取る
                let base_tip_cell = std::cell::OnceCell::new();
                let tip = || {
                    base_tip_cell
                        .get_or_init(|| base_tip(&dir, &owner, &name, &base_ref))
                        .clone()
                };
                let reviewed = gh::gh(
                    &dir,
                    &[
                        "api",
                        "--paginate",
                        &format!("repos/{owner}/{name}/pulls/{number}/reviews"),
                        "--jq",
                        "[.[] | select(.user.type == \"Bot\") | .commit_id] | last // \"\"",
                    ],
                )
                .ok()
                // --paginate はページごとに --jq を適用するため複数行になりうる。
                // レビューは提出順に返るので最後の非空行が最新の bot レビュー
                .map(|s| {
                    s.lines()
                        .map(str::trim)
                        .rfind(|l| !l.is_empty())
                        .unwrap_or("")
                        .to_string()
                });
                match reviewed {
                    None => unreviewed.push(
                        "自動レビューの対象コミットを確認できませんでした（reviews API が失敗）"
                            .to_string(),
                    ),
                    // レビューが 1 件も無い場合、通常は「自動レビューが無いリポジトリ」なので
                    // 検査しない。ただしチェックが失敗しているなら初回レビューが走らなかった
                    // ということなので、下の分岐でレビュー未実施として報告する
                    Some(sha) if sha.is_empty() && !copilot_check_failed => {}
                    // 文書だけの PR は一度レビューを受けていれば足りる。後のレビューが失敗していても止めない
                    Some(sha)
                        if !sha.is_empty()
                            && sha != head_sha
                            && is_docs_only(&dir, &owner, &name, number)
                            && has_successful_review(&dir, &owner, &name, number) => {}
                    // リベースだけで hash が変わった head は、レビュー済みの差分と同じなので通す（ADR-0021）
                    Some(sha)
                        if !sha.is_empty()
                            && sha != head_sha
                            && tip()
                                .is_some_and(|b| same_pr_diff(&dir, &b, &sha, &head_sha)) => {}
                    // head のローカルレビューの記録があれば（記録の SHA から rebase だけした head も。ADR-0059）、Copilot のレビューの有無・到着・チェックの失敗を問わず通す。安い判定（文書だけ、リベースだけ）の後に置き、記録のコメントの取得を最後にする
                    // （ADR-0043）。Copilot の指摘への対応は検証 6（未解決スレッド）が見る
                    Some(sha)
                        if sha != head_sha
                            && local_review_covers(&dir, &owner, &name, number, &head_sha, tip)
                                .unwrap_or(false) => {}
                    Some(sha) if sha != head_sha && copilot_check_failed => unreviewed.push(format!(
                        "最後の push ({}) でレビューが実行されていません（チェックが失敗。レビュー用トークンの枯渇や内部エラーが考えられます）。/gh-actions-check https://github.com/{owner}/{name}/pull/{number} で原因を確認し、head のローカルレビューを記録して（record-local-review.sh）ください。{}",
                        short(&head_sha),
                        escape_hint("ALLOW_UNREVIEWED_HEAD")
                    )),
                    Some(sha) if sha != head_sha => unreviewed.push(format!(
                        "最後の push ({}) は自動レビューを受けていません（自動レビュー済み: {}）。リベースだけ（base からの PR の差分が、自動レビューか最後のローカルレビューの記録と同じ）なら通りますが、同じとは確かめられませんでした（差分が変わった、旧 head や origin/{base_ref} が手元にない等）。push だけでは再レビューは走りません。次の一手（ローカルレビューの記録か、~/.claude/skills/gh-pr-review/scripts/request-rereview.sh https://github.com/{owner}/{name}/pull/{number} での再レビューの依頼）は ~/.claude/skills/gh-pr-review/scripts/decide-next.sh https://github.com/{owner}/{name}/pull/{number} で決め、指摘に対応してからマージしてください。{}",
                        short(&head_sha),
                        short(&sha),
                        escape_hint("ALLOW_UNREVIEWED_HEAD")
                    )),
                    Some(_) => {}
                }
                reasons.extend(apply_escape_to_reasons(
                    shell.escape("ALLOW_UNREVIEWED_HEAD"),
                    "ALLOW_UNREVIEWED_HEAD",
                    unreviewed,
                ));
            }

            // --- 9. トピックが 1 つか（新しい ADR の数）---
            // 変更ファイルの status が added のものだけ数える（既存の ADR の変更・改名は数えない）
            if !owner.is_empty()
                && !name.is_empty()
                && shell.escape("ALLOW_MULTI_TOPIC") != Escape::Given
                && let Some(number) = pr_number.as_deref()
            {
                match gh::gh(
                    &dir,
                    &[
                        "api",
                        "--paginate",
                        &format!("repos/{owner}/{name}/pulls/{number}/files"),
                        "--jq",
                        ".[] | [.status, .filename] | @tsv",
                    ],
                ) {
                    Err(()) => reasons.extend(apply_escape_to_reasons(
                        shell.escape("ALLOW_MULTI_TOPIC"),
                        "ALLOW_MULTI_TOPIC",
                        vec![
                            "PR の変更ファイルを取得できず、新しい ADR の数を確認できません（pulls/files API が失敗）"
                                .to_string(),
                        ],
                    )),
                    Ok(s) => {
                        let adrs: Vec<String> = s
                            .lines()
                            .filter_map(|l| l.split_once('\t'))
                            .filter(|(status, path)| *status == "added" && is_adr_path(path))
                            .map(|(_, path)| path.to_string())
                            .collect();
                        reasons.extend(apply_escape_to_reasons(
                            shell.escape("ALLOW_MULTI_TOPIC"),
                            "ALLOW_MULTI_TOPIC",
                            multi_topic_reason(&adrs).into_iter().collect(),
                        ));
                    }
                }
            }

            if !reasons.is_empty() {
                // 診断は cwd のリポジトリで読むので、PR が分かれば URL で渡す（ADR-0032）
                let target = match pr_number.as_deref() {
                    Some(number) if !owner.is_empty() && !name.is_empty() => {
                        format!(" https://github.com/{owner}/{name}/pull/{number}")
                    }
                    _ => String::new(),
                };
                return vec![Finding::Deny(format!(
                    "{}{}\n\n/gh-actions-check{target} で状況を診断してください。",
                    reasons.join("\n"),
                    if reasons.len() > static_reasons {
                        earlier_commands_hint(shell, cmd)
                    } else {
                        String::new()
                    }
                ))];
            }
        }
        Vec::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::shell::Shell;

    #[test]
    fn pr_ref_skips_option_values_and_stops_at_body() {
        let cs = Shell::parse("gh pr merge -R owner/repo 12 --merge --body \"PR 99\"")
            .commands()
            .to_vec();
        assert_eq!(
            pr_ref(cs[0].gh_subcommand(&["pr", "merge"]).unwrap()).as_deref(),
            Some("12")
        );
        let cs = Shell::parse("gh pr merge --merge --subject 'x 7'")
            .commands()
            .to_vec();
        assert_eq!(pr_ref(cs[0].gh_subcommand(&["pr", "merge"]).unwrap()), None);
    }

    #[test]
    fn uri_encode_matches_jq_uri() {
        assert_eq!(uri_encode("release/x"), "release%2Fx");
        assert_eq!(uri_encode("main"), "main");
    }

    #[test]
    fn json_stream_reads_concatenated_values() {
        let v = json_stream("{\"a\":1}\n{\"a\":2}\n").unwrap();
        assert_eq!(v.len(), 2);
        assert!(json_stream("not json").is_none());
    }

    #[test]
    fn rebase_source_is_the_last_record_when_it_has_no_high() {
        let marks = "<!-- local-review sha=aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa high=1 medium=0 low=0 -->\n\
                     <!-- local-review sha=bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb high=0 medium=2 low=1 -->";
        assert_eq!(
            local_review_rebase_source(marks),
            Some("bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb")
        );
    }

    #[test]
    fn rebase_source_is_none_when_the_last_record_has_high() {
        // 同じ差分を回し直して見つけた High を、前の High の無い記録で隠さない
        let marks = "<!-- local-review sha=aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa high=0 medium=0 low=0 -->\n\
                     <!-- local-review sha=bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb high=1 medium=0 low=0 -->";
        assert_eq!(local_review_rebase_source(marks), None);
        let marks = "<!-- local-review sha=aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa high=10 medium=0 low=0 -->";
        assert_eq!(local_review_rebase_source(marks), None);
    }

    #[test]
    fn rebase_source_is_none_when_the_last_record_is_malformed() {
        // 形は record-local-review.sh が書く完全な 16 進の小文字の SHA。短い SHA は別のコミットに解けうる
        for sha in [
            "CCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCC",
            "abc1234",
            "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa0",
            "",
        ] {
            let marks = format!(
                "<!-- local-review sha=aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa high=0 medium=0 low=0 -->\n\
                 <!-- local-review sha={sha} high=0 medium=0 low=0 -->"
            );
            assert_eq!(local_review_rebase_source(&marks), None, "{sha}");
        }
    }

    #[test]
    fn rebase_source_is_none_without_records() {
        assert_eq!(local_review_rebase_source(""), None);
    }

    /// 一時ディレクトリの git リポジトリ。origin/main は refs/remotes/origin/main を直に置いて作る
    struct Repo(std::path::PathBuf);

    impl Repo {
        fn new(tag: &str) -> Repo {
            let dir = std::env::temp_dir().join(format!(
                "claude-hooks-same-pr-diff-{tag}-{}",
                std::process::id()
            ));
            let _ = std::fs::remove_dir_all(&dir);
            std::fs::create_dir_all(&dir).unwrap();
            let r = Repo(dir);
            r.git(&["init", "-q", "-b", "main"]);
            r.git(&["config", "user.email", "t@example.com"]);
            r.git(&["config", "user.name", "t"]);
            r.git(&["config", "commit.gpgsign", "false"]);
            r.git(&["config", "core.hooksPath", "/dev/null"]);
            r
        }

        fn git(&self, args: &[&str]) -> String {
            crate::git::git(&self.0, args).unwrap_or_else(|| panic!("git {args:?} failed"))
        }

        fn write(&self, path: &str, body: &str) {
            std::fs::write(self.0.join(path), body).unwrap();
        }

        fn commit(&self, msg: &str) -> String {
            self.git(&["add", "-A"]);
            self.git(&["commit", "-q", "-m", msg]);
            self.git(&["rev-parse", "HEAD"])
        }

        /// origin/main を今の HEAD に合わせる
        fn publish_main(&self) {
            let head = self.git(&["rev-parse", "HEAD"]);
            self.git(&["update-ref", "refs/remotes/origin/main", &head]);
        }
    }

    impl Drop for Repo {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    const LONG: &str = "a1\na2\na3\na4\na5\na6\na7\na8\na9\na10\na11\na12\na13\na14\na15\n";

    /// 初期状態（main）に PR のコミットを積み、old とする。main を進めてから PR を載せ替えた new を返す
    /// `pr_edit` は PR の変更、`main_edit` は main が進む内容、`redo` は載せ替えた先で PR を作り直す関数
    fn rebase_case(
        tag: &str,
        pr_edit: impl Fn(&Repo),
        main_edit: impl Fn(&Repo),
        redo: impl Fn(&Repo),
    ) -> (Repo, String, String) {
        let r = Repo::new(tag);
        r.write("long.txt", LONG);
        r.write("other.txt", "o\n");
        r.commit("base");
        r.publish_main();
        r.git(&["checkout", "-q", "-b", "pr"]);
        pr_edit(&r);
        let old = r.commit("pr");
        r.git(&["checkout", "-q", "main"]);
        main_edit(&r);
        r.commit("main moves");
        r.publish_main();
        r.git(&["checkout", "-q", "-b", "pr2"]);
        redo(&r);
        let new = r.commit("pr again");
        (r, old, new)
    }

    fn pr_edit_other(r: &Repo) {
        r.write("other.txt", "o\nPR\n");
    }

    #[test]
    fn rebase_onto_unrelated_change_is_same() {
        let (r, old, new) = rebase_case(
            "unrelated",
            pr_edit_other,
            |r| r.write("third.txt", "t\n"),
            pr_edit_other,
        );
        assert!(same_pr_diff(
            &r.0,
            &r.git(&["rev-parse", "main"]),
            &old,
            &new
        ));
    }

    #[test]
    fn rebase_onto_change_in_same_file_elsewhere_is_same() {
        // main も同じファイルの先頭を変える。PR は末尾（文脈は変わらない）
        let edit_tail = |r: &Repo| r.write("long.txt", &format!("{LONG}tail\n"));
        let (r, old, new) = rebase_case(
            "samefile",
            edit_tail,
            |r| r.write("long.txt", &LONG.replacen("a1\n", "A1\n", 1)),
            |r| {
                r.write(
                    "long.txt",
                    &format!("{}tail\n", LONG.replacen("a1\n", "A1\n", 1)),
                )
            },
        );
        assert!(same_pr_diff(
            &r.0,
            &r.git(&["rev-parse", "main"]),
            &old,
            &new
        ));
    }

    #[test]
    fn changed_content_is_not_same() {
        let (r, old, new) = rebase_case(
            "changed",
            pr_edit_other,
            |r| r.write("third.txt", "t\n"),
            |r| r.write("other.txt", "o\nPR\nEXTRA\n"),
        );
        assert!(!same_pr_diff(
            &r.0,
            &r.git(&["rev-parse", "main"]),
            &old,
            &new
        ));
    }

    #[test]
    fn dropped_change_is_not_same() {
        let (r, old, new) = rebase_case(
            "dropped",
            pr_edit_other,
            |r| r.write("third.txt", "t\n"),
            |r| r.write("third2.txt", "unrelated\n"),
        );
        assert!(!same_pr_diff(
            &r.0,
            &r.git(&["rev-parse", "main"]),
            &old,
            &new
        ));
    }

    #[test]
    fn same_lines_at_another_place_in_a_file_main_also_changed_is_not_same() {
        // 追加行は同じだが、置き場所が違う（コンフリクト解決で動かした）
        let (r, old, new) = rebase_case(
            "moved",
            |r| r.write("long.txt", &LONG.replacen("a3\n", "a3\nPR\n", 1)),
            |r| r.write("long.txt", &LONG.replacen("a1\n", "A1\n", 1)),
            |r| {
                r.write(
                    "long.txt",
                    &LONG
                        .replacen("a1\n", "A1\n", 1)
                        .replacen("a12\n", "a12\nPR\n", 1),
                )
            },
        );
        assert!(!same_pr_diff(
            &r.0,
            &r.git(&["rev-parse", "main"]),
            &old,
            &new
        ));
    }

    #[test]
    fn extra_file_in_new_is_not_same() {
        let (r, old, new) = rebase_case(
            "extrafile",
            pr_edit_other,
            |r| r.write("third.txt", "t\n"),
            |r| {
                pr_edit_other(r);
                r.write("sneaky.txt", "x\n");
            },
        );
        assert!(!same_pr_diff(
            &r.0,
            &r.git(&["rev-parse", "main"]),
            &old,
            &new
        ));
    }

    #[test]
    fn mode_change_is_not_same() {
        let (r, old, new) = rebase_case(
            "mode",
            pr_edit_other,
            |r| r.write("third.txt", "t\n"),
            |r| {
                pr_edit_other(r);
                use std::os::unix::fs::PermissionsExt;
                std::fs::set_permissions(
                    r.0.join("other.txt"),
                    std::fs::Permissions::from_mode(0o755),
                )
                .unwrap();
            },
        );
        assert!(!same_pr_diff(
            &r.0,
            &r.git(&["rev-parse", "main"]),
            &old,
            &new
        ));
    }

    #[test]
    fn unknown_old_commit_is_not_same() {
        let (r, _old, new) = rebase_case(
            "unknown",
            pr_edit_other,
            |r| r.write("third.txt", "t\n"),
            pr_edit_other,
        );
        let bogus = "0123456789012345678901234567890123456789";
        let tip = r.git(&["rev-parse", "main"]);
        assert!(!same_pr_diff(&r.0, &tip, bogus, &new));
    }

    #[test]
    fn reverting_main_change_to_untouched_file_is_not_same() {
        // 旧 head は other.txt に触れない。main が変えた other.txt を、新 head が元に戻す
        let (r, old, new) = rebase_case(
            "revert-main",
            |r| r.write("pr.txt", "pr\n"),
            |r| r.write("other.txt", "o\nMAIN\n"),
            |r| {
                r.write("pr.txt", "pr\n");
                r.write("other.txt", "o\n");
            },
        );
        assert!(!same_pr_diff(
            &r.0,
            &r.git(&["rev-parse", "main"]),
            &old,
            &new
        ));
    }

    #[test]
    fn discarding_main_change_in_touched_file_is_not_same() {
        let (r, old, new) = rebase_case(
            "discard-main",
            |r| r.write("other.txt", "o\nPR\n"),
            |r| r.write("other.txt", "MAIN\no\n"),
            |r| r.write("other.txt", "o\nPR\n"),
        );
        assert!(!same_pr_diff(
            &r.0,
            &r.git(&["rev-parse", "main"]),
            &old,
            &new
        ));
    }

    #[test]
    fn stale_base_does_not_hide_reverting_main_change() {
        // 旧 head は A→B。main は A→C。新 head は C→B（main の変更を巻き戻す）。
        // 手元の origin/main が A のままでも、GitHub の今の先頭 C を base にして見分ける
        let r = Repo::new("stale");
        r.write("f.txt", "A\n");
        r.commit("base");
        r.publish_main();
        r.git(&["checkout", "-q", "-b", "pr"]);
        r.write("f.txt", "B\n");
        let old = r.commit("pr");
        r.git(&["checkout", "-q", "main"]);
        r.write("f.txt", "C\n");
        let tip = r.commit("main moves");
        r.git(&["checkout", "-q", "-b", "pr2"]);
        r.write("f.txt", "B\n");
        let new = r.commit("pr again");
        assert!(!same_pr_diff(&r.0, &tip, &old, &new));
    }

    #[test]
    fn pathspec_like_file_name_is_compared_literally() {
        let name = ":(exclude,glob)**";
        let (r, old, new) = rebase_case(
            "pathspec",
            |r| r.write(name, "one\n"),
            |r| r.write("third.txt", "t\n"),
            |r| r.write(name, "two\n"),
        );
        assert!(!same_pr_diff(
            &r.0,
            &r.git(&["rev-parse", "main"]),
            &old,
            &new
        ));
    }
}
