use super::*;

#[test]
fn time_formats() {
    assert_eq!(format_time(0.0), "0:00");
    assert_eq!(format_time(65.9), "1:05");
    assert_eq!(format_time(f64::NAN), "0:00");
    assert_eq!(format_total(42.0 * 60.0), "42 分钟");
    assert_eq!(format_total(65.0 * 60.0), "1 小时 5 分钟");
}
