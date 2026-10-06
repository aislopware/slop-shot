//! Sensitive data scanner (SensitiveScanner.swift): reads the image's text, then finds
//! emails, phone numbers, card numbers, tokens… and returns the box of just that text
//! for the editor to black out. Runs on-device like the macOS one.

use std::ops::Range;
use std::sync::OnceLock;

use fancy_regex::Regex;
use image::RgbaImage;

use crate::annotate::Rect;
use crate::ocr::{self, Line};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Kind {
    Email,
    Phone,
    Card,
    Cvv,
    Expiry,
    Token,
    Secret,
    IdNumber,
    Name,
}

impl Kind {
    fn label(self) -> &'static str {
        match self {
            Kind::Email => "email",
            Kind::Phone => "phone number",
            Kind::Card => "card number",
            Kind::Cvv => "CVV",
            Kind::Expiry => "expiry date",
            Kind::Token => "key/token",
            Kind::Secret => "password",
            Kind::IdNumber => "ID number",
            Kind::Name => "name on the card",
        }
    }

    fn plural(self) -> String {
        match self {
            Kind::Name => "names on the card".into(),
            _ => format!("{}s", self.label()),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Match {
    pub kind: Kind,
    /// Image pixels.
    pub rect: Rect,
}

/// Blocking; run it off the main thread.
pub fn scan(image: &RgbaImage) -> anyhow::Result<Vec<Match>> {
    let lines = ocr::lines(image)?;
    let mut found: Vec<Match> = lines
        .iter()
        .flat_map(|line| {
            let text = line.text();
            matches(&text).into_iter().filter_map(|(kind, range)| at(kind, range, line)).collect::<Vec<_>>()
        })
        .collect();
    // A person's name only counts next to card data, and only on the card: chats,
    // commit logs and title-case labels are full of things that look like names.
    if let Some(zone) = card_zone(&found, image.width() as f32, image.height() as f32) {
        for line in &lines {
            let text = line.text();
            for range in cardholder_names(&text) {
                if let Some(m) = at(Kind::Name, range, line)
                    && zone.contains(m.rect.center())
                {
                    found.push(m);
                }
            }
        }
    }
    Ok(merged(found))
}

/// "2 emails, 1 card number" for the status line.
pub fn summary(items: &[Match]) -> String {
    const ORDER: [Kind; 9] =
        [Kind::Email, Kind::Phone, Kind::Card, Kind::Cvv, Kind::Expiry, Kind::Name, Kind::Token, Kind::Secret, Kind::IdNumber];
    ORDER
        .iter()
        .filter_map(|&kind| {
            let n = items.iter().filter(|m| m.kind == kind).count();
            (n > 0).then(|| format!("{n} {}", if n == 1 { kind.label().to_owned() } else { kind.plural() }))
        })
        .collect::<Vec<_>>()
        .join(", ")
}

fn at(kind: Kind, range: Range<usize>, line: &Line) -> Option<Match> {
    let [x, y, w, h] = line.rect_of(range.start, range.end)?;
    (w > 0. && h > 0.).then_some(Match { kind, rect: Rect { x, y, w, h } })
}

/// Around the card: the card number, CVV and expiry, grown to reach the name line at
/// the bottom of the card. None without card data, and then names aren't looked for.
fn card_zone(found: &[Match], image_w: f32, image_h: f32) -> Option<Rect> {
    let zone = found
        .iter()
        .filter(|m| matches!(m.kind, Kind::Card | Kind::Cvv | Kind::Expiry))
        .map(|m| m.rect)
        .reduce(union)?;
    let dx = (zone.w * 0.4).max(0.06 * image_w);
    let dy = (zone.h * 1.4).max(0.08 * image_h);
    Some(Rect { x: zone.x - dx, y: zone.y - dy, w: zone.w + 2. * dx, h: zone.h + 2. * dy })
}

fn matches(s: &str) -> Vec<(Kind, Range<usize>)> {
    let mut out = Vec::new();
    for (kind, regex, group) in regexes() {
        for caps in regex.captures_iter(s).flatten() {
            // A group > 0 covers only the value, leaving its label readable.
            let Some(m) = caps.get(*group) else { continue };
            // Every real card number passes Luhn; other long digit runs mostly don't.
            if *kind == Kind::Card && !luhn_valid(m.as_str()) {
                continue;
            }
            out.push((*kind, m.range()));
        }
    }
    for m in phone().find_iter(s).flatten() {
        // Shorter runs are times, page numbers and the like.
        if m.as_str().chars().filter(char::is_ascii_digit).count() >= 8 {
            out.push((Kind::Phone, m.range()));
        }
    }
    out
}

/// Patterns and the capture group to cover (0 = the whole match).
fn regexes() -> &'static [(Kind, Regex, usize)] {
    static REGEXES: OnceLock<Vec<(Kind, Regex, usize)>> = OnceLock::new();
    REGEXES.get_or_init(|| {
        let patterns: [(Kind, &str, usize); 9] = [
            (Kind::Email, r"[A-Za-z0-9._%+\-]+@[A-Za-z0-9.\-]+\.[A-Za-z]{2,}", 0),
            // A JWT's header and payload both start with "eyJ", base64 for '{"'.
            (Kind::Token, r"eyJ[A-Za-z0-9_\-]{5,}\.eyJ[A-Za-z0-9_\-]{5,}(?:\.[A-Za-z0-9_\-]+)?", 0),
            (
                Kind::Token,
                concat!(
                    r"(?:sk-[A-Za-z0-9_\-]{16,}",
                    r"|ghp_[A-Za-z0-9]{20,}|gho_[A-Za-z0-9]{20,}|github_pat_[A-Za-z0-9_]{20,}",
                    r"|AKIA[0-9A-Z]{12,}",
                    r"|xox[abps]-[A-Za-z0-9\-]{10,}",
                    r"|AIza[0-9A-Za-z_\-]{30,})"
                ),
                0,
            ),
            (Kind::Token, r"-----BEGIN [A-Z ]*PRIVATE KEY-----", 0),
            // 13–19 digits, spaces or dashes allowed between them; Luhn filters further.
            (Kind::Card, r"(?<![0-9\-])(?:[0-9][ \-]?){12,18}[0-9](?![0-9\-])", 0),
            // From here on a label must come first: bare three digits are everywhere.
            (
                Kind::Cvv,
                concat!(
                    r"(?:CVV|CVC|CVV2|CVC2|CSC|Card\s*Verification(?:\s*(?:Value|Code))?",
                    r"|Security\s*Code|Mã\s*bảo\s*mật)\s*[:#.]?\s*([0-9]{3,4})(?![0-9])"
                ),
                1,
            ),
            (
                Kind::Expiry,
                concat!(
                    r"(?:Valid\s*Thru|Valid\s*Through|Valid\s*Until|Expiration(?:\s*Date)?",
                    r"|Expires?|Exp(?:\.|\b)|Hết\s*hạn|Hiệu\s*lực(?:\s*đến)?)",
                    r"\s*[:.]?\s*([0-9]{1,2}\s*[/\-]\s*(?:[0-9]{4}|[0-9]{2}))(?![0-9])"
                ),
                1,
            ),
            (
                Kind::Secret,
                concat!(
                    r"(?:password|passwd|pass\s*phrase|pwd|secret|api[ _\-]?key",
                    r"|access[ _\-]?token|client[ _\-]?secret|private[ _\-]?key",
                    r"|mật\s*khẩu|mã\s*OTP|OTP)\s*[:=]\s*(\S{4,})"
                ),
                1,
            ),
            (
                Kind::IdNumber,
                concat!(
                    r"(?:CCCD|CMND|CMTND|Căn\s*cước(?:\s*công\s*dân)?|Số\s*CCCD",
                    r"|ID\s*(?:No\.?|Number)|SSN|Social\s*Security(?:\s*Number)?",
                    r"|Passport(?:\s*(?:No\.?|Number))?|Hộ\s*chiếu)",
                    r"\s*[:#.]?\s*([A-Z0-9][A-Z0-9\- ]{6,17}[A-Z0-9])"
                ),
                1,
            ),
        ];
        patterns
            .into_iter()
            .map(|(kind, p, group)| (kind, Regex::new(&format!("(?i){p}")).expect("sensitive pattern"), group))
            .collect()
    })
}

/// Stands in for NSDataDetector: international numbers, numbers with a leading trunk
/// 0, and 3-3-4 groups. Dates and times don't fit these shapes.
fn phone() -> &'static Regex {
    static PHONE: OnceLock<Regex> = OnceLock::new();
    PHONE.get_or_init(|| {
        Regex::new(concat!(
            r"(?<![\w+])(?:",
            r"\+[0-9]{1,3}[\s.\-]?(?:\([0-9]{1,4}\)|[0-9]{1,4})(?:[\s.\-]?[0-9]{2,4}){2,4}",
            r"|\([0-9]{2,4}\)\s?[0-9]{3,4}[\s.\-]?[0-9]{3,4}",
            r"|0[0-9]{2,3}[\s.\-]?[0-9]{3,4}[\s.\-]?[0-9]{3,4}",
            r"|[0-9]{3}[\s.\-][0-9]{3}[\s.\-][0-9]{4}",
            r")(?!\w)"
        ))
        .expect("phone pattern")
    })
}

