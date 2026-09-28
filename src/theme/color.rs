use anyhow::{Result, anyhow};
use gpui::{Hsla, Rgba};
use serde::{Deserialize, Deserializer};

/// Parses a hex color string (`#RGB`, `#RGBA`, `#RRGGBB`, `#RRGGBBAA`) into a GPUI [`Hsla`] color.
pub fn parse_hex_color(hex_str: &str) -> Result<Hsla> {
    let clean = hex_str.trim().trim_start_matches('#');
    if !clean.is_ascii() || !clean.chars().all(|c| c.is_ascii_hexdigit()) {
        return Err(anyhow!("Invalid hex color format: '{}'", hex_str));
    }

    let (r, g, b, a) = match clean.len() {
        3 => {
            let r = u8::from_str_radix(&clean[0..1], 16)? * 17;
            let g = u8::from_str_radix(&clean[1..2], 16)? * 17;
            let b = u8::from_str_radix(&clean[2..3], 16)? * 17;
            (r, g, b, 255u8)
        }
        4 => {
            let r = u8::from_str_radix(&clean[0..1], 16)? * 17;
            let g = u8::from_str_radix(&clean[1..2], 16)? * 17;
            let b = u8::from_str_radix(&clean[2..3], 16)? * 17;
            let a = u8::from_str_radix(&clean[3..4], 16)? * 17;
            (r, g, b, a)
        }
        6 => {
            let r = u8::from_str_radix(&clean[0..2], 16)?;
            let g = u8::from_str_radix(&clean[2..4], 16)?;
            let b = u8::from_str_radix(&clean[4..6], 16)?;
            (r, g, b, 255u8)
        }
        8 => {
            let r = u8::from_str_radix(&clean[0..2], 16)?;
            let g = u8::from_str_radix(&clean[2..4], 16)?;
            let b = u8::from_str_radix(&clean[4..6], 16)?;
            let a = u8::from_str_radix(&clean[6..8], 16)?;
            (r, g, b, a)
        }
        _ => return Err(anyhow!("Invalid hex color format: '{}'", hex_str)),
    };

    let rgba = Rgba {
        r: (r as f32) / 255.0,
        g: (g as f32) / 255.0,
        b: (b as f32) / 255.0,
        a: (a as f32) / 255.0,
    };

    Ok(rgba.into())
}

/// Serde deserializer for converting hex strings directly to GPUI [`Hsla`].
pub fn deserialize_hsla_hex<'de, D>(deserializer: D) -> std::result::Result<Hsla, D::Error>
where
    D: Deserializer<'de>,
{
    let s = String::deserialize(deserializer)?;
    parse_hex_color(&s).map_err(serde::de::Error::custom)
}

#[derive(Clone, Copy, Debug, PartialEq, Default, Deserialize)]
#[serde(transparent)]
pub struct HexColor(#[serde(deserialize_with = "deserialize_hsla_hex")] pub Hsla);

impl From<HexColor> for Hsla {
    fn from(c: HexColor) -> Self {
        c.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_hex_color() {
        let c1 = parse_hex_color("#1e1e2e").unwrap();
        assert!((c1.a - 1.0).abs() < 0.001);

        let c2 = parse_hex_color("#89b4fa44").unwrap();
        assert!((c2.a - (0x44 as f32 / 255.0)).abs() < 0.01);

        let c3 = parse_hex_color("#fff").unwrap();
        assert!((c3.a - 1.0).abs() < 0.001);

        let c4 = parse_hex_color("#fff8").unwrap();
        assert!((c4.a - (0x88 as f32 / 255.0)).abs() < 0.01);

        assert!(parse_hex_color("invalid").is_err());
        assert!(parse_hex_color("#12345").is_err());
        assert!(parse_hex_color("#açb").is_err());
        assert!(parse_hex_color("#中文12").is_err());
        assert!(parse_hex_color("#1234567z").is_err());
    }
}
