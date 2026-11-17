//! 掃除の issue の本文に書く日付が、1970-01-01 からの秒の暦の日になること（期待値は GNU date -u -d の結果）

use history_rewrite::issue::date;

#[test]
fn seconds_become_the_calendar_date() {
    assert_eq!(date(0), "1970-01-01");
    assert_eq!(date(951_782_400), "2000-02-29");
    // 2026-11-17 23:59:59 と、その 1 秒後
    assert_eq!(date(1_794_959_999), "2026-11-17");
    assert_eq!(date(1_794_960_000), "2026-11-18");
    // 1970 年より前
    assert_eq!(date(-1), "1969-12-31");
}