/// Only asked for next to card data (see `scan`). Cards print names in capitals,
/// "JOHN SMITH": 2–4 capitalised words of letters that aren't the card's own print.
fn cardholder_names(s: &str) -> Vec<Range<usize>> {
    let trimmed = s.trim();
    let len = trimmed.chars().count();
    if !(4..=40).contains(&len) || trimmed.chars().any(char::is_numeric) || is_card_chrome(trimmed) {
        return vec![];
    }
    let words: Vec<&str> = trimmed.split(' ').collect();
    let name_like = (2..=4).contains(&words.len())
        && words.iter().all(|w| w.chars().count() >= 2 && w.chars().all(char::is_alphabetic) && w.chars().next().is_some_and(char::is_uppercase));
    if !name_like {
        return vec![];
    }
    let start = s.find(trimmed).unwrap_or(0);
    vec![start..start + trimmed.len()]
}

fn is_card_chrome(s: &str) -> bool {
    const CHROME: [&str; 35] = [
        "visa", "mastercard", "master card", "american express", "amex", "discover", "jcb", "unionpay", "union pay",
        "diners club", "maestro", "napas", "credit card", "debit card", "card type", "card number", "cardholder",
        "cardholder name", "card holder", "name on card", "valid thru", "valid through", "good thru", "expires",
        "expiry date", "member since", "number of cards", "generate credit card", "generate", "security code",
        "authorized signature", "chủ thẻ", "thẻ tín dụng", "thẻ ghi nợ", "ngân hàng",
    ];
    let key = s.to_lowercase();
    let key = key.trim_matches(|c: char| ":·-—".contains(c)).trim();
    CHROME.contains(&key)
}

