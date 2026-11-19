//! 中断（INT、TERM、HUP）されたときの後片付けを登録する（ADR-0101）。
//!
//! 受け方の crate（ctrlc）の名前はこのファイルの中にだけ置く。crate の名前は Ctrl-C だけを指すが、受けるのは TERM と
//! HUP を含む中断なので、呼ぶ側のコードから crate の名前で用途を読ませない。

/// 中断されたときの終了コード。どのシグナルかは受け方の crate が渡さないので、INT の 128 + 2 に揃える
pub const INTERRUPTED: i32 = 130;

/// 中断されたら cleanup を実行し、[`INTERRUPTED`] で終える。cleanup は中断を受けた別のスレッドで走るので、本体の
/// スレッドと分け合う状態は Mutex などで守る。1 つのプロセスで 1 回だけ呼べる（2 回目は Err）。
///
/// 起動したときに無視するよう引き継いだシグナル（nohup の HUP、ジョブ制御の無いシェルが `&` で起動したコマンドの
/// INT）は、無視したままにする。ctrlc は 3 つとも受ける形に書き換えるので、登録の後に無視へ戻す（登録から戻すまでの
/// 間に届いたものは、後片付けをして終わる）。sh の trap と同じく、無視して起動されたコマンドが、親が無視させた中断で
/// 止まらないため
pub fn cleanup_on_interrupt(cleanup: impl Fn() + Send + 'static) -> Result<(), String> {
    #[cfg(unix)]
    let ignored = unix::ignored();
    ctrlc::set_handler(move || {
        cleanup();
        std::process::exit(INTERRUPTED);
    })
    .map_err(|e| e.to_string())?;
    #[cfg(unix)]
    unix::ignore(&ignored);
    Ok(())
}

/// cmd の子に、端末からの中断（INT、QUIT）を無視させる。ジョブ制御の無いシェルが `&` で起動したコマンドと同じにし、
/// 端末の Ctrl-C で、終わりまで待つ子（sbatch --wait）を止めない（Windows では何もしない）
pub fn ignore_terminal_interrupt(cmd: &mut std::process::Command) {
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        // SAFETY: fork の後、exec の前の子で signal を呼ぶだけで、メモリを確保せず、ロックを取らない
        unsafe {
            cmd.pre_exec(|| {
                unix::ignore(&[unix::SIGINT, unix::SIGQUIT]);
                Ok(())
            });
        }
    }
    #[cfg(not(unix))]
    let _ = cmd;
}

/// 読む側が先に閉じたパイプ（`| head -1`）に書いても SIGPIPE で終わらず、書き込みのエラー（BrokenPipe）として受ける。
/// 書いた側が後片付けをしてから終えるため。起動する子は、std が SIGPIPE を既定に戻して起動するので変わらない
/// （Windows には SIGPIPE が無く、何もしない）
pub fn broken_pipe_as_error() {
    #[cfg(unix)]
    unix::ignore(&[unix::SIGPIPE]);
}

#[cfg(unix)]
mod unix {
    use libc::{SIG_IGN, SIGHUP, SIGTERM, c_int};
    pub use libc::{SIGINT, SIGPIPE, SIGQUIT};

    /// 中断のシグナルのうち、無視するよう引き継いだもの（設定は変えずに読む）
    pub fn ignored() -> Vec<c_int> {
        [SIGHUP, SIGINT, SIGTERM]
            .into_iter()
            .filter(|&sig| {
                // SAFETY: sigaction は整数とポインタだけの C の構造体で、全部が 0 の値も正しい値（書き込み先に使うだけ）
                let mut old: libc::sigaction = unsafe { std::mem::zeroed() };
                // SAFETY: 新しい設定に null を渡し、今の設定を old に読むだけで、何も変えない
                let read = unsafe { libc::sigaction(sig, std::ptr::null(), &mut old) };
                read == 0 && old.sa_sigaction == SIG_IGN
            })
            .collect()
    }

    pub fn ignore(sigs: &[c_int]) {
        for &sig in sigs {
            // SAFETY: 無視に替えるだけで、ハンドラの関数は渡さない（fork の後の子でも呼べる）
            unsafe { libc::signal(sig, SIG_IGN) };
        }
    }
}
