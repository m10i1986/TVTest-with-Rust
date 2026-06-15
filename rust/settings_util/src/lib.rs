// TVTest の Settings.cpp の型変換ヘルパーを Rust へ移植したもの。
//
// 移植対象:
//   - parse_bool / format_bool   : CSettings::Read(bool*):233 / Write(bool):249
//   - parse_color / format_color : CSettings::ReadColor:298 / WriteColor:312
//   - parse_font / format_font   : CSettings::Read(LOGFONT*):330 / Write(LOGFONT*):386
//   - parse_float / parse_double : CSettings::Read(float*):282 / Read(double*):257
//
// CSettings::Open/Close/Read/Write の IniFile I/O 部分は対象外(ファイル I/O 依存)。
// フォントは Win32 LOGFONT から独自の FontInfo 構造体に変換。

const FONT_FLAG_ITALIC:    u32 = 0x0001;
const FONT_FLAG_UNDERLINE: u32 = 0x0002;
const FONT_FLAG_STRIKEOUT: u32 = 0x0004;

/// bool 値を INI 文字列からパース。原実装 CSettings::Read(bool*):233。
/// "yes"/"true" → true、"no"/"false" → false。大小無視。
pub fn parse_bool(s: &str) -> Option<bool> {
    let lower = s.to_ascii_lowercase();
    match lower.as_str() {
        "yes" | "true"  => Some(true),
        "no"  | "false" => Some(false),
        _ => None,
    }
}

/// bool 値を INI 文字列に変換。原実装 CSettings::Write(bool):249。
/// true → "yes"、false → "no"。
pub fn format_bool(value: bool) -> &'static str {
    if value { "yes" } else { "no" }
}

/// `#RRGGBB` 形式のカラー文字列をパース。原実装 CSettings::ReadColor:298。
/// 戻り値は (r, g, b)。
pub fn parse_color(s: &str) -> Option<(u8, u8, u8)> {
    let s = s.trim();
    if s.len() < 7 || !s.starts_with('#') {
        return None;
    }
    let r = u8::from_str_radix(&s[1..3], 16).ok()?;
    let g = u8::from_str_radix(&s[3..5], 16).ok()?;
    let b = u8::from_str_radix(&s[5..7], 16).ok()?;
    Some((r, g, b))
}

/// カラー値を `#RRGGBB` 形式に変換。原実装 CSettings::WriteColor:312。
pub fn format_color(r: u8, g: u8, b: u8) -> String {
    format!("#{:02x}{:02x}{:02x}", r, g, b)
}

/// フォント情報。Win32 LOGFONT の移植対象フィールド。
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct FontInfo {
    pub face_name: String,
    pub height: i32,
    pub weight: i32,
    pub italic: bool,
    pub underline: bool,
    pub strikeout: bool,
}

/// `FaceName,Height,Weight,Flags` 形式のフォント文字列をパース。
/// 原実装 CSettings::Read(LOGFONT*):330。
/// face_name が空なら None を返す。
pub fn parse_font(s: &str) -> Option<FontInfo> {
    let mut parts = s.splitn(4, ',');
    let face_name = parts.next()?.trim().to_string();
    if face_name.is_empty() {
        return None;
    }
    let height = parts.next().map(|p| p.trim().parse::<i32>().unwrap_or(0)).unwrap_or(0);
    let weight = parts.next().map(|p| p.trim().parse::<i32>().unwrap_or(400)).unwrap_or(400);
    let flags  = parts.next().map(|p| p.trim().parse::<u32>().unwrap_or(0)).unwrap_or(0);
    Some(FontInfo {
        face_name,
        height,
        weight,
        italic:    (flags & FONT_FLAG_ITALIC)    != 0,
        underline: (flags & FONT_FLAG_UNDERLINE)  != 0,
        strikeout: (flags & FONT_FLAG_STRIKEOUT)  != 0,
    })
}

/// FontInfo を `FaceName,Height,Weight,Flags` 形式に変換。
/// 原実装 CSettings::Write(LOGFONT*):386。
pub fn format_font(font: &FontInfo) -> String {
    let mut flags: u32 = 0;
    if font.italic    { flags |= FONT_FLAG_ITALIC;    }
    if font.underline { flags |= FONT_FLAG_UNDERLINE; }
    if font.strikeout { flags |= FONT_FLAG_STRIKEOUT; }
    format!("{},{},{},{}", font.face_name, font.height, font.weight, flags)
}

/// float 値を INI 文字列からパース。原実装 CSettings::Read(float*):282。
pub fn parse_float(s: &str) -> Option<f32> {
    s.trim().parse::<f32>().ok()
}

/// double 値を INI 文字列からパース。原実装 CSettings::Read(double*):257。
pub fn parse_double(s: &str) -> Option<f64> {
    s.trim().parse::<f64>().ok()
}

