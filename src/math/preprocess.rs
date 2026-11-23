use std::fmt::Write as _;

use latex_rust::MathFont;

use super::fallback_font::{FallbackGlyph, FallbackSlot, lookup_fallback_glyph};

pub(crate) fn is_unicode_space(ch: char) -> bool {
    matches!(
        ch,
        '\u{00A0}' | '\u{2002}'..='\u{200A}' | '\u{202F}' | '\u{205F}' | '\u{3000}'
    )
}

pub(crate) fn is_text_mode_command(cmd: &str) -> bool {
    matches!(
        cmd,
        "text"
            | "textrm"
            | "textbf"
            | "textit"
            | "texttt"
            | "textsf"
            | "mathrm"
            | "mbox"
            | "operatorname"
    )
}

fn unicode_math_symbol_macro(ch: char) -> Option<&'static str> {
    Some(match ch {
        '\u{27F9}' => r"\Longrightarrow",
        '\u{27F8}' => r"\Longleftarrow",
        '\u{27FA}' => r"\Longleftrightarrow",
        '\u{21D2}' => r"\Rightarrow",
        '\u{21D0}' => r"\Leftarrow",
        '\u{21D4}' => r"\Leftrightarrow",
        '\u{2192}' => r"\rightarrow",
        '\u{2190}' => r"\leftarrow",
        '\u{2194}' => r"\leftrightarrow",
        '\u{21A6}' => r"\mapsto",
        '\u{2261}' => r"\equiv",
        '\u{2248}' => r"\approx",
        '\u{223C}' => r"\sim",
        '\u{221D}' => r"\propto",
        '\u{2208}' => r"\in",
        '\u{2209}' => r"\notin",
        '\u{2282}' => r"\subset",
        '\u{2286}' => r"\subseteq",
        '\u{222A}' => r"\cup",
        '\u{2229}' => r"\cap",
        '\u{2200}' => r"\forall",
        '\u{2203}' => r"\exists",
        '\u{00AC}' => r"\neg",
        '\u{2227}' => r"\wedge",
        '\u{2228}' => r"\vee",
        '\u{2295}' => r"\oplus",
        '\u{2297}' => r"\otimes",
        '\u{2211}' => r"\sum",
        '\u{220F}' => r"\prod",
        '\u{222B}' => r"\int",
        '\u{2202}' => r"\partial",
        '\u{2207}' => r"\nabla",
        '\u{22EF}' => r"\cdots",
        '\u{2026}' => r"\ldots",
        '\u{22C5}' | '\u{00B7}' => r"\cdot",
        _ => return None,
    })
}

fn read_tex_arg(chars: &[char], idx: &mut usize) -> Option<String> {
    let mut cursor = *idx;
    while cursor < chars.len()
        && (chars[cursor].is_ascii_whitespace() || is_unicode_space(chars[cursor]))
    {
        cursor += 1;
    }
    let &first = chars.get(cursor)?;
    if first == '{' {
        cursor += 1;
        let inner_start = cursor;
        let mut depth = 1usize;
        while cursor < chars.len() && depth > 0 {
            if chars[cursor] == '\\' && cursor + 1 < chars.len() {
                cursor += 2;
                continue;
            }
            match chars[cursor] {
                '{' => depth += 1,
                '}' => {
                    depth -= 1;
                    if depth == 0 {
                        let inner: String = chars[inner_start..cursor].iter().collect();
                        *idx = cursor + 1;
                        return Some(normalize_latex_inner(&inner, false));
                    }
                }
                _ => {}
            }
            cursor += 1;
        }
        None
    } else if first == '\\' {
        let cmd_start = cursor;
        cursor += 1;
        if cursor >= chars.len() {
            return None;
        }
        if chars[cursor].is_ascii_alphabetic() {
            while cursor < chars.len() && chars[cursor].is_ascii_alphabetic() {
                cursor += 1;
            }
        } else {
            cursor += 1;
        }
        *idx = cursor;
        let raw_cmd: String = chars[cmd_start..cursor].iter().collect();
        Some(raw_cmd)
    } else {
        *idx = cursor + 1;
        let ch = if is_unicode_space(first) { ' ' } else { first };
        if let Some(macro_str) = unicode_math_symbol_macro(ch) {
            Some(macro_str.to_string())
        } else {
            Some(ch.to_string())
        }
    }
}

