//! `claude-hooks script record-local-review <PR の URL> <SHA> <high> <medium> <low>` が、ローカルの /code-review を
//! 回した PR の head の記録を、マージの hook（pre-merge-check）が数える印のコメントとして PR に付けることを確かめる
//! （ADR-0087。`~/.claude/skills/gh-pr-review/scripts/record-local-review.sh` が呼ぶ）。記録はマージの hook が Copilot
//! のレビューの代わりに数えるので、head でない SHA、push できないリポジトリ、形の崩れた件数では付けない。その head で
//! /code-review を回した記録（count-review-rounds の hook が clone に書く行）が、起動したディレクトリの clone に無いときも付けない
//! （hook が書かない回し方をしたときは、`--unlogged <理由>` で理由を記録に残して付ける）

mod common;

use common::fake_gh::{FakeGh, stderr};
use common::forget_git_env;

const URL: &str = "https://github.com/octo/repo/pull/42";
const HEAD: &str = "def5678def5678def5678def5678def5678def56";

fn stdout(out: &std::process::Output) -> String {
    String::from_utf8_lossy(&out.stdout).into_owned()
}

/// 起動するディレクトリを git のリポジトリにし、rounds の HEAD で /code-review を回した記録を置く（count-review-rounds が
/// 書く形: 共通の git ディレクトリの claude-review-rounds/<ブランチ名> に、1 行が `<HEAD> <UNIX 秒>`）
fn reviewed_at(fake: &FakeGh, rounds: &[&str]) {
    let mut cmd = std::process::Command::new("git");
    let ok = forget_git_env(&mut cmd)
        .args(["init", "-q"])
        .current_dir(fake.dir())
        .env("GIT_CONFIG_GLOBAL", fake.dir().join("no-global-config"))
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .status()
        .unwrap()
        .success();
    assert!(ok, "git init");
    let dir = fake.dir().join(".git/claude-review-rounds");
    std::fs::create_dir_all(&dir).unwrap();
    let text: String = rounds.iter().map(|h| format!("{h} 1794900000\n")).collect();
    std::fs::write(dir.join("feat%2Fx"), text).unwrap();
}

/// HEAD で /code-review を回した clone から起動する
fn run(fake: &FakeGh, args: &[&str], push: &str) -> std::process::Output {
    reviewed_at(fake, &[HEAD]);
    run_as_is(fake, args, push)
}

fn run_as_is(fake: &FakeGh, args: &[&str], push: &str) -> std::process::Output {
    fake.run_calls(
        "record-local-review",
        args,
        &[
            (&format!("{HEAD}\n"), 0),
            (push, 0),
            ("https://github.com/octo/repo/pull/42#issuecomment-1\n", 0),
        ],
    )
}

#[test]
fn comments_the_record_of_the_head_on_the_pr() {
    let fake = FakeGh::new();
    let out = run(&fake, &[URL, "def5678", "1", "2", "0"], "true\n");
    assert_eq!(out.status.code(), Some(0), "{}", stderr(&out));
    assert_eq!(
        fake.log(),
        format!(
            "gh pr view 42 --json headRefOid -q .headRefOid\n\
             gh api repos/{{owner}}/{{repo}} --jq .permissions.push\n\
             gh pr comment 42 --body <!-- local-review sha={HEAD} high=1 medium=2 low=0 -->\n\
             ローカルの /code-review で def5678 を確かめた: High 1 / Medium 2 / Low 0\n"
        )
    );
    assert_eq!(
        fake.env(),
        "GH_REPO=octo/repo\nGH_HOST=github.com\n".repeat(3)
    );
    assert_eq!(
        stdout(&out),
        format!(
            "https://github.com/octo/repo/pull/42#issuecomment-1\nRECORDED: {HEAD}\n\
             NOTE: High の残る記録は、レビュー済みとして数えない。直して push し、回し直して記録する\n"
        )
    );
}

#[test]
fn a_record_without_high_has_no_note() {
    let fake = FakeGh::new();
    let out = run(&fake, &[URL, HEAD, "0", "3", "1"], "true\n");
    assert_eq!(out.status.code(), Some(0), "{}", stderr(&out));
    assert!(
        stdout(&out).ends_with(&format!("RECORDED: {HEAD}\n")),
        "{}",
        stdout(&out)
    );
}

