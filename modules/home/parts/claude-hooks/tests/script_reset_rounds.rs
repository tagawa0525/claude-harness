//! `claude-hooks script reset-rounds <PR の URL> <理由>` が、PR の URL のリポジトリの PR に、レビューの周回を数え直す
//! 印のコメントを付けることを確かめる（ADR-0087。`~/.claude/skills/gh-pr-review/scripts/reset-rounds.sh` が呼ぶ）。
//! 印の形は decide-next.sh が数えるもの

mod common;

use common::fake_gh::{FakeGh, stderr};

#[test]
fn comments_the_mark_and_the_reason_on_the_pr_of_the_url() {
    let fake = FakeGh::new();
    let out = fake.run(
        "reset-rounds",
        &[
            "https://github.com/octo/repo/pull/42",
            "ユーザーの要望: 表を足す",
        ],
        "",
        0,
    );
    assert_eq!(out.status.code(), Some(0), "{}", stderr(&out));
    assert_eq!(
        fake.log(),
        "gh pr comment 42 --body <!-- review-rounds-reset -->\n\
         レビューの周回を数え直す: ユーザーの要望: 表を足す\n"
    );
    assert_eq!(fake.env(), "GH_REPO=octo/repo\nGH_HOST=github.com\n");
}

#[test]
fn the_exit_code_of_gh_is_the_exit_code() {
    let fake = FakeGh::new();
    let out = fake.run(
        "reset-rounds",
        &["https://github.com/octo/repo/pull/42", "why"],
        "",
        4,
    );
    assert_eq!(out.status.code(), Some(4));
}

#[test]
fn a_first_argument_that_is_not_a_pr_url_stops_before_gh() {
    for url in [
        "",
        "1",
        "https://github.com/octo/repo/issues/1",
        "https://github.com/evil;echo_PWN/repo/pull/1",
        "https://github.com/../x/pull/1",
        // コメントの URL は、どのコメントの話かを印が持たないので受けない
        "https://github.com/octo/repo/pull/1#discussion_r111",
    ] {
        let fake = FakeGh::new();
        let out = fake.run("reset-rounds", &[url, "why"], "", 0);
        assert_eq!(out.status.code(), Some(1), "{url}");
        assert!(
            stderr(&out).contains("Usage: reset-rounds.sh"),
            "{url}: {}",
            stderr(&out)
        );
        assert_eq!(fake.log(), "", "{url}");
    }
}

#[test]
fn a_missing_or_empty_reason_or_extra_arguments_show_the_usage() {
    // 理由を引用符で囲み忘れると語ごとの引数になり、最初の語だけが PR に残る
    for args in [
        &["https://github.com/octo/repo/pull/1"][..],
        &["https://github.com/octo/repo/pull/1", ""][..],
        &["https://github.com/octo/repo/pull/1", "ユーザーの", "要望"][..],
    ] {
        let fake = FakeGh::new();
        let out = fake.run("reset-rounds", args, "", 0);
        assert_eq!(out.status.code(), Some(1), "{args:?}");
        assert!(stderr(&out).contains("Usage: reset-rounds.sh"), "{args:?}");
        assert_eq!(fake.log(), "", "{args:?}");
    }
}
