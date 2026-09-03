//! Just enough sRGB arithmetic to turn an imported terminal palette into a
//! full set of chrome tokens.
//!
//! An imported colour scheme describes a terminal and nothing else: sixteen
//! ANSI colours, a background, a foreground, sometimes a cursor. Harbour's
//! themes also colour the tab bar, menus and dialogs, so the missing tokens
//! are derived here by mixing the two colours every scheme does define.

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Rgb {
    pub r: u8,
    pub g: u8,
    pub b: u8,
}

impl Rgb {
    pub const fn new(r: u8, g: u8, b: u8) -> Self {
        Self { r, g, b }
    }

    /// Accepts `#rgb`, `#rrggbb` and `#rrggbbaa`, with or without the hash.
    /// Alpha is dropped: the terminal composites against its own background,
    /// and a chrome token has nothing to composite against.
    pub fn parse(text: &str) -> Option<Self> {
        let hex = text.trim().trim_start_matches('#');
        if !hex.chars().all(|c| c.is_ascii_hexdigit()) {
            return None;
        }
        let pair = |i: usize| u8::from_str_radix(&hex[i..i + 2], 16).ok();
        match hex.len() {
            3 => {
                let digit = |i: usize| u8::from_str_radix(&hex[i..i + 1], 16).ok();
                Some(Self::new(digit(0)? * 17, digit(1)? * 17, digit(2)? * 17))
            }
            6 | 8 => Some(Self::new(pair(0)?, pair(2)?, pair(4)?)),
            _ => None,
        }
    }

    /// From the 0..1 components an iTerm plist stores.
    pub fn from_unit(r: f32, g: f32, b: f32) -> Self {
        let channel = |v: f32| (v.clamp(0.0, 1.0) * 255.0).round() as u8;
        Self::new(channel(r), channel(g), channel(b))
    }

    pub fn to_hex(self) -> String {
        format!("#{:02x}{:02x}{:02x}", self.r, self.g, self.b)
    }

    /// `t` of `other`, `1 - t` of `self`.
    pub fn mix(self, other: Rgb, t: f32) -> Self {
        let t = t.clamp(0.0, 1.0);
        let channel =
            |a: u8, b: u8| (f32::from(a) + (f32::from(b) - f32::from(a)) * t).round() as u8;
        Self::new(
            channel(self.r, other.r),
            channel(self.g, other.g),
            channel(self.b, other.b),
        )
    }

    /// WCAG relative luminance, used only to decide whether a scheme is dark.
    pub fn luminance(self) -> f32 {
        fn linear(channel: u8) -> f32 {
            let c = f32::from(channel) / 255.0;
            if c <= 0.040_45 {
                c / 12.92
            } else {
                ((c + 0.055) / 1.055).powf(2.4)
            }
        }
        0.2126 * linear(self.r) + 0.7152 * linear(self.g) + 0.0722 * linear(self.b)
    }

    /// The threshold sits well below mid grey: a "dark" scheme is one whose
    /// background is dark enough that light chrome would glare beside it.
    pub fn is_dark(self) -> bool {
        self.luminance() < 0.2
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_every_hex_form() {
        assert_eq!(Rgb::parse("#1e1e1e"), Some(Rgb::new(0x1e, 0x1e, 0x1e)));
        assert_eq!(Rgb::parse("1e1e1e"), Some(Rgb::new(0x1e, 0x1e, 0x1e)));
        assert_eq!(Rgb::parse("#f0a"), Some(Rgb::new(0xff, 0x00, 0xaa)));
        // Alpha is parsed and discarded rather than rejected.
        assert_eq!(Rgb::parse("#11223380"), Some(Rgb::new(0x11, 0x22, 0x33)));
    }

    #[test]
    fn rejects_anything_that_is_not_a_colour() {
        assert_eq!(Rgb::parse(""), None);
        assert_eq!(Rgb::parse("#12345"), None);
        assert_eq!(Rgb::parse("rebeccapurple"), None);
        assert_eq!(Rgb::parse("#gggggg"), None);
    }

    #[test]
    fn round_trips_through_hex() {
        assert_eq!(Rgb::parse("#5eead4").unwrap().to_hex(), "#5eead4");
    }

    #[test]
    fn mixes_towards_the_second_colour() {
        let black = Rgb::new(0, 0, 0);
        let white = Rgb::new(255, 255, 255);
        assert_eq!(black.mix(white, 0.0), black);
        assert_eq!(black.mix(white, 1.0), white);
        assert_eq!(black.mix(white, 0.5), Rgb::new(128, 128, 128));
    }

    #[test]
    fn unit_components_become_bytes() {
        assert_eq!(Rgb::from_unit(1.0, 0.0, 0.5), Rgb::new(255, 0, 128));
        // Out-of-gamut values from a plist are clamped, not wrapped.
        assert_eq!(Rgb::from_unit(1.4, -0.2, 0.0), Rgb::new(255, 0, 0));
    }

    #[test]
    fn classifies_scheme_backgrounds() {
        assert!(Rgb::parse("#1e1e1e").unwrap().is_dark());
        assert!(Rgb::parse("#002b36").unwrap().is_dark()); // Solarized Dark
        assert!(!Rgb::parse("#fdf6e3").unwrap().is_dark()); // Solarized Light
        assert!(!Rgb::parse("#ffffff").unwrap().is_dark());
    }
}