#[test]
fn a_sha_that_is_not_the_head_records_nothing() {
    // レビューの後に head が動いたなら、その head はレビューしていない
    let fake = FakeGh::new();
    let out = run(&fake, &[URL, "abc1234", "0", "0", "0"], "true\n");
    assert_eq!(out.status.code(), Some(1));
    assert!(
        stderr(&out).contains("abc1234 は PR #42 の head（def5678）ではありません"),
        "{}",
        stderr(&out)
    );
    assert!(!fake.log().contains("pr comment"));
}

#[test]
fn a_head_without_a_review_round_in_this_clone_records_nothing() {
    // 直した後の head や rebase の後の head に、回さずに前の周の数で記録を付けない
    const EARLIER: &str = "abc1234abc1234abc1234abc1234abc1234abc12";
    let fake = FakeGh::new();
    reviewed_at(&fake, &[EARLIER]);
    let out = run_as_is(&fake, &[URL, "def5678", "0", "1", "4"], "true\n");
    assert_eq!(out.status.code(), Some(1));
    assert!(
        stderr(&out).contains("def5678 で /code-review を回した記録が"),
        "{}",
        stderr(&out)
    );
    assert!(
        stderr(&out).contains("記録を付けていません"),
        "{}",
        stderr(&out)
    );
    assert!(!fake.log().contains("pr comment"));
}

#[test]
fn outside_a_clone_records_nothing() {
    // 回した記録は clone が持つので、PR のリポジトリの外からは確かめられない。テストの置き場はこのリポジトリの target の
    // 下なので、git が上のリポジトリを見つけないよう、探す範囲を置き場で止める
    let fake = FakeGh::new();
    let ceiling = fake.dir().parent().unwrap().to_string_lossy().into_owned();
    let out = fake.run_calls_with_env(
        "record-local-review",
        &[URL, "def5678", "0", "0", "0"],
        &[(&format!("{HEAD}\n"), 0), ("true\n", 0), ("", 0)],
        &[("GIT_CEILING_DIRECTORIES", &ceiling)],
    );
    assert_eq!(out.status.code(), Some(1));
    assert!(
        stderr(&out).contains("git の共通のディレクトリを引けない"),
        "{}",
        stderr(&out)
    );
    assert!(!fake.log().contains("pr comment"));
}

#[test]
fn a_round_on_any_branch_of_the_clone_counts() {
    // 記録は SHA で引く（ブランチの名前を変えても、同じコミットを回している）。別のブランチの行でも数える
    let fake = FakeGh::new();
    reviewed_at(&fake, &["abc1234abc1234abc1234abc1234abc1234abc12"]);
    std::fs::write(
        fake.dir().join(".git/claude-review-rounds/other"),
        format!("{HEAD} 1794900000\n"),
    )
    .unwrap();
    let out = run_as_is(&fake, &[URL, HEAD, "0", "0", "0"], "true\n");
    assert_eq!(out.status.code(), Some(0), "{}", stderr(&out));
}

#[test]
fn an_unlogged_review_is_recorded_with_its_reason() {
    // hook が回した HEAD を書かない回し方（対象に PR の URL を渡した、detached HEAD、ユーザーが打った /code-review、
    // 回した後の amend）でも、理由を記録に残せば付けられる
    const EARLIER: &str = "abc1234abc1234abc1234abc1234abc1234abc12";
    let fake = FakeGh::new();
    reviewed_at(&fake, &[EARLIER]);
    let out = run_as_is(
        &fake,
        &[
            URL,
            "def5678",
            "0",
            "1",
            "0",
            "--unlogged",
            "対象に PR の URL を渡して回した",
        ],
        "true\n",
    );
    assert_eq!(out.status.code(), Some(0), "{}", stderr(&out));
    assert!(
        fake.log().contains(
            "ローカルの /code-review で def5678 を確かめた: High 0 / Medium 1 / Low 0\n\
             回した記録を確かめていない理由: 対象に PR の URL を渡して回した\n"
        ),
        "{}",
        fake.log()
    );
}

#[test]
fn the_refusal_names_the_clone_it_checked_and_the_way_out() {
    const EARLIER: &str = "abc1234abc1234abc1234abc1234abc1234abc12";
    let fake = FakeGh::new();
    reviewed_at(&fake, &[EARLIER]);
    let out = run_as_is(&fake, &[URL, "def5678", "0", "0", "0"], "true\n");
    let err = stderr(&out);
    assert!(err.contains("claude-review-rounds"), "{err}");
    assert!(err.contains("--unlogged"), "{err}");
    // 中身が変わりうる理由（回した後の amend）と、確かめようのない理由（別の clone）を、逃げ道の例に挙げない
    assert!(!err.contains("amend"), "{err}");
    assert!(!err.contains("別の clone"), "{err}");
}

