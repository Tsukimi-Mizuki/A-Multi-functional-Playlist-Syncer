//! 音质档位到网易云接口参数 / 界面文案的映射。对应 C# `AudioQualityExtensions`。

use super::AudioQuality;

impl AudioQuality {
    /// 请求播放地址时使用的 `level` 字符串（首选写法）。
    pub const fn api_level(self) -> &'static str {
        match self {
            AudioQuality::Standard => "standard",
            AudioQuality::ExHigh => "exhigh",
            AudioQuality::Lossless => "lossless",
            AudioQuality::HiRes => "hires",
            AudioQuality::SpatialAudio => "jyeffect",
            AudioQuality::SurroundAudio => "sky",
            AudioQuality::Master => "jymaster",
            AudioQuality::AudioVivid => "audio_vivid",
            AudioQuality::DolbyAtmos => "dolby",
        }
    }

    /// 请求 `level` 字符串的候选写法：首个拿不到播放地址时依次退让（部分档位服务端有多个别名）。
    pub const fn api_levels(self) -> &'static [&'static str] {
        match self {
            AudioQuality::AudioVivid => &["audio_vivid", "vivid"],
            AudioQuality::Standard => &["standard"],
            AudioQuality::ExHigh => &["exhigh"],
            AudioQuality::Lossless => &["lossless"],
            AudioQuality::HiRes => &["hires"],
            AudioQuality::SpatialAudio => &["jyeffect"],
            AudioQuality::SurroundAudio => &["sky"],
            AudioQuality::Master => &["jymaster"],
            AudioQuality::DolbyAtmos => &["dolby"],
        }
    }

    /// 经典接口（`/weapi/song/enhance/player/url`）使用的 `br` 数值。
    pub const fn bitrate(self) -> i32 {
        match self {
            AudioQuality::Standard => 128_000,
            AudioQuality::ExHigh => 320_000,
            AudioQuality::Lossless => 999_000,
            _ => 1_999_000,
        }
    }

    /// 界面显示名称，与网易云客户端的音质命名一致。
    pub const fn display_name(self) -> &'static str {
        match self {
            AudioQuality::Standard => "标准",
            AudioQuality::ExHigh => "极高 (HQ)",
            AudioQuality::Lossless => "无损 (SQ)",
            AudioQuality::HiRes => "高解析度无损 (Hi-Res)",
            AudioQuality::SpatialAudio => "高清臻音 (SpatialAudio)",
            AudioQuality::SurroundAudio => "沉浸环绕声 (Surround Audio)",
            AudioQuality::Master => "超清母带 (Master)",
            AudioQuality::AudioVivid => "臻音全景声 (Audio Vivid)",
            AudioQuality::DolbyAtmos => "杜比全景声 (Dolby Atmos)",
        }
    }

    /// 把接口返回的 `level` 解析成音质档位（不区分大小写、忽略首尾空白，`higher` 是 `exhigh` 的旧写法）；
    /// 无法识别返回 `None`。
    pub fn from_api_level(level: &str) -> Option<Self> {
        match level.trim().to_lowercase().as_str() {
            "standard" => Some(AudioQuality::Standard),
            "higher" | "exhigh" => Some(AudioQuality::ExHigh),
            "lossless" => Some(AudioQuality::Lossless),
            "hires" => Some(AudioQuality::HiRes),
            "jyeffect" => Some(AudioQuality::SpatialAudio),
            "sky" => Some(AudioQuality::SurroundAudio),
            "jymaster" => Some(AudioQuality::Master),
            "audio_vivid" | "vivid" => Some(AudioQuality::AudioVivid),
            "dolby" => Some(AudioQuality::DolbyAtmos),
            _ => None,
        }
    }

    /// 从本档开始逐级降档的顺序：本档、次一档……直到标准（对应 C# `GetQualityFallbackChain`）。
    pub fn fallback_chain(self) -> Vec<AudioQuality> {
        AudioQuality::ALL
            .iter()
            .rev()
            .copied()
            .filter(|quality| quality.value() <= self.value())
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// C# `AudioQualityTests.MappingCases`。
    const MAPPING_CASES: [(AudioQuality, &str, &str, i32); 9] = [
        (AudioQuality::Standard, "standard", "标准", 128_000),
        (AudioQuality::ExHigh, "exhigh", "极高 (HQ)", 320_000),
        (AudioQuality::Lossless, "lossless", "无损 (SQ)", 999_000),
        (
            AudioQuality::HiRes,
            "hires",
            "高解析度无损 (Hi-Res)",
            1_999_000,
        ),
        (
            AudioQuality::SpatialAudio,
            "jyeffect",
            "高清臻音 (SpatialAudio)",
            1_999_000,
        ),
        (
            AudioQuality::SurroundAudio,
            "sky",
            "沉浸环绕声 (Surround Audio)",
            1_999_000,
        ),
        (
            AudioQuality::Master,
            "jymaster",
            "超清母带 (Master)",
            1_999_000,
        ),
        (
            AudioQuality::AudioVivid,
            "audio_vivid",
            "臻音全景声 (Audio Vivid)",
            1_999_000,
        ),
        (
            AudioQuality::DolbyAtmos,
            "dolby",
            "杜比全景声 (Dolby Atmos)",
            1_999_000,
        ),
    ];

    #[test]
    fn maps_to_api_level_display_name_and_bitrate() {
        for (quality, level, display_name, bitrate) in MAPPING_CASES {
            assert_eq!(quality.api_level(), level);
            assert_eq!(quality.display_name(), display_name);
            assert_eq!(quality.bitrate(), bitrate);
        }
        assert_eq!(MAPPING_CASES.len(), AudioQuality::ALL.len());
    }

    #[test]
    fn api_level_round_trips_back_to_quality_in_any_case() {
        for (quality, level, _, _) in MAPPING_CASES {
            assert_eq!(AudioQuality::from_api_level(level), Some(quality));
            assert_eq!(
                AudioQuality::from_api_level(&level.to_uppercase()),
                Some(quality)
            );
            assert_eq!(
                AudioQuality::from_api_level(&format!("  {level}\t")),
                Some(quality)
            );
        }
    }

    #[test]
    fn audio_vivid_offers_fallback_level_spellings() {
        assert_eq!(
            AudioQuality::AudioVivid.api_levels(),
            ["audio_vivid", "vivid"]
        );
        assert_eq!(AudioQuality::DolbyAtmos.api_levels(), ["dolby"]);
        assert_eq!(AudioQuality::SurroundAudio.api_levels(), ["sky"]);
        // 首选写法永远排在第一位。
        for quality in AudioQuality::ALL {
            assert_eq!(quality.api_levels()[0], quality.api_level());
        }
    }

    #[test]
    fn unknown_or_empty_levels_are_not_recognised() {
        assert_eq!(AudioQuality::from_api_level(""), None);
        assert_eq!(AudioQuality::from_api_level("   "), None);
        assert_eq!(AudioQuality::from_api_level("surround"), None);
    }

    #[test]
    fn legacy_higher_and_vivid_aliases_are_recognised() {
        assert_eq!(
            AudioQuality::from_api_level("higher"),
            Some(AudioQuality::ExHigh)
        );
        assert_eq!(
            AudioQuality::from_api_level("vivid"),
            Some(AudioQuality::AudioVivid)
        );
    }

    #[test]
    fn fallback_chain_starts_at_the_requested_level_and_descends_to_standard() {
        use AudioQuality::*;
        assert_eq!(HiRes.fallback_chain(), [HiRes, Lossless, ExHigh, Standard]);
        assert_eq!(Lossless.fallback_chain(), [Lossless, ExHigh, Standard]);
        assert_eq!(Standard.fallback_chain(), [Standard]);
        assert_eq!(
            DolbyAtmos.fallback_chain(),
            [
                DolbyAtmos,
                AudioVivid,
                Master,
                SurroundAudio,
                SpatialAudio,
                HiRes,
                Lossless,
                ExHigh,
                Standard
            ]
        );
    }
}