fn push_macro_with_boundary(out: &mut String, macro_str: &str, next_char: Option<char>) {
    out.push_str(macro_str);
    if next_char.is_some_and(|ch| ch.is_ascii_alphabetic()) {
        out.push(' ');
    }
}

fn normalize_latex_inner(tex: &str, start_in_text: bool) -> String {
    let chars: Vec<char> = tex.chars().collect();
    let mut out = String::with_capacity(tex.len());
    let mut i = 0usize;
    let mut brace_depth = 0usize;
    let mut text_scope_depths: Vec<usize> = if start_in_text { vec![0] } else { Vec::new() };
    let mut pending_text_cmd = false;

    while i < chars.len() {
        let ch = chars[i];
        let in_text = !text_scope_depths.is_empty();

        if is_unicode_space(ch) {
            out.push(' ');
            i += 1;
            continue;
        }

        if ch == '\\' {
            if i + 1 >= chars.len() {
                out.push('\\');
                i += 1;
                continue;
            }
            let next = chars[i + 1];
            if !next.is_ascii_alphabetic() {
                out.push('\\');
                out.push(if is_unicode_space(next) { ' ' } else { next });
                i += 2;
                pending_text_cmd = false;
                continue;
            }

            let cmd_start = i + 1;
            let mut cmd_end = cmd_start;
            while cmd_end < chars.len() && chars[cmd_end].is_ascii_alphabetic() {
                cmd_end += 1;
            }
            let cmd: String = chars[cmd_start..cmd_end].iter().collect();

            if !in_text {
                match cmd.as_str() {
                    "pmod" => {
                        let mut arg_cursor = cmd_end;
                        if let Some(arg) = read_tex_arg(&chars, &mut arg_cursor) {
                            out.push_str(r"\quad(\mathrm{mod}\; ");
                            out.push_str(&arg);
                            out.push(')');
                            i = arg_cursor;
                            pending_text_cmd = false;
                            continue;
                        }
                    }
                    "pod" => {
                        let mut arg_cursor = cmd_end;
                        if let Some(arg) = read_tex_arg(&chars, &mut arg_cursor) {
                            out.push_str(r"\quad(");
                            out.push_str(&arg);
                            out.push(')');
                            i = arg_cursor;
                            pending_text_cmd = false;
                            continue;
                        }
                    }
                    "mod" => {
                        let mut arg_cursor = cmd_end;
                        if let Some(arg) = read_tex_arg(&chars, &mut arg_cursor) {
                            out.push_str(r"\quad\mathrm{mod}\; ");
                            out.push_str(&arg);
                            i = arg_cursor;
                            pending_text_cmd = false;
                            continue;
                        }
                    }
                    "bmod" => {
                        out.push_str(r"\;\mathrm{mod}\;");
                        i = cmd_end;
                        pending_text_cmd = false;
                        continue;
                    }
                    _ => {}
                }
            }

            out.push('\\');
            out.push_str(&cmd);
            pending_text_cmd = is_text_mode_command(&cmd);
            i = cmd_end;
            continue;
        }

        if pending_text_cmd && ch.is_ascii_whitespace() {
            out.push(ch);
            i += 1;
            continue;
        }

        if ch == '{' {
            brace_depth += 1;
            if pending_text_cmd {
                text_scope_depths.push(brace_depth);
                pending_text_cmd = false;
            }
            out.push('{');
            i += 1;
            continue;
        }

        pending_text_cmd = false;

        if ch == '}' {
            if text_scope_depths.last().copied() == Some(brace_depth) {
                text_scope_depths.pop();
            }
            brace_depth = brace_depth.saturating_sub(1);
            out.push('}');
            i += 1;
            continue;
        }

        if !in_text && let Some(macro_str) = unicode_math_symbol_macro(ch) {
            push_macro_with_boundary(&mut out, macro_str, chars.get(i + 1).copied());
            i += 1;
            continue;
        }

        out.push(ch);
        i += 1;
    }

    out
}

pub(crate) fn normalize_latex(tex: &str) -> String {
    normalize_latex_inner(tex, false)
}

fn needs_fallback(ch: char, math_font: &MathFont) -> bool {
    !ch.is_ascii() && math_font.glyph(ch).is_err()
}