#[test]
fn the_unlogged_way_out_says_it_needs_no_cd_into_a_worktree() {
    // 対象に PR の URL を渡した周には、cd する worktree が無い。--unlogged の記録は今のディレクトリに頼らない
    const EARLIER: &str = "abc1234abc1234abc1234abc1234abc1234abc12";
    let fake = FakeGh::new();
    reviewed_at(&fake, &[EARLIER]);
    let out = run_as_is(&fake, &[URL, "def5678", "0", "0", "0"], "true\n");
    let err = stderr(&out);
    let unlogged = err
        .lines()
        .find(|l| l.contains("--unlogged"))
        .unwrap_or_else(|| panic!("{err}"));
    assert!(unlogged.contains("cd <worktree> && は要らない"), "{err}");
}

#[test]
fn a_record_with_no_findings_notes_that_the_reply_must_show_the_diff_was_read() {
    // 対象を見つけられなかった、エラーで終わった、`[]` だけの返答の周は未実施で、0 件の記録はレビュー済みに見える
    let fake = FakeGh::new();
    let out = run(&fake, &[URL, HEAD, "0", "0", "0"], "true\n");
    assert_eq!(out.status.code(), Some(0), "{}", stderr(&out));
    let text = stdout(&out);
    let note = text
        .lines()
        .find(|l| l.starts_with("NOTE: "))
        .unwrap_or_else(|| panic!("{text}"));
    assert!(note.contains("`[]`"), "{text}");
    assert!(note.contains("回し直"), "{text}");
    assert!(note.contains("最後の記録"), "{text}");
}

#[test]
fn a_refusal_never_ends_as_a_success() {
    // 回していない head への呼び出しを、記録を足さないまま 0 で終えない（回し直して見つけた High の件数を渡した
    // 呼び出しが、成功に見えて消えないように）。rebase だけの head かは decide-next.sh が示す
    const EARLIER: &str = "abc1234abc1234abc1234abc1234abc1234abc12";
    let fake = FakeGh::new();
    reviewed_at(&fake, &[EARLIER]);
    let out = run_as_is(&fake, &[URL, "def5678", "2", "0", "0"], "true\n");
    assert_eq!(out.status.code(), Some(1));
    assert_eq!(stdout(&out), "");
    assert!(
        stderr(&out).contains("scripts/decide-next.sh"),
        "{}",
        stderr(&out)
    );
    // 確かめるのは clone の行だけで、gh は head と権限の 2 回しか呼ばない
    assert_eq!(fake.log().lines().count(), 2, "{}", fake.log());
}

#[test]
fn the_rebase_hint_is_for_a_head_that_was_not_reviewed_again() {
    // rebase だけの head でも、回し直したなら記録する（見つけた High を、前の head の記録で隠さない）。案内は、回した人が
    // 先に読む順に並べ、rebase の案内は「回し直していない」head に限る
    const EARLIER: &str = "abc1234abc1234abc1234abc1234abc1234abc12";
    let fake = FakeGh::new();
    reviewed_at(&fake, &[EARLIER]);
    let out = run_as_is(
        &fake,
        &[&format!("{URL}/files?a=1"), "def5678", "2", "0", "0"],
        "true\n",
    );
    let err = stderr(&out);
    let unlogged = err.find("--unlogged").expect(&err);
    let rebase = err.find("回し直していない").expect(&err);
    assert!(unlogged < rebase, "{err}");
    // 案内のコマンドには、受けた URL のページやクエリを除いた PR の URL を書く
    assert!(
        err.contains(&format!("scripts/decide-next.sh {URL} ")),
        "{err}"
    );
}

#[test]
fn an_inherited_git_dir_does_not_redirect_the_lookup() {
    // git の hook の中や、そこから起動したセッションでは GIT_DIR が渡る。起動したディレクトリの clone でなく、
    // GIT_DIR のリポジトリの行を見ない
    let fake = FakeGh::new();
    // 起動したディレクトリの clone には、別の head の行しか無い
    reviewed_at(&fake, &["abc1234abc1234abc1234abc1234abc1234abc12"]);
    // GIT_DIR が指す別のリポジトリには、head の行がある
    let other = fake.dir().join("other");
    let mut cmd = std::process::Command::new("git");
    let ok = forget_git_env(&mut cmd)
        .args(["init", "-q"])
        .arg(&other)
        .env("GIT_CONFIG_GLOBAL", fake.dir().join("no-global-config"))
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .status()
        .unwrap()
        .success();
    assert!(ok, "git init");
    std::fs::create_dir_all(other.join(".git/claude-review-rounds")).unwrap();
    std::fs::write(
        other.join(".git/claude-review-rounds/x"),
        format!("{HEAD} 1794900000\n"),
    )
    .unwrap();
    let git_dir = other.join(".git").to_string_lossy().into_owned();
    let out = fake.run_calls_with_env(
        "record-local-review",
        &[URL, "def5678", "0", "0", "0"],
        &[(&format!("{HEAD}\n"), 0), ("true\n", 0), ("", 0)],
        &[("GIT_DIR", &git_dir)],
    );
    assert_eq!(out.status.code(), Some(1), "{}", stderr(&out));
    assert!(!fake.log().contains("pr comment"));
}

