//! 网易云封面的取图地址。
//!
//! 接口给的 `picUrl` 是上传的原图：实测常见 3000×3000、4000×4000，个别超过 12000×12000，
//! PNG 原图能到 19 MB（超过封面下载上限 10 MB 会被整张丢弃，也超过 FLAC 图片块的 16 MB 上限）。
//! 图床的 `imageView` 参数在服务端处理：超过上限的等比缩小，PNG 转成 JPEG，
//! 上限以内的 JPEG 原样返回（字节不变），所以清晰度只在原图超过上限时才有损失。

/// 封面最长边的上限（像素）。
pub const MAX_COVER_EDGE: u32 = 4000;

/// 下载封面用的地址：网易云图床的地址加上「最长边不超过 [`MAX_COVER_EDGE`]、输出 JPEG」的参数
/// （原有的查询参数一并换掉）；其他主机的地址原样返回。
pub fn cover_download_url(url: &str) -> String {
    let url = url.trim();
    let base = url.split(['?', '#']).next().unwrap_or(url);
    if !is_netease_image_host(base) {
        return url.to_owned();
    }
    format!("{base}?imageView&thumbnail={MAX_COVER_EDGE}x{MAX_COVER_EDGE}&type=jpg")
}

/// 是否是网易云的图床（`p1.music.126.net` 等）。
fn is_netease_image_host(url: &str) -> bool {
    let Some((scheme, rest)) = url.split_once("://") else {
        return false;
    };
    let host = rest
        .split('/')
        .next()
        .unwrap_or_default()
        .to_ascii_lowercase();
    matches!(scheme.to_ascii_lowercase().as_str(), "http" | "https")
        && host.ends_with(".music.126.net")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn netease_covers_are_capped_and_converted_to_jpeg() {
        assert_eq!(
            cover_download_url(
                "http://p1.music.126.net/jtPZRUFrSS-nRCjW_LYowQ==/109951166521931227.jpg"
            ),
            "http://p1.music.126.net/jtPZRUFrSS-nRCjW_LYowQ==/109951166521931227.jpg?imageView&thumbnail=4000x4000&type=jpg"
        );
        // 原有的缩放参数换成上限参数。
        assert_eq!(
            cover_download_url(" https://P2.Music.126.net/a==/1.jpg?param=300y300 "),
            "https://P2.Music.126.net/a==/1.jpg?imageView&thumbnail=4000x4000&type=jpg"
        );
    }

    #[test]
    fn other_hosts_are_left_alone() {
        for url in [
            "https://y.gtimg.cn/music/photo_new/T002R1200x1200M000a.jpg",
            "https://img.example/cover.jpg",
            "https://music.126.net.example/cover.jpg",
            "https://evil.example/p1.music.126.net/cover.jpg",
            "p1.music.126.net/cover.jpg",
            "",
        ] {
            assert_eq!(cover_download_url(url), url);
        }
    }
}
