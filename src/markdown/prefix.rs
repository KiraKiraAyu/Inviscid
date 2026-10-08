#[derive(Clone, Debug, PartialEq, Eq)]
pub enum BlockPrefix {
    Heading { level: u8 },
    Task { checked: bool, marker: char },
    Bullet { marker: char },
    Ordered { num: usize },
    Quote,
    Fence { lang: Option<String> },
}

pub fn parse_heading_prefix(trimmed: &str) -> Option<(u8, usize)> {
    let count = trimmed.bytes().take_while(|&b| b == b'#').count();
    if (1..=6).contains(&count) && trimmed[count..].starts_with(' ') {
        Some((count as u8, count + 1))
    } else {
        None
    }
}

pub fn parse_task_prefix(trimmed: &str) -> Option<(bool, char, usize)> {
    if trimmed.len() < 6 {
        return None;
    }
    let marker = trimmed.chars().next()?;
    if marker != '-' && marker != '*' {
        return None;
    }
    if trimmed.starts_with("- [ ] ") || trimmed.starts_with("* [ ] ") {
        Some((false, marker, 6))
    } else if trimmed.starts_with("- [x] ")
        || trimmed.starts_with("- [X] ")
        || trimmed.starts_with("* [x] ")
        || trimmed.starts_with("* [X] ")
    {
        Some((true, marker, 6))
    } else {
        None
    }
}

pub fn parse_bullet_prefix(trimmed: &str) -> Option<(char, usize)> {
    if trimmed.starts_with("- ") {
        Some(('-', 2))
    } else if trimmed.starts_with("* ") {
        Some(('*', 2))
    } else if trimmed.starts_with("+ ") {
        Some(('+', 2))
    } else {
        None
    }
}

pub fn parse_ordered_prefix(trimmed: &str) -> Option<(usize, usize)> {
    let dot_pos = trimmed.find(". ")?;
    if dot_pos > 0 && trimmed[..dot_pos].chars().all(|c| c.is_ascii_digit()) {
        let num = trimmed[..dot_pos].parse::<usize>().ok()?;
        Some((num, dot_pos + 2))
    } else {
        None
    }
}

pub fn parse_quote_prefix(trimmed: &str) -> Option<usize> {
    if trimmed.starts_with("> ") {
        Some(2)
    } else {
        None
    }
}

/// Returns the info string (trimmed, possibly empty) if `line` is a ``` fence line.
pub fn fence_info(line: &str) -> Option<&str> {
    line.trim_start().strip_prefix("```").map(str::trim)
}

#[inline]
pub fn is_fence_line(line: &str) -> bool {
    fence_info(line).is_some()
}

pub fn parse_fence_prefix(trimmed: &str) -> Option<(Option<String>, usize)> {
    let info = fence_info(trimmed)?;
    let lang_str = (!info.is_empty()).then(|| info.to_string());
    Some((lang_str, 3))
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ParsedPrefix {
    pub kind: BlockPrefix,
    pub total_len: usize,
    pub leading_spaces: usize,
}

pub fn parse_block_prefix(line: &str) -> Option<ParsedPrefix> {
    let trimmed = line.trim_start_matches([' ', '\t']);
    let leading = line.len() - trimmed.len();

    let (kind, prefix_len) = if let Some((level, len)) = parse_heading_prefix(trimmed) {
        (BlockPrefix::Heading { level }, len)
    } else if let Some((checked, marker, len)) = parse_task_prefix(trimmed) {
        (BlockPrefix::Task { checked, marker }, len)
    } else if let Some((marker, len)) = parse_bullet_prefix(trimmed) {
        (BlockPrefix::Bullet { marker }, len)
    } else if let Some((num, len)) = parse_ordered_prefix(trimmed) {
        (BlockPrefix::Ordered { num }, len)
    } else if let Some(len) = parse_quote_prefix(trimmed) {
        (BlockPrefix::Quote, len)
    } else if let Some((lang, len)) = parse_fence_prefix(trimmed) {
        (BlockPrefix::Fence { lang }, len)
    } else {
        return None;
    };

    Some(ParsedPrefix {
        kind,
        total_len: leading + prefix_len,
        leading_spaces: leading,
    })
}

pub fn get_block_prefix_len(line: &str) -> usize {
    if let Some(prefix) = parse_block_prefix(line) {
        prefix.total_len
    } else {
        line.chars().take_while(|c| *c == ' ' || *c == '\t').count()
    }
}

pub fn get_smart_continuation_prefix(line: &str) -> Option<String> {
    let prefix = parse_block_prefix(line)?;
    let indent = &line[..prefix.leading_spaces];

    match prefix.kind {
        BlockPrefix::Task { marker, .. } => Some(format!("{indent}{marker} [ ] ")),
        BlockPrefix::Bullet { marker } => Some(format!("{indent}{marker} ")),
        BlockPrefix::Ordered { num } => Some(format!("{indent}{}. ", num + 1)),
        BlockPrefix::Quote => Some(format!("{indent}> ")),
        _ => None,
    }
}

pub fn toggle_task_checkbox(line: &str) -> Option<String> {
    let prefix = parse_block_prefix(line)?;
    if let BlockPrefix::Task { checked, marker } = prefix.kind {
        let indent = &line[..prefix.leading_spaces];
        let content = &line[prefix.total_len..];
        let state = if checked { ' ' } else { 'x' };
        Some(format!("{indent}{marker} [{state}] {content}"))
    } else {
        None
    }
}

pub fn is_thematic_break(trimmed: &str) -> bool {
    (trimmed.starts_with("---") || trimmed.starts_with("***") || trimmed.starts_with("___"))
        && trimmed
            .chars()
            .all(|c| c == '-' || c == '*' || c == '_' || c == ' ')
        && trimmed.len() >= 3
}
