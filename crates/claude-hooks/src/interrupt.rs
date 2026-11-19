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
/// INT）は、無視したままにする。ctrlc は 3 つとも受ける形に書き換えるので、登録の後に無視へ戻す。sh の trap と同じく、
/// 無視して起動されたコマンドが、親が無視させた中断で止まらないため
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

#[cfg(unix)]
mod unix {
    use std::ffi::c_int;

    unsafe extern "C" {
        fn signal(signum: c_int, handler: usize) -> usize;
    }

    // Linux、macOS、BSD で同じ値（signal.h）
    const SIGHUP: c_int = 1;
    pub const SIGINT: c_int = 2;
    pub const SIGQUIT: c_int = 3;
    const SIGTERM: c_int = 15;
    const SIG_IGN: usize = 1;

    /// 中断のシグナルのうち、無視するよう引き継いだもの。調べるには一度無視に替えて前の値を受けるしか無いので、
    /// 無視でなかったものは前の値に戻す（ハンドラを登録する前で、前の値は既定か無視のどちらか）
    pub fn ignored() -> Vec<c_int> {
        let mut ignored = Vec::new();
        for sig in [SIGHUP, SIGINT, SIGTERM] {
            // SAFETY: 既定か無視の値を出し入れするだけで、ハンドラの関数は渡さない
            let old = unsafe { signal(sig, SIG_IGN) };
            if old == SIG_IGN {
                ignored.push(sig);
            } else {
                // SAFETY: 同上（今の前の値に戻す）
                unsafe { signal(sig, old) };
            }
        }
        ignored
    }

    pub fn ignore(sigs: &[c_int]) {
        for &sig in sigs {
            // SAFETY: 無視に替えるだけで、ハンドラの関数は渡さない
            unsafe { signal(sig, SIG_IGN) };
        }
    }
}
