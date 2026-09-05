//! guard-gh-run-rerun（未移植: 次のコミットで bash 版から移す）

use super::Rule;
use crate::input::Input;
use crate::output::Finding;
use crate::shell::Shell;

pub struct GuardGhRunRerun;

impl Rule for GuardGhRunRerun {
    fn name(&self) -> &'static str {
        "guard-gh-run-rerun"
    }

    fn check(&self, _input: &Input, _shell: &Shell) -> Vec<Finding> {
        Vec::new()
    }
}
