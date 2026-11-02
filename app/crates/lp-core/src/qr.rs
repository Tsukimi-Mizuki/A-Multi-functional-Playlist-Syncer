//! 登录二维码渲染：把内容编码成 SVG 字符串交给前端显示，前端不需要任何二维码库。
//!
//! C# 用 QRCoder（纠错级别 Q）生成 PNG；这里用 `qrcode` 生成同级别的 SVG。
//! 始终是白底黑码（含静区），深色主题下也能被手机扫描。

use qrcode::render::svg;
use qrcode::{EcLevel, QrCode};

/// 把 `content` 渲染成白底黑码的 SVG 文本。
pub fn render_svg(content: &str) -> Result<String, String> {
    if content.trim().is_empty() {
        return Err("二维码内容为空".to_owned());
    }
    let code = QrCode::with_error_correction_level(content, EcLevel::Q)
        .map_err(|error| format!("生成二维码失败：{error}"))?;
    Ok(code
        .render()
        .min_dimensions(240, 240)
        .quiet_zone(true)
        .dark_color(svg::Color("#000000"))
        .light_color(svg::Color("#ffffff"))
        .build())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn renders_a_square_svg_with_black_on_white() {
        let svg = render_svg("https://music.163.com/login?codekey=abcDEF123456").unwrap();

        assert!(svg.contains("<svg"));
        assert!(svg.contains("#000000"));
        assert!(svg.contains("#ffffff"));
        assert!(svg.trim_end().ends_with("</svg>"));
        // 宽高相等才是正方形。
        let attribute = |name: &str| {
            let marker = format!(" {name}=\"");
            let start = svg.find(&marker).unwrap() + marker.len();
            svg[start..].split('"').next().unwrap().to_owned()
        };
        assert_eq!(attribute("width"), attribute("height"));
    }

    #[test]
    fn different_content_gives_different_codes() {
        let first = render_svg("https://music.163.com/login?codekey=aaaaaaaa").unwrap();
        let second = render_svg("https://music.163.com/login?codekey=bbbbbbbb").unwrap();

        assert_ne!(first, second);
    }

    #[test]
    fn empty_content_is_rejected() {
        assert!(render_svg("  ").is_err());
    }
}
