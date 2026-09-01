//! Colour parsing and WCAG contrast, shared by the catalog theme validator and
//! the per-account accent colour.

/// Parses a `#rrggbb` string. Any other shape is rejected.
pub fn parse_color(value: &str) -> Option<[u8; 3]> {
    if value.len() != 7 || !value.starts_with('#') {
        return None;
    }
    Some([
        u8::from_str_radix(&value[1..3], 16).ok()?,
        u8::from_str_radix(&value[3..5], 16).ok()?,
        u8::from_str_radix(&value[5..7], 16).ok()?,
    ])
}

fn relative_luminance(color: [u8; 3]) -> f64 {
    let channel = |value: u8| {
        let value = f64::from(value) / 255.0;
        if value <= 0.04045 {
            value / 12.92
        } else {
            ((value + 0.055) / 1.055).powf(2.4)
        }
    };
    0.2126 * channel(color[0]) + 0.7152 * channel(color[1]) + 0.0722 * channel(color[2])
}

/// WCAG 2.x contrast ratio, between 1.0 and 21.0.
pub fn contrast_ratio(left: [u8; 3], right: [u8; 3]) -> f64 {
    let left = relative_luminance(left);
    let right = relative_luminance(right);
    (left.max(right) + 0.05) / (left.min(right) + 0.05)
}

/// Contrast between two `#rrggbb` strings. Returns `None` if either is malformed.
pub fn contrast_ratio_hex(left: &str, right: &str) -> Option<f64> {
    Some(contrast_ratio(parse_color(left)?, parse_color(right)?))
}

/// Minimum contrast for a colour used as a user interface component boundary —
/// borders, underlines, icons and focus rings (WCAG 1.4.11). The accent colour
/// is never used as body text, so the 4.5 text threshold does not apply to it.
pub const UI_COMPONENT_CONTRAST: f64 = 3.0;

/// Minimum contrast for text (WCAG 1.4.3, normal size).
pub const TEXT_CONTRAST: f64 = 4.5;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_only_full_length_hexadecimal_colours() {
        assert_eq!(parse_color("#ffffff"), Some([255, 255, 255]));
        assert_eq!(parse_color("#000000"), Some([0, 0, 0]));
        assert_eq!(parse_color("#FFF"), None);
        assert_eq!(parse_color("ffffff"), None);
        assert_eq!(parse_color("#gggggg"), None);
    }

    #[test]
    fn contrast_matches_the_wcag_reference_values() {
        let black_on_white = contrast_ratio([0, 0, 0], [255, 255, 255]);
        assert!((black_on_white - 21.0).abs() < 0.01);
        assert!((contrast_ratio([255, 255, 255], [255, 255, 255]) - 1.0).abs() < 0.01);
    }

    #[test]
    fn contrast_is_symmetric() {
        let forward = contrast_ratio([79, 70, 229], [0, 0, 0]);
        let backward = contrast_ratio([0, 0, 0], [79, 70, 229]);
        assert!((forward - backward).abs() < f64::EPSILON);
    }

    #[test]
    fn the_shipped_default_accent_clears_the_component_threshold() {
        let ratio = contrast_ratio_hex("#3A82F6", "#000000").expect("valid colours");
        assert!(ratio >= UI_COMPONENT_CONTRAST, "ratio was {ratio}");
    }

    #[test]
    fn a_near_black_accent_is_rejected_by_the_component_threshold() {
        let ratio = contrast_ratio_hex("#0a0a0a", "#000000").expect("valid colours");
        assert!(ratio < UI_COMPONENT_CONTRAST, "ratio was {ratio}");
    }

    #[test]
    fn malformed_input_yields_no_ratio() {
        assert_eq!(contrast_ratio_hex("#zzz", "#000000"), None);
    }
}