#[test]
fn an_unlogged_review_is_recorded_where_the_directory_is_not_a_clone() {
    // WORKTREE が none で PR の URL を対象に渡した周は、cd する worktree が無い
    let fake = FakeGh::new();
    let ceiling = fake.dir().parent().unwrap().to_string_lossy().into_owned();
    let out = fake.run_calls_with_env(
        "record-local-review",
        &[
            URL,
            "def5678",
            "0",
            "0",
            "0",
            "--unlogged",
            "対象に PR の URL を渡して回した",
        ],
        &[(&format!("{HEAD}\n"), 0), ("true\n", 0), ("", 0)],
        &[("GIT_CEILING_DIRECTORIES", &ceiling)],
    );
    assert_eq!(out.status.code(), Some(0), "{}", stderr(&out));
    assert!(
        fake.log().contains("回した記録を確かめていない理由"),
        "{}",
        fake.log()
    );
}

#[test]
fn a_reason_is_dropped_when_the_round_is_logged() {
    // 行があるのに理由を付けても、記録のコメントに「行が無い」と書かない
    let fake = FakeGh::new();
    let out = run(
        &fake,
        &[URL, "def5678", "0", "0", "0", "--unlogged", "念のため"],
        "true\n",
    );
    assert_eq!(out.status.code(), Some(0), "{}", stderr(&out));
    assert!(
        !fake.log().contains("回した記録を確かめていない理由"),
        "{}",
        fake.log()
    );
}

#[test]
fn an_empty_reason_is_a_usage_error() {
    let fake = FakeGh::new();
    for args in [
        &[URL, "def5678", "0", "0", "0", "--unlogged"][..],
        &[URL, "def5678", "0", "0", "0", "--unlogged", " "][..],
        &[URL, "def5678", "0", "0", "0", "--other", "x"][..],
    ] {
        let out = run(&fake, args, "true\n");
        assert_eq!(out.status.code(), Some(1), "{args:?}");
        assert!(
            stderr(&out).contains("Usage: record-local-review.sh"),
            "{args:?}: {}",
            stderr(&out)
        );
    }
}

#[test]
fn a_repository_it_cannot_push_to_records_nothing() {
    // 記録を数えるのは書き込める人のコメントだけなので、上流の PR に付けても効かない
    let fake = FakeGh::new();
    let out = run(&fake, &[URL, "def5678", "0", "0", "0"], "false\n");
    assert_eq!(out.status.code(), Some(1));
    assert!(stderr(&out).contains("push できない"), "{}", stderr(&out));
    assert!(!fake.log().contains("pr comment"));
}

#[test]
fn a_failed_query_records_nothing() {
    for calls in [
        &[("", 1)][..],
        // head が空なら、記録の無い出力の空行と一致してしまう
        &[("\n", 0)][..],
        &[(&format!("{HEAD}\n")[..], 0), ("", 1)][..],
    ] {
        let fake = FakeGh::new();
        let out = fake.run_calls(
            "record-local-review",
            &[URL, "def5678", "0", "0", "0"],
            calls,
        );
        assert_eq!(out.status.code(), Some(1), "{calls:?}");
        assert!(
            stderr(&out).contains("記録を付けていません"),
            "{calls:?}: {}",
            stderr(&out)
        );
        assert!(!stderr(&out).contains("push できない"), "{calls:?}");
        assert!(!fake.log().contains("pr comment"), "{calls:?}");
    }
}