fn luhn_valid(raw: &str) -> bool {
    let digits: Vec<u32> = raw.chars().filter_map(|c| c.to_digit(10)).collect();
    if !(13..=19).contains(&digits.len()) {
        return false;
    }
    let sum: u32 = digits
        .iter()
        .rev()
        .enumerate()
        .map(|(i, &d)| if i % 2 == 1 { if d * 2 > 9 { d * 2 - 9 } else { d * 2 } } else { d })
        .sum();
    sum % 10 == 0
}

/// Overlapping finds (a token inside a longer string) become one box.
fn merged(items: Vec<Match>) -> Vec<Match> {
    let mut out: Vec<Match> = Vec::new();
    for m in items {
        match out.iter_mut().find(|o| intersects(o.rect, m.rect)) {
            Some(o) => o.rect = union(o.rect, m.rect),
            None => out.push(m),
        }
    }
    out
}

fn union(a: Rect, b: Rect) -> Rect {
    let (x, y) = (a.x.min(b.x), a.y.min(b.y));
    Rect { x, y, w: (a.x + a.w).max(b.x + b.w) - x, h: (a.y + a.h).max(b.y + b.h) - y }
}

fn intersects(a: Rect, b: Rect) -> bool {
    a.x < b.x + b.w && b.x < a.x + a.w && a.y < b.y + b.h && b.y < a.y + a.h
}

#[cfg(test)]
mod tests {
    use super::*;

    fn kinds(s: &str) -> Vec<(Kind, &str)> {
        matches(s).into_iter().map(|(k, r)| (k, &s[r])).collect()
    }

