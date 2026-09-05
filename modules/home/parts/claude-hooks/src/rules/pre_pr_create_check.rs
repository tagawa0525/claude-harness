//! pre-pr-create-check（未移植: 次のコミットで bash 版から移す）

use super::Rule;
use crate::input::Input;
use crate::output::Finding;
use crate::shell::Shell;

pub struct PrePrCreateCheck;

impl Rule for PrePrCreateCheck {
    fn name(&self) -> &'static str {
        "pre-pr-create-check"
    }

    fn check(&self, _input: &Input, _shell: &Shell) -> Vec<Finding> {
        Vec::new()
    }
}
