use crate::markdown::structure::LineStructure;

/// Unidirectional prefix block state before scanning a line.
#[repr(u8)]
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub(crate) enum BlockState {
    #[default]
    Outside = 0,
    RawCode = 1,
    QuotedCode = 2,
    Math = 3,
}

impl BlockState {
    pub(crate) const COUNT: usize = 4;

    #[inline]
    pub(crate) const fn is_code(self) -> bool {
        matches!(self, Self::RawCode | Self::QuotedCode)
    }

    #[inline]
    pub(crate) const fn is_raw_code(self) -> bool {
        matches!(self, Self::RawCode)
    }

    #[inline]
    pub(crate) const fn is_math(self) -> bool {
        matches!(self, Self::Math)
    }

    #[inline]
    pub(crate) const fn from_usize(index: usize) -> Self {
        match index {
            1 => Self::RawCode,
            2 => Self::QuotedCode,
            3 => Self::Math,
            _ => Self::Outside,
        }
    }
}

/// Summary of transitions and opened/closed blocks across a line segment
/// when entered from a given incoming `BlockState`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct PrefixTransition {
    pub exit: BlockState,
    pub code_opens: usize,
    pub code_closes: usize,
    pub math_opens: usize,
    pub math_closes: usize,
}

/// Pure prefix-only block-state monoid over a line segment (zero lookahead).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct PrefixSummary(pub [PrefixTransition; BlockState::COUNT]);

impl Default for PrefixSummary {
    fn default() -> Self {
        Self::neutral()
    }
}

impl PrefixSummary {
    pub const fn neutral() -> Self {
        Self([
            PrefixTransition {
                exit: BlockState::Outside,
                code_opens: 0,
                code_closes: 0,
                math_opens: 0,
                math_closes: 0,
            },
            PrefixTransition {
                exit: BlockState::RawCode,
                code_opens: 0,
                code_closes: 0,
                math_opens: 0,
                math_closes: 0,
            },
            PrefixTransition {
                exit: BlockState::QuotedCode,
                code_opens: 0,
                code_closes: 0,
                math_opens: 0,
                math_closes: 0,
            },
            PrefixTransition {
                exit: BlockState::Math,
                code_opens: 0,
                code_closes: 0,
                math_opens: 0,
                math_closes: 0,
            },
        ])
    }

    pub fn line(raw: LineStructure, projected: LineStructure) -> Self {
        Self(std::array::from_fn(|incoming| {
            let state = BlockState::from_usize(incoming);
            match state {
                BlockState::Outside => {
                    if raw.is_fence {
                        PrefixTransition {
                            exit: BlockState::RawCode,
                            code_opens: 1,
                            ..Default::default()
                        }
                    } else if projected.is_fence {
                        PrefixTransition {
                            exit: BlockState::QuotedCode,
                            code_opens: 1,
                            ..Default::default()
                        }
                    } else if projected.is_math_delim {
                        PrefixTransition {
                            exit: BlockState::Math,
                            math_opens: 1,
                            ..Default::default()
                        }
                    } else {
                        PrefixTransition {
                            exit: BlockState::Outside,
                            ..Default::default()
                        }
                    }
                }
                BlockState::RawCode => {
                    if raw.is_fence {
                        PrefixTransition {
                            exit: BlockState::Outside,
                            code_closes: 1,
                            ..Default::default()
                        }
                    } else {
                        PrefixTransition {
                            exit: BlockState::RawCode,
                            ..Default::default()
                        }
                    }
                }
                BlockState::QuotedCode => {
                    if projected.is_fence {
                        PrefixTransition {
                            exit: BlockState::Outside,
                            code_closes: 1,
                            ..Default::default()
                        }
                    } else {
                        PrefixTransition {
                            exit: BlockState::QuotedCode,
                            ..Default::default()
                        }
                    }
                }
                BlockState::Math => {
                    if projected.is_math_delim {
                        PrefixTransition {
                            exit: BlockState::Outside,
                            math_closes: 1,
                            ..Default::default()
                        }
                    } else {
                        PrefixTransition {
                            exit: BlockState::Math,
                            ..Default::default()
                        }
                    }
                }
            }
        }))
    }

    pub fn append(&mut self, other: &Self) {
        for left in &mut self.0 {
            let right = other.0[left.exit as usize];
            left.exit = right.exit;
            left.code_opens += right.code_opens;
            left.code_closes += right.code_closes;
            left.math_opens += right.math_opens;
            left.math_closes += right.math_closes;
        }
    }
}
