use zeo_ui_views_base::{last_segment, url_to_path};

#[test]
pub(crate) fn file_urls_decode() {
    assert_eq!(
        url_to_path("file:///Users/me/My%20Music/"),
        "/Users/me/My Music/"
    );
    assert_eq!(url_to_path("file:///tmp/%E9%9F%B3%E4%B9%90"), "/tmp/音乐");
    assert_eq!(
        url_to_path("file:///C:/Users/me/My%20Music"),
        "C:\\Users\\me\\My Music"
    );
    assert_eq!(last_segment("C:\\Users\\me\\Music\\"), "Music");
    assert_eq!(last_segment("/Users/me/Music"), "Music");
}
