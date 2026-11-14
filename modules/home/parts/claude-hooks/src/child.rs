//! 子のプロセスの終わり方を、呼んだスクリプトの終了コードにする

/// 子の終了コード。シグナルで終わったら、sh と同じく 128 + シグナルの番号にする（`| head` で読む側が閉じ、
/// 標準出力を引き継いだ子が SIGPIPE で終われば 141。1 と読み替えると本物の失敗と見分けられない）
pub fn exit_code(status: std::process::ExitStatus) -> i32 {
    #[cfg(unix)]
    {
        use std::os::unix::process::ExitStatusExt;
        if let Some(sig) = status.signal() {
            return 128 + sig;
        }
    }
    status.code().unwrap_or(1)
}

#[cfg(test)]
mod tests {
    use super::exit_code;

    #[cfg(unix)]
    #[test]
    fn a_child_ended_by_a_signal_gives_128_plus_the_signal_like_sh() {
        use std::os::unix::process::ExitStatusExt;
        assert_eq!(exit_code(std::process::ExitStatus::from_raw(13)), 141);
        assert_eq!(exit_code(std::process::ExitStatus::from_raw(1 << 8)), 1);
        assert_eq!(exit_code(std::process::ExitStatus::from_raw(0)), 0);
    }
}