fn next_slot(glyphs: Vec<FallbackGlyph>, slots: &mut Vec<FallbackSlot>) -> Option<FallbackSlot> {
    let slot_id = slots.len() + 1;
    if slot_id > 0xffff {
        return None;
    }
    let width_em = glyphs.iter().map(|g| g.advance_em).sum::<f32>().max(0.01);
    let r = 0xfe_u8;
    let g = ((slot_id >> 8) & 0xff) as u8;
    let b = (slot_id & 0xff) as u8;
    let color_hex = format!("#{r:02x}{g:02x}{b:02x}");
    let slot = FallbackSlot {
        color_hex,
        color_rgb: (r, g, b),
        width_em,
        glyphs,
    };
    slots.push(slot.clone());
    Some(slot)
}

fn push_placeholder_tex(out: &mut String, slot: &FallbackSlot, wrap_braces: bool) {
    let max_ascent_em = slot
        .glyphs
        .iter()
        .map(|g| g.ascent_em)
        .fold(0.70_f32, f32::max);
    let hex_digits = &slot.color_hex[1..];
    if wrap_braces {
        out.push('{');
    }
    let _ = write!(
        out,
        r"\textcolor[HTML]{{{hex_digits}}}{{\strut\hspace{{{:.4}em}}\rule{{0pt}}{{{:.4}em}}}}",
        slot.width_em, max_ascent_em
    );
    if wrap_braces {
        out.push('}');
    }
}

fn read_braced_raw(chars: &[char], idx: usize) -> Option<(String, usize)> {
    let mut cursor = idx;
    while cursor < chars.len() && chars[cursor].is_ascii_whitespace() {
        cursor += 1;
    }
    if chars.get(cursor).copied() != Some('{') {
        return None;
    }
    cursor += 1;
    let start = cursor;
    let mut depth = 1usize;
    while cursor < chars.len() && depth > 0 {
        if chars[cursor] == '\\' && cursor + 1 < chars.len() {
            cursor += 2;
            continue;
        }
        match chars[cursor] {
            '{' => depth += 1,
            '}' => {
                depth -= 1;
                if depth == 0 {
                    let body: String = chars[start..cursor].iter().collect();
                    return Some((body, cursor + 1));
                }
            }
            _ => {}
        }
        cursor += 1;
    }
    None
}

enum TextSeg {
    Plain(String),
    Fallback(Vec<FallbackGlyph>),
    InlineMath(String),
}

fn push_text_body_with_fallback(
    cmd: &str,
    body: &str,
    math_font: &MathFont,
    slots: &mut Vec<FallbackSlot>,
    out: &mut String,
) {
    if !body
        .chars()
        .any(|ch| needs_fallback(ch, math_font) && lookup_fallback_glyph(ch).is_some())
    {
        out.push('\\');
        out.push_str(cmd);
        out.push('{');
        out.push_str(body);
        out.push('}');
        return;
    }

    let chars: Vec<char> = body.chars().collect();
    let mut segments: Vec<TextSeg> = Vec::new();
    let mut plain = String::new();
    let mut i = 0usize;

    while i < chars.len() {
        let ch = chars[i];
        if ch == '\\' && i + 1 < chars.len() {
            let next = chars[i + 1];
            if next == '(' {
                let math_start = i + 2;
                let mut j = math_start;
                let mut found_end = None;
                while j + 1 < chars.len() {
                    if chars[j] == '\\' && chars[j + 1] == ')' {
                        found_end = Some(j);
                        break;
                    }
                    j += 1;
                }
                if let Some(math_end) = found_end {
                    if !plain.is_empty() {
                        segments.push(TextSeg::Plain(std::mem::take(&mut plain)));
                    }
                    let math_src: String = chars[math_start..math_end].iter().collect();
                    segments.push(TextSeg::InlineMath(math_src));
                    i = math_end + 2;
                    continue;
                }
            }
            plain.push('\\');
            plain.push(next);
            i += 2;
            continue;
        }

        if ch == '$' {
            let math_start = i + 1;
            let mut j = math_start;
            let mut found_end = None;
            while j < chars.len() {
                if chars[j] == '\\' && j + 1 < chars.len() {
                    j += 2;
                    continue;
                }
                if chars[j] == '$' {
                    found_end = Some(j);
                    break;
                }
                j += 1;
            }
            if let Some(math_end) = found_end {
                if !plain.is_empty() {
                    segments.push(TextSeg::Plain(std::mem::take(&mut plain)));
                }
                let math_src: String = chars[math_start..math_end].iter().collect();
                segments.push(TextSeg::InlineMath(math_src));
                i = math_end + 1;
                continue;
            }
        }

        if needs_fallback(ch, math_font)
            && let Some(first_glyph) = lookup_fallback_glyph(ch)
        {
            if !plain.is_empty() {
                segments.push(TextSeg::Plain(std::mem::take(&mut plain)));
            }
            let mut glyphs = vec![first_glyph];
            i += 1;
            while i < chars.len() {
                let next_ch = chars[i];
                if needs_fallback(next_ch, math_font)
                    && let Some(next_glyph) = lookup_fallback_glyph(next_ch)
                {
                    glyphs.push(next_glyph);
                    i += 1;
                } else {
                    break;
                }
            }
            segments.push(TextSeg::Fallback(glyphs));
            continue;
        }

        plain.push(ch);
        i += 1;
    }

    if !plain.is_empty() {
        segments.push(TextSeg::Plain(plain));
    }

    let need_outer_braces = segments.len() > 1;
    if need_outer_braces {
        out.push('{');
    }
    for seg in segments {
        match seg {
            TextSeg::Plain(text) => {
                out.push('\\');
                out.push_str(cmd);
                out.push('{');
                out.push_str(&text);
                out.push('}');
            }
            TextSeg::Fallback(glyphs) => {
                if let Some(slot) = next_slot(glyphs, slots) {
                    push_placeholder_tex(out, &slot, !need_outer_braces);
                }
            }
            TextSeg::InlineMath(math_src) => {
                let inner = extract_fallback_inner(&math_src, math_font, slots);
                out.push('{');
                out.push_str(&inner);
                out.push('}');
            }
        }
    }
    if need_outer_braces {
        out.push('}');
    }
}

