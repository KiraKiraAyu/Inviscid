use crate::markdown::structure::LineStructure;

const OUTSIDE: usize = 0;
const CODE: usize = 1;
const MATH: usize = 2;

#[cfg(test)]
thread_local! {
    pub(super) static COMPOSITIONS: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

#[derive(Clone, Copy, Debug, Default)]
pub(super) struct Lookahead {
    fence: bool,
    odd_fences: bool,
    math: [bool; 2],
}

impl Lookahead {
    pub fn new(fences: usize, math: [bool; 2]) -> Self {
        Self {
            fence: fences > 0,
            odd_fences: fences % 2 == 1,
            math,
        }
    }

    pub fn has_math(self) -> bool {
        self.math[0]
    }

    fn prepend(self, suffix: Self) -> Self {
        Self {
            fence: self.fence || suffix.fence,
            odd_fences: self.odd_fences ^ suffix.odd_fences,
            math: std::array::from_fn(|state| {
                self.math[state] || suffix.math[state ^ usize::from(self.odd_fences)]
            }),
        }
    }

    fn key(self) -> usize {
        usize::from(self.fence)
            | (usize::from(self.math[0]) << 1)
            | (usize::from(self.math[1]) << 2)
    }

    fn from_key(key: usize) -> Self {
        Self {
            fence: key & 1 != 0,
            odd_fences: false,
            math: [key & 2 != 0, key & 4 != 0],
        }
    }
}

#[derive(Clone, Copy, Debug)]
pub(super) struct ParseEffect {
    state: usize,
    pub starts: usize,
    pub ends: usize,
}

/// Scanner transitions for each suffix lookahead and incoming parser state.
/// The lookahead preserves the scanner's treatment of unmatched delimiters.
#[derive(Clone, Debug)]
pub(super) struct ParserSummary {
    pub lookahead: Lookahead,
    effects: [[ParseEffect; 3]; 8],
}

impl Default for ParserSummary {
    fn default() -> Self {
        Self {
            lookahead: Lookahead::default(),
            effects: std::array::from_fn(|_| {
                std::array::from_fn(|state| ParseEffect {
                    state,
                    starts: 0,
                    ends: 0,
                })
            }),
        }
    }
}

impl ParserSummary {
    fn line(syntax: LineStructure) -> Self {
        Self {
            lookahead: Lookahead {
                fence: syntax.is_fence,
                odd_fences: syntax.is_fence,
                math: [syntax.is_math_delim, false],
            },
            effects: std::array::from_fn(|key| {
                let suffix = Lookahead::from_key(key);
                std::array::from_fn(|state| {
                    let next = match state {
                        MATH if syntax.is_math_delim => OUTSIDE,
                        MATH => MATH,
                        CODE if syntax.is_fence => OUTSIDE,
                        CODE => CODE,
                        _ if syntax.is_fence => CODE,
                        _ if syntax.is_math_delim && suffix.math[0] => MATH,
                        _ => OUTSIDE,
                    };
                    ParseEffect {
                        state: next,
                        starts: usize::from(state == OUTSIDE && next != OUTSIDE),
                        ends: usize::from(state != OUTSIDE && next == OUTSIDE),
                    }
                })
            }),
        }
    }

    pub fn append(&mut self, other: &Self) {
        #[cfg(test)]
        COMPOSITIONS.update(|count| count + 1);
        self.effects = std::array::from_fn(|key| {
            let through_other = other.lookahead.prepend(Lookahead::from_key(key));
            std::array::from_fn(|state| {
                let left = self.effects[through_other.key()][state];
                let right = other.effects[key][left.state];
                ParseEffect {
                    state: right.state,
                    starts: left.starts + right.starts,
                    ends: left.ends + right.ends,
                }
            })
        });
        self.lookahead = self.lookahead.prepend(other.lookahead);
    }

    pub fn with_suffix(&self, suffix: Lookahead) -> ParseEffect {
        self.effects[suffix.key()][OUTSIDE]
    }
}

/// Raw-fence projection is independent of the scanner's code/math state.
/// Keeping both incoming raw parities also handles the unpaired final fence.
#[derive(Clone, Debug, Default)]
pub(super) struct ParserEffects {
    pub normal: [ParserSummary; 2],
    pub projected: ParserSummary,
    pub through_last_fence: [ParserSummary; 2],
    pub after_last_fence: ParserSummary,
    raw_fences: usize,
}

impl ParserEffects {
    pub fn line(raw: LineStructure, projected: LineStructure) -> Self {
        let normal = [ParserSummary::line(projected), ParserSummary::line(raw)];
        let projected = ParserSummary::line(projected);
        Self {
            through_last_fence: if raw.is_fence {
                normal.clone()
            } else {
                Default::default()
            },
            after_last_fence: if raw.is_fence {
                Default::default()
            } else {
                projected.clone()
            },
            normal,
            projected,
            raw_fences: usize::from(raw.is_fence),
        }
    }

    pub fn append(&mut self, other: &Self) {
        let parity = self.raw_fences % 2;
        if other.raw_fences > 0 {
            self.through_last_fence = std::array::from_fn(|state| {
                let mut prefix = self.normal[state].clone();
                prefix.append(&other.through_last_fence[state ^ parity]);
                prefix
            });
            self.after_last_fence = other.after_last_fence.clone();
        } else {
            self.after_last_fence.append(&other.projected);
        }
        for state in 0..2 {
            self.normal[state].append(&other.normal[state ^ parity]);
        }
        self.projected.append(&other.projected);
        self.raw_fences += other.raw_fences;
    }
}
