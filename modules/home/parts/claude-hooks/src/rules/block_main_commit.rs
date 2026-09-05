//! block-main-commit（未移植: 次のコミットで bash 版から移す）

use super::Rule;
use crate::input::Input;
use crate::output::Finding;
use crate::shell::Shell;

pub struct BlockMainCommit;

impl Rule for BlockMainCommit {
    fn name(&self) -> &'static str {
        "block-main-commit"
    }

    fn check(&self, _input: &Input, _shell: &Shell) -> Vec<Finding> {
        Vec::new()
    }
}