fn extract_fallback_inner(
    tex: &str,
    math_font: &MathFont,
    slots: &mut Vec<FallbackSlot>,
) -> String {
    if tex.is_ascii() {
        return tex.to_owned();
    }

    let chars: Vec<char> = tex.chars().collect();
    let mut out = String::with_capacity(tex.len());
    let mut i = 0usize;

    while i < chars.len() {
        let ch = chars[i];
        if ch == '\\' {
            if i + 1 >= chars.len() {
                out.push('\\');
                i += 1;
                continue;
            }
            if !chars[i + 1].is_ascii_alphabetic() {
                out.push('\\');
                out.push(chars[i + 1]);
                i += 2;
                continue;
            }

            let cmd_start = i + 1;
            let mut cmd_end = cmd_start;
            while cmd_end < chars.len() && chars[cmd_end].is_ascii_alphabetic() {
                cmd_end += 1;
            }
            let cmd: String = chars[cmd_start..cmd_end].iter().collect();
            if matches!(cmd.as_str(), "text" | "mbox")
                && let Some((body, after)) = read_braced_raw(&chars, cmd_end)
            {
                push_text_body_with_fallback(&cmd, &body, math_font, slots, &mut out);
                i = after;
                continue;
            }

            out.push('\\');
            out.push_str(&cmd);
            i = cmd_end;
            continue;
        }

        if needs_fallback(ch, math_font)
            && let Some(first_glyph) = lookup_fallback_glyph(ch)
        {
            let mut glyphs = vec![first_glyph];
            i += 1;
            while i < chars.len() {
                let next_ch = chars[i];
                if needs_fallback(next_ch, math_font)
                    && let Some(next_glyph) = lookup_fallback_glyph(next_ch)
                {
                    glyphs.push(next_glyph);
                    i += 1;
                } else {
                    break;
                }
            }
            if let Some(slot) = next_slot(glyphs, slots) {
                push_placeholder_tex(&mut out, &slot, true);
            }
            continue;
        }

        out.push(ch);
        i += 1;
    }

    out
}

pub(crate) fn extract_fallback_slots(
    tex: &str,
    math_font: &MathFont,
) -> (String, Vec<FallbackSlot>) {
    if tex.is_ascii() {
        return (tex.to_owned(), Vec::new());
    }
    let mut slots = Vec::new();
    let rewritten = extract_fallback_inner(tex, math_font, &mut slots);
    (rewritten, slots)
}
