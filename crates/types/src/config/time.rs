/// 现在（Unix 秒）。
pub fn unix_now() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_secs())
}

/// 本地时区相对 UTC 的偏移（秒）。取现值——夏令时切换边界的历史时刻
/// 用现值近似即可（首页统计的粒度是天）。
pub fn local_offset_seconds() -> i64 {
    #[cfg(target_os = "macos")]
    {
        // NSTimeZone.localTimeZone.secondsFromGMT 是现成的偏移秒数。
        objc2_foundation::NSTimeZone::localTimeZone().secondsFromGMT() as i64
    }
    #[cfg(target_os = "windows")]
    {
        // GetTimeZoneInformation 的 Bias 是「本地 + N 分钟 = UTC」——
        // 偏移取负；夏令时时段叠 DaylightBias。返回值是裸 u32
        // （windows-sys 0.61 只导出 INVALID 常量）：1=STANDARD 2=DAYLIGHT。
        use windows_sys::Win32::System::Time::{GetTimeZoneInformation, TIME_ZONE_INFORMATION};
        unsafe {
            let mut info: TIME_ZONE_INFORMATION = std::mem::zeroed();
            let id = GetTimeZoneInformation(&mut info);
            let extra = match id {
                2 => info.DaylightBias,
                1 => info.StandardBias,
                _ => 0,
            };
            -((info.Bias + extra) as i64) * 60
        }
    }
    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    {
        0
    }
}

/// `unix` 秒的本地天数（本地历法从 `unix + offset` 切）。
pub fn local_day(unix: u64, offset: i64) -> i64 {
    (unix as i64 + offset).div_euclid(86_400)
}

/// `unix` 秒的本地小时（0–23）。
pub fn local_hour(unix: u64, offset: i64) -> u32 {
    ((unix as i64 + offset).rem_euclid(86_400) / 3600) as u32
}

/// `unix` 所在本地周（周一 00:00）的起始 Unix 秒。
pub fn local_week_start(unix: u64, offset: i64) -> u64 {
    let day = local_day(unix, offset);
    // 纪元日 1970-01-01 是周四：day+3 对 7 取模 = 周一为 0 的星期序。
    let monday = day - (day + 3).rem_euclid(7);
    (monday * 86_400 - offset).max(0) as u64
}

/// `unix` 秒 → 本地 "YYYY-MM-DD HH:MM"（元数据面板的修改 / 入库时间用）。
pub fn format_local_datetime(unix: u64) -> String {
    format_local_at(unix, local_offset_seconds())
}

/// `unix` 秒 + 偏移 → "YYYY-MM-DD HH:MM"（测试可注入偏移）。
pub fn format_local_at(unix: u64, offset: i64) -> String {
    let t = unix as i64 + offset;
    let (y, m, d) = civil_from_days(t.div_euclid(86_400));
    let secs = t.rem_euclid(86_400);
    format!(
        "{:04}-{:02}-{:02} {:02}:{:02}",
        y,
        m,
        d,
        secs / 3600,
        secs % 3600 / 60
    )
}

/// 纪元日起算的天数 → (年, 月, 日)（Hinnant 的 civil_from_days，公历）。
fn civil_from_days(z: i64) -> (i64, u32, u32) {
    let z = z + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097); // [0, 146096]
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365; // [0, 399]
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100); // [0, 365]
    let mp = (5 * doy + 2) / 153; // [0, 11]
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32; // [1, 31]
    let m = (if mp < 10 { mp + 3 } else { mp - 9 }) as u32; // [1, 12]
    (if m <= 2 { y + 1 } else { y }, m, d)
}
