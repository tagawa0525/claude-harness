//! `claude-hooks script record-local-review <PR の URL> <SHA> <high> <medium> <low>` が、ローカルの /code-review を
//! 回した PR の head の記録を、マージの hook（pre-merge-check）が数える印のコメントとして PR に付けることを確かめる
//! （ADR-0087。`~/.claude/skills/gh-pr-review/scripts/record-local-review.sh` が呼ぶ）。記録はマージの hook が Copilot
//! のレビューの代わりに数えるので、head でない SHA、push できないリポジトリ、形の崩れた件数では付けない

mod common;

use common::fake_gh::{FakeGh, stderr};

const URL: &str = "https://github.com/octo/repo/pull/42";
const HEAD: &str = "def5678def5678def5678def5678def5678def56";

fn stdout(out: &std::process::Output) -> String {
    String::from_utf8_lossy(&out.stdout).into_owned()
}

fn run(fake: &FakeGh, args: &[&str], push: &str) -> std::process::Output {
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
