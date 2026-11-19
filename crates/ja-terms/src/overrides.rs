//! リポジトリの辞書を共通の辞書に重ねたとき（ADR-0096。[`crate::Dictionary::merge`]）に外れる、共通の辞書の規則。
//!
//! 重ね方は expected が同じかだけを見るので、リポジトリの辞書に同じ expected で pattern の違う規則を足すと、共通の規則が
//! 何も言わずに外れる。外れた規則を、リポジトリの辞書に同じ pattern の規則があるもの（共通の辞書から移す途中の、意図した
//! 重なり）と、無いもの（置き換え。意図しない置き換えの疑い）に分けて出す。

use crate::{Dictionary, Rule};

/// 外れた共通の規則と、それを外したリポジトリの規則
#[derive(Debug)]
pub struct Overridden<'a> {
    common: &'a Rule,
    /// 同じ expected のリポジトリの規則（辞書の順）
    by: Vec<&'a Rule>,
}

/// common の規則のうち、repo を前に重ねると外れるもの（common の順）
pub fn overridden<'a>(repo: &'a Dictionary, common: &'a Dictionary) -> Vec<Overridden<'a>> {
    common
        .rules
        .iter()
        .filter(|rule| repo.has_expected(&rule.expected))
        .map(|rule| Overridden {
            common: rule,
            by: repo
                .rules
                .iter()
                .filter(|r| r.expected == rule.expected)
                .collect(),
        })
        .collect()
}

impl Overridden<'_> {
    /// 同じ pattern のリポジトリの規則（あれば最初のもの）。当たり方が同じなので、外れても検査は変わらない
    fn same(&self) -> Option<&Rule> {
        self.by
            .iter()
            .find(|r| r.source == self.common.source)
            .copied()
    }

    /// リポジトリの辞書に同じ pattern の規則が無い（共通の規則の当たる語を、もう止めない）
    pub fn is_replaced(&self) -> bool {
        self.same().is_none()
    }

    /// 1 行の説明。置き換えは `REPLACED: `、同じ pattern の規則があれば `SAME: ` で始める
    pub fn line(&self) -> String {
        let c = self.common;
        let head = format!(
            "{} /{}/（共通の辞書の {} 番目の規則）を、",
            c.expected, c.source, c.position
        );
        match self.same() {
            Some(same) => {
                // specs は当たり方を変えないが、移した後に共通の辞書で直した例を持ち越していないことがある
                let specs = if same.specs == c.specs {
                    ""
                } else {
                    "（specs は違う）"
                };
                format!(
                    "SAME: {head}同じ pattern のリポジトリの辞書の {} 番目の規則{specs}が外す",
                    same.position
                )
            }
            None => {
                let by: Vec<String> = self
                    .by
                    .iter()
                    .map(|r| {
                        format!(
                            "/{}/（リポジトリの辞書の {} 番目の規則）",
                            r.source, r.position
                        )
                    })
                    .collect();
                format!("REPLACED: {head}{}が外す", by.join("、"))
            }
        }
    }
}
