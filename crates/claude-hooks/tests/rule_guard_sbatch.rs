//! guard-sbatch が、/slurm-run の「順番」と「複数のジョブを待つ」で使わない Slurm の操作と出力の扱いを止め、
//! 代わりの手を示すこと

use std::io::Write;
use std::process::{Command, Stdio};

/// command を guard-sbatch だけに background の Bash として掛け、止めた理由を返す（止めなければ None）
fn deny_reason(command: &str) -> Option<String> {
    let input = serde_json::json!({
        "tool_name": "Bash",
        "tool_input": {"command": command, "run_in_background": true},
        "cwd": env!("CARGO_TARGET_TMPDIR"),
    });
    let mut child = Command::new(env!("CARGO_BIN_EXE_claude-hooks"))
        .args(["pre-tool-use", "--rule", "guard-sbatch"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .unwrap();
    child
        .stdin
        .take()
        .unwrap()
        .write_all(input.to_string().as_bytes())
        .unwrap();
    let out = child.wait_with_output().unwrap();
    assert!(out.status.success());
    let stdout = String::from_utf8(out.stdout).unwrap();
    if stdout.trim().is_empty() {
        return None;
    }
    let json: serde_json::Value = serde_json::from_str(&stdout).unwrap();
    json["hookSpecificOutput"]["permissionDecisionReason"]
        .as_str()
        .map(str::to_string)
}

const RUN: &str = "cd ~/github/xlc && ~/.claude/scripts/slurm-run.sh --class trial --ref '#1' --purpose 'p' -t 5 xlc-a 'true'";

#[test]
fn scontrol_hold_release_and_suspend_are_denied_with_the_ways_to_change_the_order() {
    for command in [
        "scontrol hold 42",
        "scontrol release 42",
        "scontrol uhold 42",
        "scontrol suspend 42",
        "scontrol -o hold 42,43",
        "sudo scontrol suspend 42",
    ] {
        let reason = deny_reason(command).unwrap_or_else(|| panic!("{command} が止まらない"));
        assert!(reason.contains("--top"), "{command}: {reason}");
        assert!(reason.contains("ORDER:"), "{command}: {reason}");
        assert!(reason.contains("--timed"), "{command}: {reason}");
    }
}

#[test]
fn other_scontrol_commands_pass() {
    for command in [
        "scontrol top 42,43",
        "scontrol show job 42",
        "scontrol show config",
        "squeue -o '%i %T'",
    ] {
        assert_eq!(deny_reason(command), None, "{command}");
    }
}

#[test]
fn scancel_by_user_name_or_state_is_denied_with_cancelling_one_number_at_a_time() {
    for command in [
        "scancel -u tagawa",
        "scancel --user=tagawa",
        "scancel -n xlc-a",
        "scancel --name xlc-a",
        "scancel --jobname=xlc-a",
        "scancel --me",
        "scancel -t PENDING",
        "scancel --state=PENDING",
        "scancel -p main",
        "scancel --partition=main",
    ] {
        let reason = deny_reason(command).unwrap_or_else(|| panic!("{command} が止まらない"));
        assert!(reason.contains("scancel <番号>"), "{command}: {reason}");
    }
}

#[test]
fn scancel_with_job_numbers_passes() {
    for command in ["scancel 42", "scancel 42 43", "scancel --signal=INT 42"] {
        assert_eq!(deny_reason(command), None, "{command}");
    }
}

#[test]
fn slurm_run_output_into_a_pipe_or_a_display_command_is_denied() {
    for command in [
        format!("{RUN} | tail -n 5"),
        format!("{RUN} 2>&1 | grep TIME"),
        format!("{RUN}; tail -n 3 out.txt"),
        format!("{RUN} && cat out.txt"),
        format!("{RUN} || head log.txt"),
        format!("cd ~/github/xlc && {{ {RUN} & {RUN} & wait; }}; tail -n 3 a.txt b.txt"),
    ] {
        let reason = deny_reason(&command).unwrap_or_else(|| panic!("{command} が止まらない"));
        assert!(reason.contains("別の呼び出し"), "{command}: {reason}");
    }
}

#[test]
fn slurm_run_alone_and_display_commands_before_it_pass() {
    for command in [
        RUN.to_string(),
        format!("cat notes.txt && {RUN}"),
        format!("{RUN} && {RUN}"),
        format!("cd ~/github/xlc && {{ {RUN} & {RUN} & wait; }}"),
        "~/.claude/scripts/slurm-run.sh --class trial --ref '#1' --purpose \"$(cat p.txt)\" -t 5 xlc-a 'true'"
            .to_string(),
        "~/.claude/scripts/slurm-run.sh --class trial --ref '#1' --purpose p -t 5 xlc-a 'make | tail -n 3; cat out'"
            .to_string(),
    ] {
        assert_eq!(deny_reason(&command), None, "{command}");
    }
}
