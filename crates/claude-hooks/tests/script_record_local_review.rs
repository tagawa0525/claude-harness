//! `claude-hooks script record-local-review <PR の URL> <SHA> <high> <medium> <low>` が、ローカルの /code-review を
//! 回した PR の head の記録を、マージの hook（pre-merge-check）が数える印のコメントとして PR に付けることを確かめる
//! （ADR-0087。`~/.claude/skills/gh-pr-review/scripts/record-local-review.sh` が呼ぶ）。記録はマージの hook が Copilot
//! のレビューの代わりに数えるので、head でない SHA、push できないリポジトリ、形の崩れた件数では付けない。その head で
//! /code-review を回した記録（count-review-rounds の hook が clone に書く行）が、起動したディレクトリの clone に無いときも付けない
//! （hook が書かない回し方をしたときは、`--unlogged <理由>` で理由を記録に残して付ける）。指摘は 1 件ごとに
//! `--finding <要旨>` で受けて記録に書き、数が件数の合計と合わなければ付けない。0 件の記録は、その head で回した
//! /code-review の記録（サブエージェントの transcript）の tool の結果に、PR の差分の追加行が入っているときだけ付ける

mod common;

use common::fake_gh::{FakeGh, stderr};
use common::forget_git_env;

const URL: &str = "https://github.com/octo/repo/pull/42";
const HEAD: &str = "def5678def5678def5678def5678def5678def56";
const FINDING: &str = "Medium a.rs:12 引数が空のときに落ちる（再現なし）";

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
    let out = run(
        &fake,
        &[
            URL,
            "def5678",
            "1",
            "2",
            "0",
            "--finding",
            FINDING,
            "--finding",
            FINDING,
            "--finding",
            FINDING,
        ],
        "true\n",
    );
    assert_eq!(out.status.code(), Some(0), "{}", stderr(&out));
    assert_eq!(
        fake.log(),
        format!(
            "gh pr view 42 --json headRefOid -q .headRefOid\n\
             gh api repos/{{owner}}/{{repo}} --jq .permissions.push\n\
             gh pr comment 42 --body <!-- local-review sha={HEAD} high=1 medium=2 low=0 -->\n\
             ローカルの /code-review で def5678 を確かめた: High 1 / Medium 2 / Low 0\n\
             \n\
             指摘:\n\
             - {FINDING}\n\
             - {FINDING}\n\
             - {FINDING}\n"
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
    let out = run(
        &fake,
        &[
            URL,
            HEAD,
            "0",
            "3",
            "1",
            "--finding",
            FINDING,
            "--finding",
            FINDING,
            "--finding",
            FINDING,
            "--finding",
            FINDING,
        ],
        "true\n",
    );
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
    let out = run_as_is(
        &fake,
        &[
            URL,
            "def5678",
            "0",
            "1",
            "4",
            "--finding",
            FINDING,
            "--finding",
            FINDING,
            "--finding",
            FINDING,
            "--finding",
            FINDING,
            "--finding",
            FINDING,
        ],
        "true\n",
    );
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
    let out = run_as_is(
        &fake,
        &[URL, HEAD, "0", "0", "1", "--finding", FINDING],
        "true\n",
    );
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
            "--finding",
            FINDING,
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
fn an_unverified_record_with_no_findings_notes_that_the_reply_must_show_the_diff_was_read() {
    // --unlogged の 0 件の記録は、差分を読んだかを機械で確かめていない。対象を見つけられなかった、エラーで終わった、
    // `[]` だけの返答の周は未実施で、0 件の記録はレビュー済みに見える
    let fake = FakeGh::new();
    reviewed_at(&fake, &["abc1234abc1234abc1234abc1234abc1234abc12"]);
    let out = run_as_is(
        &fake,
        &[URL, HEAD, "0", "0", "0", "--unlogged", "PR の URL を渡した"],
        "true\n",
    );
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
    let out = run_as_is(
        &fake,
        &[
            URL,
            "def5678",
            "2",
            "0",
            "0",
            "--finding",
            FINDING,
            "--finding",
            FINDING,
        ],
        "true\n",
    );
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
        &[
            &format!("{URL}/files?a=1"),
            "def5678",
            "2",
            "0",
            "0",
            "--finding",
            FINDING,
            "--finding",
            FINDING,
        ],
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
        &[
            URL,
            "def5678",
            "0",
            "0",
            "1",
            "--finding",
            FINDING,
            "--unlogged",
            "念のため",
        ],
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

#[test]
fn each_finding_is_a_line_under_the_record() {
    // 要旨は引数で受ける。ファイルを読んでコメントに書く形は、確認なしで通るこのスクリプトが、手元の任意のファイルを
    // PR に出す経路になる
    let fake = FakeGh::new();
    let out = run(
        &fake,
        &[
            URL,
            HEAD,
            "0",
            "1",
            "1",
            "--finding",
            "Medium a.rs:12 引数が空のときに落ちる（再現なし）",
            "--unlogged",
            "PR の URL を渡した",
            "--finding",
            " Low b.md:3 語の揺れ（再現なし） ",
        ],
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
fn the_findings_must_be_as_many_as_the_counts() {
    // 件数だけの記録では、セッションが替わると、対応しなかった指摘をマージの本文に書けない
    for args in [
        &[URL, HEAD, "0", "1", "0"][..],
        &[URL, HEAD, "1", "0", "1", "--finding", "High a.rs:1 落ちる"][..],
        &[URL, HEAD, "0", "0", "0", "--finding", "Low a.rs:1 語"][..],
        &[URL, HEAD, "1", "0", "0", "--unlogged", "PR の URL を渡した"][..],
        // 足すと u64 を超える件数も、0 件に見せない
        &[URL, HEAD, "18446744073709551615", "1", "0"][..],
    ] {
        let fake = FakeGh::new();
        let out = run(&fake, args, "true\n");
        assert_eq!(out.status.code(), Some(1), "{args:?}");
        assert!(
            stderr(&out).contains("件なのに、要旨（--finding）が"),
            "{args:?}: {}",
            stderr(&out)
        );
        assert_eq!(fake.log(), "", "{args:?}");
    }
}

#[test]
fn a_malformed_option_is_a_usage_error() {
    for args in [
        // 値が無い、空、複数行（1 件を 1 行で読めるように）
        &[URL, HEAD, "0", "1", "0", "--finding"][..],
        &[URL, HEAD, "0", "1", "0", "--finding", " "][..],
        &[
            URL,
            HEAD,
            "0",
            "1",
            "0",
            "--finding",
            "Low a.rs:1 語\n- 2 行目",
        ][..],
        // 知らないフラグ、2 度目の --unlogged
        &[URL, HEAD, "0", "0", "0", "--force", "x"][..],
        &[
            URL,
            HEAD,
            "0",
            "0",
            "0",
            "--unlogged",
            "a",
            "--unlogged",
            "b",
        ][..],
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

#[test]
fn a_bad_finding_names_the_value() {
    // 何件も渡した要旨のどれが悪いかを、使い方の行だけでは見分けられない。CR だけの改行も 1 行に数えない
    for value in [" ", "Low a.rs:1 語\n- 2 行目", "Low a.rs:1 語\r2 行目"] {
        let fake = FakeGh::new();
        let out = run(
            &fake,
            &[URL, HEAD, "0", "0", "1", "--finding", value],
            "true\n",
        );
        assert_eq!(out.status.code(), Some(1), "{value:?}");
        assert!(
            stderr(&out).contains("--finding の値は、空でない 1 行"),
            "{value:?}: {}",
            stderr(&out)
        );
        assert_eq!(fake.log(), "", "{value:?}");
    }
}

// ---------------------------------------------------------------------------
// 0 件の記録: その head で回した /code-review の記録に、PR の差分の追加行が入っているか
// ---------------------------------------------------------------------------

const SESSION: &str = "0a87f718-test-session";
const TARGET: &str = "/work/app-feat-x";
/// 周の行の時刻（1794900000 = 2026-11-17T07:20:00Z）の直後（/code-review のサブエージェントは、Skill の呼び出しの
/// 1 秒以内に始まる）
const AFTER: &str = "2026-11-17T07:20:01.000Z";
const BEFORE: &str = "2026-11-17T07:19:00.000Z";

/// PR の差分。本体（src/a.rs）のほかに、テストと lock のファイルの追加行がある
const PR_DIFF: &str = "diff --git a/src/a.rs b/src/a.rs\n\
index 1111111..2222222 100644\n\
--- a/src/a.rs\n\
+++ b/src/a.rs\n\
@@ -1,0 +2,3 @@\n\
+fn checked(x: u8) -> bool {\n\
+    x > 0\n\
+}\n\
diff --git a/tests/a.rs b/tests/a.rs\n\
index 1111111..2222222 100644\n\
--- a/tests/a.rs\n\
+++ b/tests/a.rs\n\
@@ -1,0 +2,1 @@\n\
+assert!(checked(1));\n\
diff --git a/Cargo.lock b/Cargo.lock\n\
index 1111111..2222222 100644\n\
--- a/Cargo.lock\n\
+++ b/Cargo.lock\n\
@@ -1,0 +2,1 @@\n\
+version = \"9.9.9\"\n";

/// テストのファイルだけの PR の差分
const TEST_ONLY_DIFF: &str = "diff --git a/tests/a.rs b/tests/a.rs\n\
index 1111111..2222222 100644\n\
--- a/tests/a.rs\n\
+++ b/tests/a.rs\n\
@@ -1,0 +2,1 @@\n\
+assert!(checked(1));\n";

/// 偽の Claude Code の設定の場所（CLAUDE_CONFIG_DIR）
fn config(fake: &FakeGh) -> std::path::PathBuf {
    fake.dir().join("claude-config")
}

/// count-review-rounds が書く形の行（`<HEAD> <UNIX 秒> <セッション> <回した worktree>`）で、rounds の HEAD を回した記録を置く
fn reviewed_in_session(fake: &FakeGh, rounds: &[&str]) {
    reviewed_at(fake, &[]);
    let text: String = rounds
        .iter()
        .map(|h| format!("{h} 1794900000 {SESSION} {TARGET}\n"))
        .collect();
    std::fs::write(fake.dir().join(".git/claude-review-rounds/feat%2Fx"), text).unwrap();
}

/// /code-review が起動したサブエージェントの記録を、Claude Code が置く形で置く（`<設定>/projects/<プロジェクト>/<セッション>/
/// subagents/agent-<id>.jsonl` と、隣の `.forked-skill.json`）。tool の結果は、文字列と text の配列の両方の形で入れる
fn transcript(fake: &FakeGh, id: &str, skill: &str, effort: &str, started: &str, results: &[&str]) {
    transcript_of(fake, id, TARGET, skill, effort, started, results);
}

/// [`transcript`] と同じだが、依頼文の対象を target にする
fn transcript_of(
    fake: &FakeGh,
    id: &str,
    target: &str,
    skill: &str,
    effort: &str,
    started: &str,
    results: &[&str],
) {
    let dir = config(fake).join(format!("projects/-work-app/{SESSION}/subagents"));
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(
        dir.join(format!("agent-{id}.forked-skill.json")),
        serde_json::json!({"skillName": skill, "attributionName": skill, "effort": effort})
            .to_string(),
    )
    .unwrap();
    let mut lines = vec![serde_json::json!({
        "type": "user",
        "timestamp": started,
        "message": {"role": "user", "content": format!("Review target: `{target} 焦点: 直した差分`\n\n## Turn 1 — read\n")},
    })];
    for (i, result) in results.iter().enumerate() {
        let content = if i % 2 == 0 {
            serde_json::json!(result)
        } else {
            serde_json::json!([{"type": "text", "text": result}])
        };
        lines.push(serde_json::json!({
            "type": "user",
            "timestamp": started,
            "message": {"role": "user", "content": [{"type": "tool_result", "tool_use_id": "t", "content": content}]},
        }));
    }
    lines.push(serde_json::json!({
        "type": "assistant",
        "timestamp": started,
        "message": {"role": "assistant", "content": [{"type": "text", "text": "(none)"}]},
    }));
    let text: String = lines.iter().map(|l| format!("{l}\n")).collect();
    std::fs::write(dir.join(format!("agent-{id}.jsonl")), text).unwrap();
}

/// 0 件で記録する（gh は head、権限、PR の差分、コメントの順に呼ばれる）
fn record_zero(fake: &FakeGh, head: &str, diff: &str, extra: &[&str]) -> std::process::Output {
    let mut args = vec![URL, head, "0", "0", "0"];
    args.extend_from_slice(extra);
    let config = config(fake).to_string_lossy().into_owned();
    fake.run_calls_with_env(
        "record-local-review",
        &args,
        &[
            (&format!("{head}\n"), 0),
            ("true\n", 0),
            (diff, 0),
            ("https://github.com/octo/repo/pull/42#issuecomment-1\n", 0),
        ],
        &[("CLAUDE_CONFIG_DIR", &config)],
    )
}

/// 読んだ周の tool の結果（git diff の出力）
const READ_DIFF: &str = "diff --git a/src/a.rs b/src/a.rs\n@@ -1,0 +2,3 @@\n+fn checked(x: u8) -> bool {\n+    x > 0\n+}\n";

#[test]
fn zero_findings_from_a_review_that_read_the_diff_are_recorded() {
    // テストのファイル（/code-review の low は外す）と lock のファイルの行は、読んでいなくてよい
    let fake = FakeGh::new();
    reviewed_in_session(&fake, &[HEAD]);
    transcript(&fake, "a1", "code-review", "low", AFTER, &["", READ_DIFF]);
    let out = record_zero(&fake, HEAD, PR_DIFF, &[]);
    assert_eq!(out.status.code(), Some(0), "{}", stderr(&out));
    assert!(
        fake.log().contains("gh pr diff 42 --color never\n"),
        "{}",
        fake.log()
    );
    assert!(
        fake.log().contains(&format!(
            "gh pr comment 42 --body <!-- local-review sha={HEAD} high=0 medium=0 low=0 -->\n\
             ローカルの /code-review で def5678 を確かめた: High 0 / Medium 0 / Low 0\n"
        )),
        "{}",
        fake.log()
    );
    assert!(
        !fake.log().contains("回した記録を確かめていない理由"),
        "{}",
        fake.log()
    );
    // 読んだことを機械で確かめた記録には、返答の読み方の注意を出さない
    assert!(!stdout(&out).contains("NOTE:"), "{}", stdout(&out));
}

#[test]
fn zero_findings_from_a_review_that_did_not_read_the_diff_record_nothing_and_name_the_next_round() {
    // 大きな出力はファイルに置かれ、tool の結果は先頭の 2KB だけになる。そのファイルを読まずに `(none)` を返した周は未実施
    let fake = FakeGh::new();
    reviewed_in_session(&fake, &[HEAD]);
    transcript(
        &fake,
        "a1",
        "code-review",
        "low",
        AFTER,
        &[
            "<persisted-output>\nOutput too large (50KB).\n\nPreview (first 2KB):\ndiff --git a/src/a.rs b/src/a.rs\n",
        ],
    );
    let out = record_zero(&fake, HEAD, PR_DIFF, &[]);
    assert_eq!(out.status.code(), Some(1), "{}", stderr(&out));
    assert!(!fake.log().contains("pr comment"), "{}", fake.log());
    let err = stderr(&out);
    assert!(err.contains("src/a.rs"), "{err}");
    assert!(err.contains("記録を付けていません"), "{err}");
    // 次の手はレベルを上げて同じ対象を、欠けたファイルを焦点に回し直すこと
    assert!(
        stdout(&out).contains(&format!(
            "NEXT: /code-review medium {TARGET} 焦点: src/a.rs の差分"
        )),
        "{}",
        stdout(&out)
    );
}

#[test]
fn the_rounds_of_a_head_together_can_cover_the_diff() {
    // 欠けたファイルを焦点に回し直した周は、そのファイルだけを読む。同じ head の周が合わせて読んでいれば記録する
    let fake = FakeGh::new();
    reviewed_in_session(&fake, &[HEAD]);
    // 回し直した周の行（10 分後）
    let rounds = fake.dir().join(".git/claude-review-rounds/feat%2Fx");
    let text = std::fs::read_to_string(&rounds).unwrap();
    std::fs::write(
        &rounds,
        format!("{text}{HEAD} 1794900600 {SESSION} {TARGET}\n"),
    )
    .unwrap();
    transcript(
        &fake,
        "a1",
        "code-review",
        "low",
        AFTER,
        &["+fn checked(x: u8) -> bool {"],
    );
    transcript(
        &fake,
        "a2",
        "code-review",
        "medium",
        "2026-11-17T07:30:00.500Z",
        &["+    x > 0\n+}"],
    );
    let out = record_zero(&fake, HEAD, PR_DIFF, &[]);
    assert_eq!(out.status.code(), Some(0), "{}", stderr(&out));
}

#[test]
fn a_test_only_diff_must_be_read_in_its_test_files() {
    // 差分がテストだけなら、テストを外して何も読まない周は未実施。テストを読んだ周は記録する
    let fake = FakeGh::new();
    reviewed_in_session(&fake, &[HEAD]);
    transcript(
        &fake,
        "a1",
        "code-review",
        "medium",
        AFTER,
        &["no diff outside tests"],
    );
    let out = record_zero(&fake, HEAD, TEST_ONLY_DIFF, &[]);
    assert_eq!(out.status.code(), Some(1), "{}", stderr(&out));
    assert!(stderr(&out).contains("tests/a.rs"), "{}", stderr(&out));
    assert!(
        stdout(&out).contains(&format!("NEXT: /code-review high {TARGET} ")),
        "{}",
        stdout(&out)
    );

    let fake = FakeGh::new();
    reviewed_in_session(&fake, &[HEAD]);
    transcript(
        &fake,
        "a1",
        "code-review",
        "medium",
        AFTER,
        &["+assert!(checked(1));"],
    );
    let out = record_zero(&fake, HEAD, TEST_ONLY_DIFF, &[]);
    assert_eq!(out.status.code(), Some(0), "{}", stderr(&out));
}

#[test]
fn only_code_review_transcripts_started_after_the_round_count() {
    // 前の周の記録、ほかのサブエージェント（差分を書いた担当など）の記録、周の行の直後に始まっていない記録は、この周に
    // 読んだことを示さない
    let fake = FakeGh::new();
    reviewed_in_session(&fake, &[HEAD]);
    transcript(&fake, "old", "code-review", "low", BEFORE, &[READ_DIFF]);
    transcript(&fake, "other", "simplify", "low", AFTER, &[READ_DIFF]);
    // 同じセッションで後に回した、別の PR の周（/delegate の担当は親のセッションを共有する）
    transcript(
        &fake,
        "later",
        "code-review",
        "low",
        "2026-11-17T07:30:00.000Z",
        &[READ_DIFF],
    );
    transcript(&fake, "a1", "code-review", "low", AFTER, &["nothing"]);
    let out = record_zero(&fake, HEAD, PR_DIFF, &[]);
    assert_eq!(out.status.code(), Some(1), "{}", stderr(&out));
    assert!(!fake.log().contains("pr comment"), "{}", fake.log());
    assert!(
        stdout(&out).contains("NEXT: /code-review medium"),
        "{}",
        stdout(&out)
    );
}

#[test]
fn a_round_whose_transcript_cannot_be_found_needs_a_reason() {
    // 別の機械で回した、記録が消えた、前の版の hook が書いたセッションの無い行は、読んだかを確かめられない。--unlogged と
    // 同じく、理由を記録に残して付ける
    for setup in [
        // セッションの無い行
        (|fake: &FakeGh| reviewed_at(fake, &[HEAD])) as fn(&FakeGh),
        // セッションはあるが、その記録が無い
        |fake: &FakeGh| reviewed_in_session(fake, &[HEAD]),
    ] {
        let fake = FakeGh::new();
        setup(&fake);
        let out = record_zero(&fake, HEAD, PR_DIFF, &[]);
        assert_eq!(out.status.code(), Some(1), "{}", stderr(&out));
        assert!(stderr(&out).contains("--unlogged"), "{}", stderr(&out));
        assert!(!fake.log().contains("pr comment"), "{}", fake.log());

        let fake = FakeGh::new();
        setup(&fake);
        let out = record_zero(&fake, HEAD, PR_DIFF, &["--unlogged", "別の機械で回した"]);
        assert_eq!(out.status.code(), Some(0), "{}", stderr(&out));
        assert!(
            fake.log()
                .contains("回した記録を確かめていない理由: 別の機械で回した\n"),
            "{}",
            fake.log()
        );
    }
}

#[test]
fn a_reason_does_not_pass_a_review_that_did_not_read_the_diff() {
    // --unlogged は記録を確かめられないときの口で、読んでいないと分かった周を通さない
    let fake = FakeGh::new();
    reviewed_in_session(&fake, &[HEAD]);
    transcript(&fake, "a1", "code-review", "low", AFTER, &["nothing"]);
    let out = record_zero(&fake, HEAD, PR_DIFF, &["--unlogged", "念のため"]);
    assert_eq!(out.status.code(), Some(1), "{}", stderr(&out));
    assert!(!fake.log().contains("pr comment"), "{}", fake.log());
}

/// 起動するディレクトリの clone に、base の上の 2 つのコミット（前の周の head と、直した head）を作る。返すのは base と直した head
fn two_heads(fake: &FakeGh) -> (String, String) {
    let git = |args: &[&str]| {
        let mut cmd = std::process::Command::new("git");
        let out = forget_git_env(&mut cmd)
            .args(["-c", "user.name=t", "-c", "user.email=t@example.com"])
            .args(args)
            .current_dir(fake.dir())
            .env("GIT_CONFIG_GLOBAL", fake.dir().join("no-global-config"))
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .output()
            .unwrap();
        assert!(out.status.success(), "git {args:?}");
        String::from_utf8(out.stdout).unwrap().trim().to_string()
    };
    reviewed_at(fake, &[]);
    std::fs::create_dir_all(fake.dir().join("src")).unwrap();
    std::fs::write(fake.dir().join("src/a.rs"), "fn base() {}\n").unwrap();
    git(&["add", "src/a.rs"]);
    git(&["commit", "-q", "-m", "base"]);
    let base = git(&["rev-parse", "HEAD"]);
    std::fs::write(fake.dir().join("src/a.rs"), "fn base() {}\nfn first() {}\n").unwrap();
    git(&["commit", "-q", "-am", "first"]);
    let first = git(&["rev-parse", "HEAD"]);
    std::fs::write(
        fake.dir().join("src/a.rs"),
        "fn base() {}\nfn first() {}\nfn second() {}\n",
    )
    .unwrap();
    git(&["commit", "-q", "-am", "second"]);
    let second = git(&["rev-parse", "HEAD"]);
    std::fs::write(
        fake.dir().join(".git/claude-review-rounds/feat%2Fx"),
        format!("{first} 1794800000 {SESSION}\n{second} 1794900000 {SESSION}\n"),
    )
    .unwrap();
    (base, second)
}

const TWO_HEADS_DIFF: &str = "diff --git a/src/a.rs b/src/a.rs\n\
--- a/src/a.rs\n\
+++ b/src/a.rs\n\
@@ -1,0 +2,2 @@\n\
+fn first() {}\n\
+fn second() {}\n";

#[test]
fn after_a_fix_the_lines_changed_since_the_previous_round_must_be_read() {
    // 直した後の周は、直した差分を焦点に読む。前の周から変わらない行は前の周が読んだ
    let fake = FakeGh::new();
    let (_, second) = two_heads(&fake);
    transcript(
        &fake,
        "a1",
        "code-review",
        "low",
        AFTER,
        &["+fn second() {}"],
    );
    let out = record_zero(&fake, &second, TWO_HEADS_DIFF, &[]);
    assert_eq!(out.status.code(), Some(0), "{}", stderr(&out));

    let fake = FakeGh::new();
    let (_, second) = two_heads(&fake);
    transcript(
        &fake,
        "a1",
        "code-review",
        "low",
        AFTER,
        &["+fn first() {}"],
    );
    let out = record_zero(&fake, &second, TWO_HEADS_DIFF, &[]);
    assert_eq!(out.status.code(), Some(1), "{}", stderr(&out));
    assert!(!fake.log().contains("pr comment"), "{}", fake.log());
}

/// 0 件で記録する。前の周があるときは、PR の base の SHA も gh で引く（head、権限、差分、base、コメントの順）
fn record_zero_with_base(
    fake: &FakeGh,
    head: &str,
    diff: &str,
    base: &str,
) -> std::process::Output {
    let config = config(fake).to_string_lossy().into_owned();
    fake.run_calls_with_env(
        "record-local-review",
        &[URL, head, "0", "0", "0"],
        &[
            (&format!("{head}\n"), 0),
            ("true\n", 0),
            (diff, 0),
            (&format!("{base}\n"), 0),
            ("https://github.com/octo/repo/pull/42#issuecomment-1\n", 0),
        ],
        &[("CLAUDE_CONFIG_DIR", &config)],
    )
}

/// 前の周の head を base の上に、直した head を base の進んだ先（main が同じ行を足した）の上に作る（rebase の後の形）
fn rebased_heads(fake: &FakeGh) -> (String, String) {
    let git = |args: &[&str]| {
        let mut cmd = std::process::Command::new("git");
        let out = forget_git_env(&mut cmd)
            .args(["-c", "user.name=t", "-c", "user.email=t@example.com"])
            .args(args)
            .current_dir(fake.dir())
            .env("GIT_CONFIG_GLOBAL", fake.dir().join("no-global-config"))
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .output()
            .unwrap();
        assert!(out.status.success(), "git {args:?}");
        String::from_utf8(out.stdout).unwrap().trim().to_string()
    };
    let write = |text: &str| std::fs::write(fake.dir().join("src/a.rs"), text).unwrap();
    reviewed_at(fake, &[]);
    std::fs::create_dir_all(fake.dir().join("src")).unwrap();
    write("fn base() {}\n");
    git(&["add", "src/a.rs"]);
    git(&["commit", "-q", "-m", "base"]);
    let base = git(&["rev-parse", "HEAD"]);
    write("let shared = 1;\nfn first() {}\nfn base() {}\n");
    git(&["commit", "-q", "-am", "pr"]);
    let prev = git(&["rev-parse", "HEAD"]);
    git(&["checkout", "-q", &base]);
    write("fn base() {}\nlet shared = 1;\n");
    git(&["commit", "-q", "-am", "main"]);
    let main = git(&["rev-parse", "HEAD"]);
    write("let shared = 1;\nfn first() {}\nfn base() {}\nlet shared = 1;\n");
    git(&["commit", "-q", "-am", "pr rebased"]);
    let head = git(&["rev-parse", "HEAD"]);
    std::fs::write(
        fake.dir().join(".git/claude-review-rounds/feat%2Fx"),
        format!("{prev} 1794800000 {SESSION}\n{head} 1794900000 {SESSION}\n"),
    )
    .unwrap();
    (main, head)
}

const REBASED_DIFF: &str = "diff --git a/src/a.rs b/src/a.rs\n\
--- a/src/a.rs\n\
+++ b/src/a.rs\n\
@@ -0,0 +1,2 @@\n\
+let shared = 1;\n\
+fn first() {}\n";

#[test]
fn after_a_rebase_lines_the_base_also_added_do_not_stand_for_the_pr() {
    // rebase の後の `git diff <前の head> <head>` は base の変更を含む。PR が足した行と字面が同じ base の行だけを求めると、
    // PR の変更を読まない周を通す。前の周から PR の差分が変わっていなければ、PR の差分の全部を求める
    let fake = FakeGh::new();
    let (main, head) = rebased_heads(&fake);
    transcript(
        &fake,
        "a1",
        "code-review",
        "low",
        AFTER,
        &["+let shared = 1;"],
    );
    let out = record_zero_with_base(&fake, &head, REBASED_DIFF, &main);
    assert_eq!(out.status.code(), Some(1), "{}", stderr(&out));
    assert!(!fake.log().contains("pr comment"), "{}", fake.log());

    // base を引けない（手元に無い）ときも、rebase した head には PR の差分の全部を求める
    let fake = FakeGh::new();
    let (_, head) = rebased_heads(&fake);
    transcript(
        &fake,
        "a1",
        "code-review",
        "low",
        AFTER,
        &["+let shared = 1;"],
    );
    let out = record_zero_with_base(
        &fake,
        &head,
        REBASED_DIFF,
        "0000000000000000000000000000000000000000",
    );
    assert_eq!(out.status.code(), Some(1), "{}", stderr(&out));

    let fake = FakeGh::new();
    let (main, head) = rebased_heads(&fake);
    transcript(
        &fake,
        "a1",
        "code-review",
        "low",
        AFTER,
        &["+let shared = 1;\n+fn first() {}"],
    );
    let out = record_zero_with_base(&fake, &head, REBASED_DIFF, &main);
    assert_eq!(out.status.code(), Some(0), "{}", stderr(&out));
}

#[test]
fn a_review_whose_children_read_the_diff_needs_a_reason() {
    // xhigh と max の /code-review は、子のエージェントに読ませる。子の記録は数えないので、読んだかを確かめられない
    let fake = FakeGh::new();
    reviewed_in_session(&fake, &[HEAD]);
    transcript(
        &fake,
        "a1",
        "code-review",
        "max",
        AFTER,
        &["summary of reviewers"],
    );
    let out = record_zero(&fake, HEAD, PR_DIFF, &[]);
    assert_eq!(out.status.code(), Some(1), "{}", stderr(&out));
    assert!(stderr(&out).contains("--unlogged"), "{}", stderr(&out));
    assert!(!stdout(&out).contains("NEXT:"), "{}", stdout(&out));

    let fake = FakeGh::new();
    reviewed_in_session(&fake, &[HEAD]);
    transcript(
        &fake,
        "a1",
        "code-review",
        "max",
        AFTER,
        &["summary of reviewers"],
    );
    let out = record_zero(&fake, HEAD, PR_DIFF, &["--unlogged", "max の周"]);
    assert_eq!(out.status.code(), Some(0), "{}", stderr(&out));
}

#[test]
fn a_review_of_another_worktree_started_at_the_same_time_does_not_count() {
    // /delegate の担当は親のセッションを共有し、同じ頃に別の PR を回す。周の行に書いた worktree と依頼文の対象で分ける
    let fake = FakeGh::new();
    reviewed_at(&fake, &[]);
    let [own, other] = ["wt-a", "wt-b"].map(|name| {
        let dir = fake.dir().join(name);
        let mut cmd = std::process::Command::new("git");
        let ok = forget_git_env(&mut cmd)
            .args(["init", "-q"])
            .arg(&dir)
            .env("GIT_CONFIG_GLOBAL", fake.dir().join("no-global-config"))
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .status()
            .unwrap()
            .success();
        assert!(ok, "git init");
        dir.to_string_lossy().into_owned()
    });
    std::fs::write(
        fake.dir().join(".git/claude-review-rounds/feat%2Fx"),
        format!("{HEAD} 1794900000 {SESSION} {own}\n"),
    )
    .unwrap();
    transcript_of(&fake, "a1", &own, "code-review", "low", AFTER, &["nothing"]);
    transcript_of(
        &fake,
        "b1",
        &other,
        "code-review",
        "max",
        AFTER,
        &[READ_DIFF],
    );
    let out = record_zero(&fake, HEAD, PR_DIFF, &[]);
    assert_eq!(out.status.code(), Some(1), "{}", stderr(&out));
    assert!(!fake.log().contains("pr comment"), "{}", fake.log());
    assert!(
        stdout(&out).contains(&format!("NEXT: /code-review medium {own} ")),
        "{}",
        stdout(&out)
    );
}

#[test]
fn a_review_whose_level_is_unknown_is_still_checked() {
    // 子に読ませるのは xhigh と max だけ。レベルが読めない記録を、確かめられないとして --unlogged で通さない
    let fake = FakeGh::new();
    reviewed_in_session(&fake, &[HEAD]);
    transcript(&fake, "a1", "code-review", "", AFTER, &["nothing"]);
    let out = record_zero(&fake, HEAD, PR_DIFF, &["--unlogged", "念のため"]);
    assert_eq!(out.status.code(), Some(1), "{}", stderr(&out));
    assert!(
        stdout(&out).contains("NEXT: /code-review"),
        "{}",
        stdout(&out)
    );
}

#[test]
fn a_round_line_with_a_broken_time_is_not_a_window() {
    // 手で直して壊れた時刻で、窓の計算を溢れさせない
    let fake = FakeGh::new();
    reviewed_at(&fake, &[]);
    std::fs::write(
        fake.dir().join(".git/claude-review-rounds/feat%2Fx"),
        format!("{HEAD} 99999999999999 {SESSION}\n"),
    )
    .unwrap();
    transcript(&fake, "a1", "code-review", "low", AFTER, &[READ_DIFF]);
    let out = record_zero(&fake, HEAD, PR_DIFF, &[]);
    assert_eq!(out.status.code(), Some(1), "{}", stderr(&out));
    assert!(stderr(&out).contains("--unlogged"), "{}", stderr(&out));
}

/// 前の周の head に base を merge した head を作る（rebase でなく merge で base に追いついた形。前の head は祖先のまま）
fn merged_heads(fake: &FakeGh) -> String {
    let git = |args: &[&str]| {
        let mut cmd = std::process::Command::new("git");
        let out = forget_git_env(&mut cmd)
            .args(["-c", "user.name=t", "-c", "user.email=t@example.com"])
            .args(args)
            .current_dir(fake.dir())
            .env("GIT_CONFIG_GLOBAL", fake.dir().join("no-global-config"))
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .output()
            .unwrap();
        assert!(out.status.success(), "git {args:?}");
        String::from_utf8(out.stdout).unwrap().trim().to_string()
    };
    reviewed_at(fake, &[]);
    std::fs::create_dir_all(fake.dir().join("src")).unwrap();
    std::fs::write(fake.dir().join("src/a.rs"), "fn base() {}\n").unwrap();
    git(&["add", "src/a.rs"]);
    git(&["commit", "-q", "-m", "base"]);
    let base = git(&["rev-parse", "HEAD"]);
    std::fs::write(
        fake.dir().join("src/a.rs"),
        "let shared = 1;\nfn first() {}\nfn base() {}\n",
    )
    .unwrap();
    git(&["commit", "-q", "-am", "pr"]);
    let prev = git(&["rev-parse", "HEAD"]);
    git(&["checkout", "-q", "-b", "main-moved", &base]);
    std::fs::write(fake.dir().join("src/b.rs"), "let shared = 1;\n").unwrap();
    git(&["add", "src/b.rs"]);
    git(&["commit", "-q", "-m", "main"]);
    git(&["checkout", "-q", &prev]);
    git(&["merge", "-q", "--no-edit", "main-moved"]);
    let head = git(&["rev-parse", "HEAD"]);
    std::fs::write(
        fake.dir().join(".git/claude-review-rounds/feat%2Fx"),
        format!("{prev} 1794800000 {SESSION}\n{head} 1794900000 {SESSION}\n"),
    )
    .unwrap();
    head
}

#[test]
fn after_merging_the_base_lines_the_base_added_do_not_stand_for_the_pr() {
    // base を引けず、前の head が祖先でも、base を merge した head の `git diff <前の head> <head>` は base の変更を含む
    const DIFF: &str = "diff --git a/src/a.rs b/src/a.rs\n\
--- a/src/a.rs\n\
+++ b/src/a.rs\n\
@@ -0,0 +1,2 @@\n\
+let shared = 1;\n\
+fn first() {}\n\
diff --git a/src/b.rs b/src/b.rs\n\
--- a/src/b.rs\n\
+++ b/src/b.rs\n\
@@ -0,0 +1 @@\n\
+let shared = 1;\n";
    let fake = FakeGh::new();
    let head = merged_heads(&fake);
    transcript(
        &fake,
        "a1",
        "code-review",
        "low",
        AFTER,
        &["+let shared = 1;"],
    );
    let out = record_zero_with_base(
        &fake,
        &head,
        DIFF,
        "0000000000000000000000000000000000000000",
    );
    assert_eq!(out.status.code(), Some(1), "{}", stderr(&out));
    assert!(!fake.log().contains("pr comment"), "{}", fake.log());
}

#[test]
fn a_review_without_a_worktree_in_its_target_counts_for_the_worktree_of_the_line() {
    // 対象を書かない、PR の番号を渡した、worktree の下のディレクトリを渡した /code-review は、依頼文の対象が周の行の worktree と
    // 字面で合わない。別の worktree と分かるのは、対象がほかのディレクトリを指すときだけ
    // `/` は在るが git の worktree の中に無く、どの worktree かを決められない（焦点の文の「a / b」から取った語）
    for target in ["", "340", "/work/app-feat-x/crates", "/"] {
        let fake = FakeGh::new();
        reviewed_in_session(&fake, &[HEAD]);
        transcript_of(
            &fake,
            "a1",
            target,
            "code-review",
            "low",
            AFTER,
            &[READ_DIFF],
        );
        let out = record_zero(&fake, HEAD, PR_DIFF, &[]);
        assert_eq!(out.status.code(), Some(0), "{target:?}: {}", stderr(&out));
    }
}

#[test]
fn a_worktree_nested_in_the_checkout_of_the_line_is_another_worktree() {
    // Claude Code の `isolation: "worktree"` は、worktree を本体の `.claude/worktrees/<名前>` に置く。パスの上下でなく、git の
    // worktree の根で分ける。本体の下のディレクトリを渡した周は、本体の周と数える
    let git = |dir: &std::path::Path, args: &[&str]| {
        let mut cmd = std::process::Command::new("git");
        let ok = forget_git_env(&mut cmd)
            .args(["-c", "user.name=t", "-c", "user.email=t@example.com"])
            .args(args)
            .current_dir(dir)
            .env("GIT_CONFIG_GLOBAL", dir.join("no-global-config"))
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .status()
            .unwrap()
            .success();
        assert!(ok, "git {args:?}");
    };
    for (target, recorded) in [(".claude/worktrees/x", false), ("src", true)] {
        let fake = FakeGh::new();
        reviewed_at(&fake, &[]);
        std::fs::create_dir_all(fake.dir().join("src")).unwrap();
        std::fs::write(fake.dir().join("src/a.rs"), "fn a() {}\n").unwrap();
        git(fake.dir(), &["add", "src/a.rs"]);
        git(fake.dir(), &["commit", "-q", "-m", "a"]);
        git(
            fake.dir(),
            &["worktree", "add", "-q", "-b", "x", ".claude/worktrees/x"],
        );
        let root = fake.dir().to_string_lossy().into_owned();
        std::fs::write(
            fake.dir().join(".git/claude-review-rounds/feat%2Fx"),
            format!("{HEAD} 1794900000 {SESSION} {root}\n"),
        )
        .unwrap();
        let path = fake.dir().join(target).to_string_lossy().into_owned();
        transcript_of(
            &fake,
            "a1",
            &path,
            "code-review",
            "low",
            AFTER,
            &[READ_DIFF],
        );
        let out = record_zero(&fake, HEAD, PR_DIFF, &[]);
        assert_eq!(
            out.status.code() == Some(0),
            recorded,
            "{target}: {}",
            stderr(&out)
        );
    }
}

#[test]
fn a_transcript_with_bytes_that_are_not_utf8_is_still_read() {
    // 大きな tool の結果に UTF-8 でないバイトが残っても、ほかの行の tool の結果は読む
    let fake = FakeGh::new();
    reviewed_in_session(&fake, &[HEAD]);
    transcript(&fake, "a1", "code-review", "low", AFTER, &[READ_DIFF]);
    let path = config(&fake).join(format!(
        "projects/-work-app/{SESSION}/subagents/agent-a1.jsonl"
    ));
    let mut bytes = std::fs::read(&path).unwrap();
    bytes.extend_from_slice(b"{\"type\":\"user\",\"message\":\"\xff\xfe\"}\n");
    std::fs::write(&path, bytes).unwrap();
    let out = record_zero(&fake, HEAD, PR_DIFF, &[]);
    assert_eq!(out.status.code(), Some(0), "{}", stderr(&out));
}

#[test]
fn the_target_is_the_first_directory_like_the_round_line() {
    // count-review-rounds は引数の中で最初の、在るディレクトリの絶対パスを行の worktree にする。和文が続いて在るディレクトリで
    // ない語や、空白で切れた語を対象と読まない
    for suffix in ["", "の差分"] {
        let fake = FakeGh::new();
        reviewed_at(&fake, &[]);
        let dir = fake.dir().join("wt a");
        let mut cmd = std::process::Command::new("git");
        let ok = forget_git_env(&mut cmd)
            .args(["init", "-q"])
            .arg(&dir)
            .env("GIT_CONFIG_GLOBAL", fake.dir().join("no-global-config"))
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .status()
            .unwrap()
            .success();
        assert!(ok, "git init");
        let own = dir.to_string_lossy().into_owned();
        std::fs::write(
            fake.dir().join(".git/claude-review-rounds/feat%2Fx"),
            format!("{HEAD} 1794900000 {SESSION} {own}\n"),
        )
        .unwrap();
        let target = format!("{own}{suffix}");
        transcript_of(
            &fake,
            "a1",
            &target,
            "code-review",
            "low",
            AFTER,
            &[READ_DIFF],
        );
        let out = record_zero(&fake, HEAD, PR_DIFF, &[]);
        assert_eq!(out.status.code(), Some(0), "{target}: {}", stderr(&out));
    }
}

/// 起動するディレクトリの clone の下に、名前ごとの git のリポジトリを作る（別々の worktree の根）
fn repos(fake: &FakeGh, names: &[&str]) -> Vec<String> {
    names
        .iter()
        .map(|name| {
            let dir = fake.dir().join(name);
            let mut cmd = std::process::Command::new("git");
            let ok = forget_git_env(&mut cmd)
                .args(["init", "-q"])
                .arg(&dir)
                .env("GIT_CONFIG_GLOBAL", fake.dir().join("no-global-config"))
                .env("GIT_CONFIG_NOSYSTEM", "1")
                .status()
                .unwrap()
                .success();
            assert!(ok, "git init");
            dir.to_string_lossy().into_owned()
        })
        .collect()
}

#[test]
fn the_target_is_read_from_the_whole_arguments_like_the_round_line() {
    // count-review-rounds は引数の全体から最初の在るディレクトリを取る。PR の番号の後や次の行に書いた worktree を、依頼文の
    // 対象として行の worktree と比べる
    for (args, recorded) in [
        ("340 焦点: {wt} の差分", true),
        ("340\n{wt}", true),
        ("340\n{other}", false),
        ("340 焦点: {other} との違い", false),
    ] {
        let fake = FakeGh::new();
        reviewed_at(&fake, &[]);
        let [wt, other] = <[String; 2]>::try_from(repos(&fake, &["wt", "other"])).unwrap();
        std::fs::write(
            fake.dir().join(".git/claude-review-rounds/feat%2Fx"),
            format!("{HEAD} 1794900000 {SESSION} {wt}\n"),
        )
        .unwrap();
        let target = args.replace("{wt}", &wt).replace("{other}", &other);
        transcript_of(
            &fake,
            "a1",
            &target,
            "code-review",
            "low",
            AFTER,
            &[READ_DIFF],
        );
        let out = record_zero(&fake, HEAD, PR_DIFF, &[]);
        assert_eq!(
            out.status.code() == Some(0),
            recorded,
            "{args}: {}",
            stderr(&out)
        );
    }
}

#[test]
fn the_next_round_is_not_aimed_at_a_directory_outside_the_worktree() {
    // 焦点の文の `/` は在るディレクトリだが worktree でない。NEXT: の対象にせず、周の行の worktree を対象にする
    let fake = FakeGh::new();
    reviewed_in_session(&fake, &[HEAD]);
    transcript_of(
        &fake,
        "a1",
        "340 焦点: searched / failed",
        "code-review",
        "low",
        AFTER,
        &["nothing"],
    );
    let out = record_zero(&fake, HEAD, PR_DIFF, &[]);
    assert_eq!(out.status.code(), Some(1), "{}", stderr(&out));
    assert!(
        stdout(&out).contains(&format!("NEXT: /code-review medium {TARGET} ")),
        "{}",
        stdout(&out)
    );
}
