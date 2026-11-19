//! 中断（INT、TERM、HUP）されたときの後片付けを登録する（ADR-0101）。
//!
//! 受け方の crate（ctrlc）の名前はこのファイルの中にだけ置く。crate の名前は Ctrl-C だけを指すが、受けるのは TERM と
//! HUP を含む中断なので、呼ぶ側のコードから crate の名前で用途を読ませない。

/// 中断されたときの終了コード。どのシグナルかは受け方の crate が渡さないので、INT の 128 + 2 に揃える
pub const INTERRUPTED: i32 = 130;

/// 中断されたら cleanup を実行し、[`INTERRUPTED`] で終える。cleanup は中断を受けた別のスレッドで走るので、本体の
/// スレッドと分け合う状態は Mutex などで守る。1 つのプロセスで 1 回だけ呼べる（2 回目は Err）
pub fn cleanup_on_interrupt(cleanup: impl Fn() + Send + 'static) -> Result<(), String> {
    ctrlc::set_handler(move || {
        cleanup();
        std::process::exit(INTERRUPTED);
    })
    .map_err(|e| e.to_string())
}
