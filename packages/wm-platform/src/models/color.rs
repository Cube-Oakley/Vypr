use std::str::FromStr;

use serde::{Deserialize, Deserializer, Serialize};

#[derive(Debug, Clone, Serialize)]
pub struct Color {
  pub r: u8,
  pub g: u8,
  pub b: u8,
  pub a: u8,
}

impl Color {
  #[must_use]
  #[allow(clippy::missing_panics_doc)]
  pub fn to_bgr(&self) -> u32 {
    let bgr = format!("{:02x}{:02x}{:02x}", self.b, self.g, self.r);
    // SAFETY: An invalid hex value is unrepresentable.
    u32::from_str_radix(&bgr, 16).unwrap()
  }

  /// Returns premultiplied ARGB as a `u32` for use with
  /// `UpdateLayeredWindow` pixel buffers.
  #[must_use]
  pub fn to_premultiplied_argb(&self) -> u32 {
    let a = u32::from(self.a);
    let r = (u32::from(self.r) * a) / 255;
    let g = (u32::from(self.g) * a) / 255;
    let b = (u32::from(self.b) * a) / 255;
    (a << 24) | (r << 16) | (g << 8) | b
  }

  /// Linearly interpolates between `self` and `other` at parameter `t`
  /// (clamped to 0.0..=1.0).
  #[must_use]
  pub fn lerp(&self, other: &Color, t: f64) -> Color {
    let t = t.clamp(0.0, 1.0);
    let mix = |a: u8, b: u8| -> u8 {
      (f64::from(a) + (f64::from(b) - f64::from(a)) * t) as u8
    };
    Color {
      r: mix(self.r, other.r),
      g: mix(self.g, other.g),
      b: mix(self.b, other.b),
      a: mix(self.a, other.a),
    }
  }
}

impl FromStr for Color {
  type Err = crate::ParseError;

  fn from_str(unparsed: &str) -> Result<Self, crate::ParseError> {
    let mut chars = unparsed.chars();

    if chars.next() != Some('#') {
      return Err(crate::ParseError::Color(unparsed.to_string()));
    }

    let parse_hex = |slice: &str| -> Result<u8, crate::ParseError> {
      u8::from_str_radix(slice, 16)
        .map_err(|_| crate::ParseError::Color(unparsed.to_string()))
    };

    let r = parse_hex(&unparsed[1..3])?;
    let g = parse_hex(&unparsed[3..5])?;
    let b = parse_hex(&unparsed[5..7])?;

    let a = match unparsed.len() {
      9 => parse_hex(&unparsed[7..9])?,
      7 => 255,
      _ => return Err(crate::ParseError::Color(unparsed.to_string())),
    };

    Ok(Self { r, g, b, a })
  }
}

/// Deserialize a `Color` from either a string or a struct.
impl<'de> Deserialize<'de> for Color {
  fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
  where
    D: Deserializer<'de>,
  {
    #[derive(Deserialize)]
    #[serde(untagged)]
    enum ColorDe {
      Struct { r: u8, g: u8, b: u8, a: u8 },
      String(String),
    }

    match ColorDe::deserialize(deserializer)? {
      ColorDe::Struct { r, g, b, a } => Ok(Self { r, g, b, a }),
      ColorDe::String(str) => {
        Self::from_str(&str).map_err(serde::de::Error::custom)
      }
    }
  }
}
