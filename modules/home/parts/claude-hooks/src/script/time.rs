//! 区間の端（`--since`、`--until`）を受けるスクリプトの日時: 時差つきの ISO 8601 を読み、JST で書く

use regex::Regex;
use std::sync::LazyLock;

/// JST の時差（秒）
pub(super) const JST: i64 = 9 * 3600;
pub(super) const DAY_MICROS: i64 = 86_400_000_000;

pub(super) static ISO_WITH_OFFSET: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"^\d{4}-\d{2}-\d{2}T\d{2}:\d{2}(:\d{2}(\.\d+)?)?(Z|[+-]\d{2}:\d{2})$").unwrap()
});

/// 時差つきの日時。比べるのは UNIX の時刻（マイクロ秒）
#[derive(Debug, Clone, Copy, PartialEq, PartialOrd)]
pub(super) struct Time {
    pub(super) micros: i64,
}

/// 1970-01-01 からの日数（Howard Hinnant の days_from_civil。グレゴリオ暦）
fn days_from_civil(y: i64, m: i64, d: i64) -> i64 {
    let y = if m <= 2 { y - 1 } else { y };
    let era = y.div_euclid(400);
    let yoe = y - era * 400;
    let mp = (m + 9) % 12;
    let doy = (153 * mp + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe - 719_468
}

/// days_from_civil の逆（年、月、日）
fn civil_from_days(z: i64) -> (i64, i64, i64) {
    let z = z + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    (
        if m <= 2 {
            yoe + era * 400 + 1
        } else {
            yoe + era * 400
        },
        m,
        d,
    )
}

fn days_in_month(y: i64, m: i64) -> i64 {
    match m {
        2 if (y % 4 == 0 && y % 100 != 0) || y % 400 == 0 => 29,
        2 => 28,
        4 | 6 | 9 | 11 => 30,
        _ => 31,
    }
}

/// `YYYY-MM-DDTHH:MM[:SS[.端数]](Z|±HH:MM)` を読む（秒の端数は 6 桁より後を切り捨てる）。読めなければ None
pub(super) fn parse_time(s: &str) -> Option<Time> {
    let b = s.as_bytes();
    let num = |from: usize, len: usize| -> Option<i64> {
        let part = b.get(from..from + len)?;
        part.iter().all(u8::is_ascii_digit).then(|| {
            part.iter()
                .fold(0i64, |acc, d| acc * 10 + i64::from(d - b'0'))
        })
    };
    let (y, mo, d) = (num(0, 4)?, num(5, 2)?, num(8, 2)?);
    if b.get(4) != Some(&b'-') || b.get(7) != Some(&b'-') || b.get(10) != Some(&b'T') {
        return None;
    }
    let (h, mi) = (num(11, 2)?, num(14, 2)?);
    if b.get(13) != Some(&b':') {
        return None;
    }
    let mut i = 16;
    let mut sec = 0;
    let mut micros = 0;
    if b.get(i) == Some(&b':') {
        sec = num(i + 1, 2)?;
        i += 3;
        if b.get(i) == Some(&b'.') {
            let start = i + 1;
            let mut j = start;
            while b.get(j).is_some_and(u8::is_ascii_digit) {
                j += 1;
            }
            if j == start {
                return None;
            }
            let digits = &s[start..j];
            let six: String = digits
                .chars()
                .chain(std::iter::repeat('0'))
                .take(6)
                .collect();
            micros = six.parse::<i64>().ok()?;
            i = j;
        }
    }
    let offset = match &s[i..] {
        "Z" => 0,
        rest if rest.len() == 6 && (rest.starts_with('+') || rest.starts_with('-')) => {
            if b.get(i + 3) != Some(&b':') {
                return None;
            }
            let (oh, om) = (num(i + 1, 2)?, num(i + 4, 2)?);
            if oh > 23 || om > 59 {
                return None;
            }
            let sign = if rest.starts_with('-') { -1 } else { 1 };
            sign * (oh * 3600 + om * 60)
        }
        _ => return None,
    };
    if !(1..=12).contains(&mo) || d < 1 || d > days_in_month(y, mo) || h > 23 || mi > 59 || sec > 59
    {
        return None;
    }
    let seconds = days_from_civil(y, mo, d) * 86_400 + h * 3600 + mi * 60 + sec - offset;
    Some(Time {
        micros: seconds * 1_000_000 + micros,
    })
}

/// JST の暦の (年, 月, 日, 時, 分, 秒, マイクロ秒)
pub(super) fn jst_fields(t: Time) -> (i64, i64, i64, i64, i64, i64, i64) {
    let local = t.micros + JST * 1_000_000;
    let days = local.div_euclid(DAY_MICROS);
    let rest = local.rem_euclid(DAY_MICROS);
    let (y, m, d) = civil_from_days(days);
    let secs = rest / 1_000_000;
    (
        y,
        m,
        d,
        secs / 3600,
        secs / 60 % 60,
        secs % 60,
        rest % 1_000_000,
    )
}

/// JST の `YYYY-MM-DDTHH:MM:SS[.ffffff]+09:00`（マイクロ秒は 0 でなければ付ける）
pub(super) fn jst_iso(t: Time) -> String {
    let (y, m, d, h, mi, s, us) = jst_fields(t);
    let frac = if us == 0 {
        String::new()
    } else {
        format!(".{us:06}")
    };
    format!("{y:04}-{m:02}-{d:02}T{h:02}:{mi:02}:{s:02}{frac}+09:00")
}

/// UTC の `YYYY-MM-DDTHH:MM:SS.mmmZ`（Claude Code の記録の timestamp と同じ形。ミリ秒より後は切り捨てる）
pub(super) fn utc_millis(t: Time) -> String {
    let days = t.micros.div_euclid(DAY_MICROS);
    let rest = t.micros.rem_euclid(DAY_MICROS);
    let (y, m, d) = civil_from_days(days);
    let secs = rest / 1_000_000;
    let millis = rest % 1_000_000 / 1000;
    format!(
        "{y:04}-{m:02}-{d:02}T{:02}:{:02}:{:02}.{millis:03}Z",
        secs / 3600,
        secs / 60 % 60,
        secs % 60
    )
}

pub(super) fn now_micros() -> i64 {
    let since_epoch = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default();
    since_epoch.as_micros() as i64
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn times_with_an_offset_are_read_as_unix_time() {
        let t = |s: &str| parse_time(s).map(|t| t.micros);
        assert_eq!(t("1970-01-01T00:00Z"), Some(0));
        assert_eq!(t("1970-01-01T09:00:00+09:00"), Some(0));
        assert_eq!(t("2026-11-06T15:00:00.000Z"), t("2026-11-07T00:00+09:00"));
        // 6 桁より後の端数は切り捨てる
        assert_eq!(t("1970-01-01T00:00:00.1234567Z"), Some(123_456));
        assert!(t("2024-02-29T00:00Z").is_some());
        for bad in [
            "2026-11-07T00:00",
            "2026-02-29T00:00Z",
            "2026-11-07T24:00Z",
            "2026-11-07T00:00:00.Z",
            "2026-11-07 00:00Z",
            "2026-11-07T00:00+0900",
        ] {
            assert_eq!(t(bad), None, "{bad}");
        }
    }

    #[test]
    fn jst_iso_shows_microseconds_only_when_present() {
        let t = |s: &str| jst_iso(parse_time(s).unwrap());
        assert_eq!(t("2026-11-06T15:00Z"), "2026-11-07T00:00:00+09:00");
        assert_eq!(
            t("2026-12-31T15:00:00.5Z"),
            "2027-01-01T00:00:00.500000+09:00"
        );
    }

    #[test]
    fn utc_millis_cuts_the_fraction_to_milliseconds() {
        let t = |s: &str| utc_millis(parse_time(s).unwrap());
        assert_eq!(t("2026-11-06T00:00+09:00"), "2026-11-05T15:00:00.000Z");
        assert_eq!(
            t("2026-11-05T23:59:59.99999-01:00"),
            "2026-11-06T00:59:59.999Z"
        );
        assert_eq!(t("1969-12-31T23:59:59.9995Z"), "1969-12-31T23:59:59.999Z");
    }
}