#[test]
fn bad_arguments_stop_before_gh() {
    for args in [
        &[URL, "def5678", "0", "0"][..],
        &[URL, "def5678", "0", "0", "0", "0"][..],
        &["1", "def5678", "0", "0", "0"][..],
        &[
            "https://github.com/octo/repo/pull/42#r1",
            "def5678",
            "0",
            "0",
            "0",
        ][..],
        // SHA は 7〜40 桁の 16 進の小文字
        &[URL, "HEAD", "0", "0", "0"][..],
        &[URL, "abc12", "0", "0", "0"][..],
        &[URL, "DEF5678", "0", "0", "0"][..],
        // 読む側は high=0 の字面で比べるので、先頭の 0 を許さない
        &[URL, "def5678", "00", "0", "0"][..],
        &[URL, "def5678", "1", "x", "0"][..],
        &[URL, "def5678", "1", "0", "-1"][..],
    ] {
        let fake = FakeGh::new();
        let out = run(&fake, args, "true\n");
        assert_eq!(out.status.code(), Some(1), "{args:?}");
        assert!(
            stderr(&out).contains("Usage: record-local-review.sh"),
            "{args:?}: {}",
            stderr(&out)
        );
        assert_eq!(fake.log(), "", "{args:?}");
    }
}

/// 指摘の要旨を書いたファイルを、起動するディレクトリに置く
fn findings_file(fake: &FakeGh, text: &str) -> String {
    let path = fake.dir().join("findings.md");
    std::fs::write(&path, text).unwrap();
    path.display().to_string()
}

#[test]
#[ignore = "RED: --findings は未実装"]
fn the_findings_are_written_under_the_record() {
    // 件数だけの記録では、レビューの途中でセッションが替わると、対応しなかった指摘の中身が会話にしか残らない
    let fake = FakeGh::new();
    let path = findings_file(
        &fake,
        "- Medium a.rs:12 引数が空のときに落ちる（再現なし）\n- Low b.md:3 語の揺れ（再現なし）\n\n",
    );
    let out = run(
        &fake,
        &[URL, HEAD, "0", "1", "1", "--findings", &path],
        "true\n",
    );
    assert_eq!(out.status.code(), Some(0), "{}", stderr(&out));
    assert!(
        fake.log().contains(&format!(
            "gh pr comment 42 --body <!-- local-review sha={HEAD} high=0 medium=1 low=1 -->\n\
             ローカルの /code-review で def5678 を確かめた: High 0 / Medium 1 / Low 1\n\
             \n\
             指摘:\n\
             - Medium a.rs:12 引数が空のときに落ちる（再現なし）\n\
             - Low b.md:3 語の揺れ（再現なし）\n"
        )),
        "{}",
        fake.log()
    );
}

#[test]
#[ignore = "RED: --findings は未実装"]
fn the_findings_follow_the_reason_of_an_unlogged_review() {
    let fake = FakeGh::new();
    let path = findings_file(&fake, "- Low b.md:3 語の揺れ（再現なし）\n");
    let reason = "PR の URL を渡した";
    for args in [
        &[
            URL,
            HEAD,
            "0",
            "0",
            "1",
            "--unlogged",
            reason,
            "--findings",
            &path,
        ][..],
        &[
            URL,
            HEAD,
            "0",
            "0",
            "1",
            "--findings",
            &path,
            "--unlogged",
            reason,
        ][..],
    ] {
        let fake_run = FakeGh::new();
        let out = run_as_is(&fake_run, args, "true\n");
        assert_eq!(out.status.code(), Some(0), "{args:?}: {}", stderr(&out));
        assert!(
            fake_run.log().contains(
                "回した記録を確かめていない理由: PR の URL を渡した\n\
                 \n\
                 指摘:\n\
                 - Low b.md:3 語の揺れ（再現なし）\n"
            ),
            "{args:?}: {}",
            fake_run.log()
        );
    }
}

#[test]
#[ignore = "RED: --findings は未実装"]
fn findings_without_their_text_stop_before_gh() {
    let files = FakeGh::new();
    let empty = findings_file(&files, " \n\n");
    let missing = files.dir().join("no-such-file.md").display().to_string();
    for args in [
        // 指摘があるのに、要旨のファイルを渡していない
        &[URL, HEAD, "0", "1", "0"][..],
        &[URL, HEAD, "1", "0", "0", "--unlogged", "PR の URL を渡した"][..],
        // ファイルが無い、空
        &[URL, HEAD, "0", "1", "0", "--findings", &missing][..],
        &[URL, HEAD, "0", "1", "0", "--findings", &empty][..],
    ] {
        let fake = FakeGh::new();
        let out = run(&fake, args, "true\n");
        assert_eq!(out.status.code(), Some(1), "{args:?}");
        assert!(
            stderr(&out).contains("--findings"),
            "{args:?}: {}",
            stderr(&out)
        );
        assert_eq!(fake.log(), "", "{args:?}");
    }
}