/// double 値を INI 文字列に変換。原実装 CSettings::Write(double,int):273。
pub fn format_double(value: f64, digits: usize) -> String {
    format!("{:.prec$}", value, prec = digits)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_bool_true_variants() {
        assert_eq!(parse_bool("yes"), Some(true));
        assert_eq!(parse_bool("YES"), Some(true));
        assert_eq!(parse_bool("true"), Some(true));
        assert_eq!(parse_bool("True"), Some(true));
    }

    #[test]
    fn test_parse_bool_false_variants() {
        assert_eq!(parse_bool("no"), Some(false));
        assert_eq!(parse_bool("NO"), Some(false));
        assert_eq!(parse_bool("false"), Some(false));
        assert_eq!(parse_bool("False"), Some(false));
    }

    #[test]
    fn test_parse_bool_invalid() {
        assert!(parse_bool("1").is_none());
        assert!(parse_bool("").is_none());
        assert!(parse_bool("ok").is_none());
    }

    #[test]
    fn test_format_bool() {
        assert_eq!(format_bool(true), "yes");
        assert_eq!(format_bool(false), "no");
    }

    #[test]
    fn test_parse_color_basic() {
        let (r, g, b) = parse_color("#1a2b3c").unwrap();
        assert_eq!((r, g, b), (0x1a, 0x2b, 0x3c));
    }

    #[test]
    fn test_parse_color_white() {
        assert_eq!(parse_color("#ffffff"), Some((255, 255, 255)));
    }

    #[test]
    fn test_parse_color_invalid_prefix() {
        assert!(parse_color("1a2b3c").is_none());
    }

    #[test]
    fn test_parse_color_too_short() {
        assert!(parse_color("#1a2b").is_none());
    }

    #[test]
    fn test_format_color() {
        assert_eq!(format_color(0x1a, 0x2b, 0x3c), "#1a2b3c");
        assert_eq!(format_color(0, 0, 0), "#000000");
        assert_eq!(format_color(255, 255, 255), "#ffffff");
    }

    #[test]
    fn test_color_roundtrip() {
        let s = format_color(12, 34, 56);
        let (r, g, b) = parse_color(&s).unwrap();
        assert_eq!((r, g, b), (12, 34, 56));
    }

    #[test]
    fn test_parse_font_basic() {
        let f = parse_font("MS Gothic,-16,400,0").unwrap();
        assert_eq!(f.face_name, "MS Gothic");
        assert_eq!(f.height, -16);
        assert_eq!(f.weight, 400);
        assert!(!f.italic);
        assert!(!f.underline);
        assert!(!f.strikeout);
    }

    #[test]
    fn test_parse_font_flags() {
        // italic(1) + underline(2) = 3
        let f = parse_font("Arial,12,700,3").unwrap();
        assert_eq!(f.face_name, "Arial");
        assert_eq!(f.weight, 700);
        assert!(f.italic);
        assert!(f.underline);
        assert!(!f.strikeout);
    }

    #[test]
    fn test_parse_font_strikeout() {
        let f = parse_font("Courier,10,400,4").unwrap();
        assert!(f.strikeout);
    }

    #[test]
    fn test_parse_font_empty_face_fails() {
        assert!(parse_font(",12,400,0").is_none());
    }

    #[test]
    fn test_format_font() {
        let f = FontInfo {
            face_name: "MS Gothic".to_string(),
            height: -16,
            weight: 400,
            italic: false,
            underline: false,
            strikeout: false,
        };
        assert_eq!(format_font(&f), "MS Gothic,-16,400,0");
    }

    #[test]
    fn test_format_font_with_flags() {
        let f = FontInfo {
            face_name: "Arial".to_string(),
            height: 12,
            weight: 700,
            italic: true,
            underline: true,
            strikeout: false,
        };
        // italic(1) + underline(2) = 3
        assert_eq!(format_font(&f), "Arial,12,700,3");
    }

    #[test]
    fn test_font_roundtrip() {
        let original = FontInfo {
            face_name: "游ゴシック".to_string(),
            height: -18,
            weight: 600,
            italic: false,
            underline: false,
            strikeout: true,
        };
        let s = format_font(&original);
        let parsed = parse_font(&s).unwrap();
        assert_eq!(parsed, original);
    }

    #[test]
    fn test_parse_float() {
        assert!((parse_float("3.14").unwrap() - 3.14f32).abs() < 1e-5);
        assert!(parse_float("abc").is_none());
    }

    #[test]
    fn test_parse_double() {
        assert!((parse_double("2.718281828").unwrap() - 2.718281828f64).abs() < 1e-9);
    }

    #[test]
    fn test_format_double() {
        assert_eq!(format_double(3.14159, 2), "3.14");
        assert_eq!(format_double(1.0, 3), "1.000");
    }
}