    #[test]
    fn finds_each_kind_and_covers_only_the_value() {
        assert_eq!(kinds("mail me at jane.doe+x@example.co.uk now"), [(Kind::Email, "jane.doe+x@example.co.uk")]);
        assert_eq!(kinds("Card 4111 1111 1111 1111"), [(Kind::Card, "4111 1111 1111 1111")]);
        assert_eq!(kinds("CVV: 231"), [(Kind::Cvv, "231")]);
        assert_eq!(kinds("Valid Thru 08/27"), [(Kind::Expiry, "08/27")]);
        assert_eq!(kinds("password = hunter22"), [(Kind::Secret, "hunter22")]);
        assert_eq!(kinds("CCCD: 001203004567")[0], (Kind::IdNumber, "001203004567"));
        assert_eq!(kinds("key ghp_abcdefghijklmnopqrstuvwx1234"), [(Kind::Token, "ghp_abcdefghijklmnopqrstuvwx1234")]);
        assert_eq!(kinds("Call +84 912 345 678 today"), [(Kind::Phone, "+84 912 345 678")]);
        assert_eq!(kinds("Hotline 0912.345.678"), [(Kind::Phone, "0912.345.678")]);
        assert_eq!(kinds("(555) 123-4567"), [(Kind::Phone, "(555) 123-4567")]);
    }

    #[test]
    fn leaves_lookalikes_alone() {
        // Fails Luhn, a bare 3-digit number, a date and a time, an order number.
        assert!(kinds("4111 1111 1111 1112").is_empty());
        assert!(kinds("Room 231").is_empty());
        assert!(kinds("2026-10-06 at 17.13.27").is_empty(), "{:?}", kinds("2026-10-06 at 17.13.27"));
        assert!(kinds("Order #12345678").is_empty(), "{:?}", kinds("Order #12345678"));
        assert!(kinds("1000×5776px").is_empty());
    }

    #[test]
    fn names_need_card_shape() {
        assert_eq!(cardholder_names("  JOHN SMITH "), [2..12]);
        // Like the Mac, every word needs two letters: a middle initial rules a line out.
        assert!(cardholder_names("JOHN A SMITH").is_empty());
        assert_eq!(cardholder_names("Eulah Harris"), [0..12]);
        assert!(cardholder_names("VALID THRU").is_empty());
        assert!(cardholder_names("Line Count Tool 2").is_empty());
        assert!(cardholder_names("hello world").is_empty());
    }

    /// Through tesseract, on text the editor itself draws.
    #[test]
    fn scan_reads_the_image_and_boxes_only_the_values() {
        use crate::annotate::{Annotation, Rgba, Tool};
        // Laid out like a card: the name sits right under the number and expiry.
        let lines = ["Contact: jane.doe@example.com", "Card 4111 1111 1111 1111   CVV: 231", "Valid Thru 08/27", "JOHN SMITH", "Saved 2026-10-06 at 17.13.27"];
        let annotations: Vec<Annotation> = lines
            .iter()
            .enumerate()
            .map(|(i, line)| {
                let mut a = Annotation::new(Tool::Text, Rgba::BLACK, 0.005, vec![(40., 30. + i as f32 * 45.)]);
                a.text = (*line).into();
                a
            })
            .collect();
        let white = RgbaImage::from_pixel(1100, 290, image::Rgba([255; 4]));
        let image = crate::raster::flatten(&white, &annotations, 1., &mut Default::default());
        let found = scan(&image).unwrap();
        assert_eq!(summary(&found), "1 email, 1 card number, 1 CVV, 1 expiry date, 1 name on the card", "{found:?}");
        let email = found.iter().find(|m| m.kind == Kind::Email).unwrap().rect;
        assert!(email.x > 130. && email.y < 60., "the label stays readable: {email:?}");
    }

    #[test]
    fn summary_reads_like_the_mac() {
        let r = Rect { x: 0., y: 0., w: 1., h: 1. };
        let items = [Match { kind: Kind::Card, rect: r }, Match { kind: Kind::Email, rect: r }, Match { kind: Kind::Email, rect: r }];
        assert_eq!(summary(&items), "2 emails, 1 card number");
    }
}
