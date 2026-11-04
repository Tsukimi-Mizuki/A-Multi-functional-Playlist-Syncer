//! 领域枚举。
//!
//! 对应 C# 的 `Core/Models/`（`AudioQuality`、`LyricsWriteMode`、`OnlineSourcePriority`、`RenderFrameRateMode`）、
//! `Core/Settings/ThemeMode.cs` 与 `Core/Online/LyricsSourceKind.cs`。
//!
//! 这些枚举会写进 `settings.json`，与 C# 版保持兼容：
//! - 序列化为**成员名字符串**（`System.Text.Json` 的 `JsonStringEnumConverter`）；
//! - 读取时名字不区分大小写，也接受已定义的整数值；
//! - 遇到无法识别的值时回落到默认成员，而不是让整份设置读取失败。

use serde::{Deserialize, Deserializer, Serialize, Serializer};
use serde_json::Value;

mod audio_quality;

/// 声明一个“按名字序列化、宽松读取”的设置枚举。
macro_rules! name_enum {
    (
        $(#[$meta:meta])*
        $vis:vis enum $name:ident {
            default $default:ident;
            $( $(#[$vmeta:meta])* $variant:ident = $value:literal => $text:literal ),+ $(,)?
        }
    ) => {
        $(#[$meta])*
        #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
        $vis enum $name {
            $( $(#[$vmeta])* $variant = $value ),+
        }

        impl $name {
            /// 所有成员，按数值升序。
            pub const ALL: &'static [$name] = &[ $( $name::$variant ),+ ];

            /// 序列化用的成员名（与 C# 枚举成员名一致）。
            pub const fn name(self) -> &'static str {
                match self { $( $name::$variant => $text ),+ }
            }

            /// 对应 C# 枚举的整数值。
            pub const fn value(self) -> i64 {
                self as i64
            }

            /// 按名字解析（去掉首尾空白，不区分大小写）。
            pub fn from_name(name: &str) -> Option<Self> {
                let name = name.trim();
                $( if name.eq_ignore_ascii_case($text) { return Some($name::$variant); } )+
                None
            }

            /// 按整数值解析，未定义的值返回 `None`。
            pub fn from_value(value: i64) -> Option<Self> {
                match value {
                    $( $value => Some($name::$variant), )+
                    _ => None,
                }
            }

            /// 从 JSON 值解析：字符串按名字（或数字字符串），整数按数值；无法识别返回 `None`。
            pub fn from_json(value: &Value) -> Option<Self> {
                match value {
                    Value::String(text) => Self::from_name(text)
                        .or_else(|| text.trim().parse::<i64>().ok().and_then(Self::from_value)),
                    Value::Number(number) => number.as_i64().and_then(Self::from_value),
                    _ => None,
                }
            }
        }

        impl Default for $name {
            fn default() -> Self {
                $name::$default
            }
        }

        impl Serialize for $name {
            fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
                serializer.serialize_str(self.name())
            }
        }

        impl<'de> Deserialize<'de> for $name {
            fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
                let value = Value::deserialize(deserializer)?;
                Ok(Self::from_json(&value).unwrap_or_default())
            }
        }
    };
}

name_enum! {
    /// 下载/试听音质，与网易云客户端的音质档位一一对应（从低到高）。
    pub enum AudioQuality {
        default ExHigh;
        /// 标准（MP3 128k）。
        Standard = 0 => "Standard",
        /// 极高 HQ（MP3 320k）。
        ExHigh = 1 => "ExHigh",
        /// 无损 SQ（FLAC）。
        Lossless = 2 => "Lossless",
        /// 高解析度无损 Hi-Res（FLAC）。
        HiRes = 3 => "HiRes",
        /// 高清臻音 Spatial Audio。
        SpatialAudio = 4 => "SpatialAudio",
        /// 沉浸环绕声 Surround Audio。
        SurroundAudio = 5 => "SurroundAudio",
        /// 超清母带 Master。
        Master = 6 => "Master",
        /// 臻音全景声 Audio Vivid。
        AudioVivid = 7 => "AudioVivid",
        /// 杜比全景声 Dolby Atmos。
        DolbyAtmos = 8 => "DolbyAtmos",
    }
}

impl AudioQuality {
    /// 解析设置文件里的音质名称：兼容旧版 `Higher`（已并入极高）。
    ///
    /// 对应 C# 的 `AudioQualityExtensions.TryParseSettingValue`；空白或无法识别返回 `None`。
    pub fn parse_setting_value(value: &str) -> Option<Self> {
        let trimmed = value.trim();
        if trimmed.is_empty() {
            return None;
        }
        if trimmed.eq_ignore_ascii_case("Higher") {
            return Some(AudioQuality::ExHigh);
        }
        Self::from_name(trimmed).or_else(|| trimmed.parse::<i64>().ok().and_then(Self::from_value))
    }
}

name_enum! {
    /// 歌词写入标签的方式。
    pub enum LyricsWriteMode {
        default Line;
        /// 不写歌词。
        None = 0 => "None",
        /// 逐行歌词。
        Line = 1 => "Line",
        /// 逐字歌词。
        Word = 2 => "Word",
    }
}

name_enum! {
    /// 在线标签/歌词来源优先级。
    pub enum OnlineSourcePriority {
        default NeteaseFirst;
        NeteaseFirst = 0 => "NeteaseFirst",
        QqMusicFirst = 1 => "QQMusicFirst",
        NeteaseOnly = 2 => "NeteaseOnly",
        QqMusicOnly = 3 => "QQMusicOnly",
    }
}

name_enum! {
    /// 界面渲染帧率上限。
    pub enum RenderFrameRateMode {
        default VSync;
        /// 跟随显示器垂直同步刷新率。
        VSync = 0 => "VSync",
        Fps30 = 30 => "Fps30",
        Fps60 = 60 => "Fps60",
        Fps90 = 90 => "Fps90",
        Fps120 = 120 => "Fps120",
        Fps144 = 144 => "Fps144",
        Fps165 = 165 => "Fps165",
        Fps240 = 240 => "Fps240",
    }
}

name_enum! {
    /// 界面主题：液态玻璃的深色/浅色两套配色，或跟随系统。
    pub enum ThemeMode {
        default Dark;
        /// 深色液态玻璃（默认）。
        Dark = 0 => "Dark",
        /// 浅色液态玻璃。
        Light = 1 => "Light",
        /// 跟随系统当前的应用主题。
        System = 2 => "System",
    }
}

name_enum! {
    /// 主窗口铺在网页下面的系统背景材质（Rust 重写版新增，C# 版没有这个设置）。
    pub enum WindowMaterial {
        default Acrylic;
        /// 亚克力：实时模糊并透出窗口下面的桌面与其他窗口（默认，液态玻璃效果依赖它）。
        Acrylic = 0 => "Acrylic",
        /// 云母：只取桌面壁纸的色调，不透出其他窗口，拖动 / 缩放时更省电。
        Mica = 1 => "Mica",
        /// 关闭系统材质，窗口完全透明，只剩界面自身的玻璃底色。
        None = 2 => "None",
    }
}

name_enum! {
    /// 歌词检索来源：主词源（网易云、QQ 音乐）与备用词源（酷狗、LRCLIB，只提供歌词）。
    pub enum LyricsSourceKind {
        default Netease;
        Netease = 0 => "Netease",
        QqMusic = 1 => "QQMusic",
        Kugou = 2 => "Kugou",
        Lrclib = 3 => "Lrclib",
    }
}

impl LyricsSourceKind {
    /// 是否为备用词源（只补歌词，不改写标签与封面）。
    pub fn is_fallback(self) -> bool {
        matches!(self, LyricsSourceKind::Kugou | LyricsSourceKind::Lrclib)
    }

    /// 下拉框与状态提示里显示的来源名。
    pub fn display_label(self) -> &'static str {
        match self {
            LyricsSourceKind::Netease => "网易云音乐",
            LyricsSourceKind::QqMusic => "QQ 音乐",
            LyricsSourceKind::Kugou => "酷狗音乐（仅歌词）",
            LyricsSourceKind::Lrclib => "LRCLIB（仅歌词）",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn names_are_serialized_as_csharp_member_names() {
        assert_eq!(
            serde_json::to_string(&AudioQuality::HiRes).unwrap(),
            "\"HiRes\""
        );
        assert_eq!(
            serde_json::to_string(&OnlineSourcePriority::QqMusicFirst).unwrap(),
            "\"QQMusicFirst\""
        );
        assert_eq!(
            serde_json::to_string(&LyricsSourceKind::QqMusic).unwrap(),
            "\"QQMusic\""
        );
        assert_eq!(
            serde_json::to_string(&RenderFrameRateMode::VSync).unwrap(),
            "\"VSync\""
        );
    }

    #[test]
    fn parsing_is_case_insensitive_and_accepts_numbers() {
        assert_eq!(
            ThemeMode::from_json(&json!("light")),
            Some(ThemeMode::Light)
        );
        assert_eq!(
            ThemeMode::from_json(&json!(" SYSTEM ")),
            Some(ThemeMode::System)
        );
        assert_eq!(ThemeMode::from_json(&json!(1)), Some(ThemeMode::Light));
        assert_eq!(
            RenderFrameRateMode::from_json(&json!(144)),
            Some(RenderFrameRateMode::Fps144)
        );
        assert_eq!(ThemeMode::from_json(&json!(99)), None);
        assert_eq!(ThemeMode::from_json(&json!("Neon")), None);
        assert_eq!(ThemeMode::from_json(&json!(null)), None);
        assert_eq!(ThemeMode::from_json(&json!(true)), None);
    }

    #[test]
    fn deserialization_falls_back_to_default() {
        let theme: ThemeMode = serde_json::from_str("99").unwrap();
        assert_eq!(theme, ThemeMode::Dark);
        let source: LyricsSourceKind = serde_json::from_str("\"nope\"").unwrap();
        assert_eq!(source, LyricsSourceKind::Netease);
    }

    #[test]
    fn legacy_higher_quality_maps_to_ex_high() {
        assert_eq!(
            AudioQuality::parse_setting_value("Higher"),
            Some(AudioQuality::ExHigh)
        );
        assert_eq!(
            AudioQuality::parse_setting_value(" higher "),
            Some(AudioQuality::ExHigh)
        );
        assert_eq!(
            AudioQuality::parse_setting_value("lossless"),
            Some(AudioQuality::Lossless)
        );
        assert_eq!(
            AudioQuality::parse_setting_value("3"),
            Some(AudioQuality::HiRes)
        );
        assert_eq!(AudioQuality::parse_setting_value("Ultra"), None);
        assert_eq!(AudioQuality::parse_setting_value("  "), None);
        assert_eq!(AudioQuality::parse_setting_value("99"), None);
    }

    #[test]
    fn values_match_csharp_discriminants() {
        assert_eq!(AudioQuality::DolbyAtmos.value(), 8);
        assert_eq!(RenderFrameRateMode::Fps240.value(), 240);
        assert_eq!(LyricsWriteMode::Word.value(), 2);
        assert_eq!(AudioQuality::ALL.len(), 9);
    }

    #[test]
    fn fallback_lyrics_sources_are_flagged() {
        assert!(LyricsSourceKind::Kugou.is_fallback());
        assert!(LyricsSourceKind::Lrclib.is_fallback());
        assert!(!LyricsSourceKind::Netease.is_fallback());
        assert_eq!(LyricsSourceKind::QqMusic.display_label(), "QQ 音乐");
    }
}
