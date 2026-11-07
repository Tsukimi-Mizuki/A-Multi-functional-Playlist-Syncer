//! 液态玻璃的高级参数（设置窗口「外观 → 高级设置」，`settings.json` 里的 `liquidGlass` 对象）。
//!
//! 参数的含义与取值范围沿用 liquid-glass-studio（MIT，见 `THIRD_PARTY_NOTICES.md`）：折射、色散、菲涅尔反射、
//! 高光、模糊、色调、阴影与超椭圆圆角。内置背景下由前端的 WebGL 真正计算；系统材质下界面用 CSS 近似同一组参数。
//! 参考实现里的宽 / 高 / 形状融合度 / 动画形变只用来摆弄示范形状，这里不保存（设置里的预览框自己处理）。
//!
//! 与外层设置一样：读文件时宽容（缺字段取默认、越界夹取、类型不对的字段回落默认），
//! 设置窗口提交的修改严格校验（未知参数、类型不对、越界整份拒绝）。

use std::collections::HashMap;

use serde::Serialize;
use serde_json::{Map, Value};

/// 一个数值参数：JSON 键、界面上的名字、合法范围、默认值与保留的小数位。
struct NumberSpec {
    key: &'static str,
    label: &'static str,
    min: f64,
    max: f64,
    default: f64,
    decimals: i32,
}

/// 声明全部数值参数：同一张表生成结构体字段、默认值、读取、夹取与校验，避免各处的范围不一致。
macro_rules! liquid_glass {
    (
        $( $(#[$meta:meta])* $field:ident : $key:literal, $label:literal, $min:expr, $max:expr, $default:expr, $decimals:expr; )+
    ) => {
        /// 液态玻璃的高级参数。
        #[derive(Debug, Clone, PartialEq, Serialize)]
        #[serde(rename_all = "camelCase")]
        pub struct LiquidGlass {
            $( $(#[$meta])* pub $field: f64, )+
            /// 模糊边缘：折射的边缘也用模糊后的背景。默认关：边缘按厚度从清晰过渡到模糊，
            /// 否则在默认的 24 px 模糊下折射几乎看不出来（参考实现默认只模糊 1 px，所以默认开）。
            pub blur_edge: bool,
            /// 色调：`#rrggbb`，空字符串表示跟随主题（深色 / 浅色各自的底色）；浓度沿用「玻璃浓度」。
            pub tint_color: String,
        }

        const NUMBERS: &[NumberSpec] = &[
            $( NumberSpec { key: $key, label: $label, min: $min, max: $max, default: $default, decimals: $decimals }, )+
        ];

        impl LiquidGlass {
            fn number_mut(&mut self, key: &str) -> Option<&mut f64> {
                match key {
                    $( $key => Some(&mut self.$field), )+
                    _ => None,
                }
            }
        }

        impl Default for LiquidGlass {
            fn default() -> Self {
                Self {
                    $( $field: $default, )+
                    blur_edge: false,
                    tint_color: String::new(),
                }
            }
        }
    };
}

liquid_glass! {
    /// 折射厚度（px）：边缘有多宽的一圈在折射。
    ref_thickness: "refThickness", "折射厚度", 1.0, 80.0, 20.0, 2;
    /// 背景距离：背景离玻璃越远，折射位移越大。
    ref_distance: "refDistance", "背景距离", 0.0, 0.2, 0.05, 3;
    /// 折射系数（折射率）。
    ref_factor: "refFactor", "折射系数", 1.0, 4.0, 1.4, 2;
    /// 色散增益：红 / 蓝通道折射量的差别，边缘出现彩边。
    ref_dispersion: "refDispersion", "色散增益", 0.0, 50.0, 7.0, 2;
    /// 菲涅尔反射范围：边缘亮起来的宽度。
    fresnel_range: "fresnelRange", "菲涅尔反射范围", 0.0, 100.0, 30.0, 2;
    /// 菲涅尔反射硬度：亮边是柔和过渡还是一条硬线。
    fresnel_hardness: "fresnelHardness", "菲涅尔反射硬度", 0.0, 100.0, 20.0, 2;
    /// 菲涅尔反射强度。
    fresnel_factor: "fresnelFactor", "菲涅尔反射强度", 0.0, 100.0, 20.0, 2;
    /// 高光范围：沿着边缘的高光宽度。
    glare_range: "glareRange", "高光范围", 0.0, 100.0, 30.0, 2;
    /// 高光硬度。
    glare_hardness: "glareHardness", "高光硬度", 0.0, 100.0, 20.0, 2;
    /// 高光强度。
    glare_factor: "glareFactor", "高光强度", 0.0, 120.0, 90.0, 2;
    /// 高光聚拢度：高光集中在光照方向的程度。
    glare_convergence: "glareConvergence", "高光聚拢度", 0.0, 100.0, 50.0, 2;
    /// 高光对侧强度：光照方向对面那一侧的高光。
    glare_opposite_factor: "glareOppositeFactor", "高光对侧强度", 0.0, 100.0, 80.0, 2;
    /// 高光角度（度）：光从哪个方向照过来。
    glare_angle: "glareAngle", "高光角度", -180.0, 180.0, -45.0, 2;
    /// 模糊半径（px）：玻璃后面的背景模糊多少。界面上有文字，默认比参考实现（1）模糊得多。
    blur_radius: "blurRadius", "模糊半径", 1.0, 200.0, 24.0, 0;
    /// 阴影扩散（px）。
    shadow_expand: "shadowExpand", "阴影扩散", 2.0, 100.0, 25.0, 2;
    /// 阴影强度。
    shadow_factor: "shadowFactor", "阴影强度", 0.0, 100.0, 15.0, 2;
    /// 阴影水平偏移（px，向右为正）。
    shadow_offset_x: "shadowOffsetX", "阴影水平位置", -20.0, 20.0, 0.0, 2;
    /// 阴影垂直偏移（px，向下为正）。
    shadow_offset_y: "shadowOffsetY", "阴影垂直位置", -20.0, 20.0, 10.0, 2;
    /// 圆角（px）：面板与对话框的圆角，按钮不超过自身高度的一半。
    corner_radius: "cornerRadius", "圆角", 0.0, 40.0, 16.0, 0;
    /// 超椭圆系数：2 是普通圆角，越大越接近「方中带圆」的超椭圆。
    superellipse: "superellipse", "超椭圆系数", 2.0, 7.0, 5.0, 2;
}

/// 「模糊边缘」与「色调」的 JSON 键。
const BLUR_EDGE: &str = "blurEdge";
const TINT_COLOR: &str = "tintColor";

impl LiquidGlass {
    /// 校验并归位：数值夹到合法范围并按小数位四舍五入，非有限数回落默认；色调不是 `#rrggbb` 就改为跟随主题。
    pub fn normalize(&mut self) {
        for spec in NUMBERS {
            if let Some(value) = self.number_mut(spec.key) {
                *value = clamp_number(spec, *value);
            }
        }
        self.tint_color = normalize_tint(&self.tint_color).unwrap_or_default();
    }

    /// 从 `settings.json` 里的对象读取：键名不区分大小写，缺字段或类型不对的字段取默认值。
    pub fn from_json(value: &Value) -> Self {
        let mut glass = Self::default();
        let Some(object) = value.as_object() else {
            return glass;
        };
        let fields: HashMap<String, &Value> = object
            .iter()
            .map(|(key, value)| (key.to_ascii_lowercase(), value))
            .collect();
        let get = |name: &str| fields.get(&name.to_ascii_lowercase()).copied();
        for spec in NUMBERS {
            if let Some(number) = get(spec.key).and_then(as_float)
                && let Some(value) = glass.number_mut(spec.key)
            {
                *value = number;
            }
        }
        if let Some(edge) = get(BLUR_EDGE).and_then(Value::as_bool) {
            glass.blur_edge = edge;
        }
        if let Some(tint) = get(TINT_COLOR).and_then(Value::as_str) {
            glass.tint_color = tint.to_owned();
        }
        glass.normalize();
        glass
    }

    /// 校验设置窗口提交的 `liquidGlass`：可以只含改动的参数，但每一项都必须是已知参数、类型正确且在范围内。
    pub fn validate_patch(patch: &Value) -> Result<(), String> {
        let Some(changes) = patch.as_object() else {
            return Err("液态玻璃参数必须是键值对象".to_owned());
        };
        for (key, value) in changes {
            if let Some(spec) = NUMBERS.iter().find(|spec| spec.key == key) {
                match value.as_f64() {
                    Some(number) if (spec.min..=spec.max).contains(&number) => {}
                    _ => {
                        return Err(format!(
                            "{}必须在 {}–{} 之间",
                            spec.label,
                            format_bound(spec.min),
                            format_bound(spec.max)
                        ));
                    }
                }
            } else if key == BLUR_EDGE {
                if !value.is_boolean() {
                    return Err("模糊边缘只能是开或关".to_owned());
                }
            } else if key == TINT_COLOR {
                let valid = value
                    .as_str()
                    .is_some_and(|text| text.trim().is_empty() || normalize_tint(text).is_some());
                if !valid {
                    return Err("色调必须是 #RRGGBB 形式的颜色，或留空跟随主题".to_owned());
                }
            } else {
                return Err(format!("未知的液态玻璃参数：{key}"));
            }
        }
        Ok(())
    }

    /// 把已校验的修改合并进当前参数（没提到的参数保持原样）。调用前先用 [`validate_patch`](Self::validate_patch)。
    pub fn merged(&self, patch: &Map<String, Value>) -> Self {
        let mut root = serde_json::to_value(self).unwrap_or(Value::Null);
        if let Some(object) = root.as_object_mut() {
            for (key, value) in patch {
                object.insert(key.clone(), value.clone());
            }
        }
        Self::from_json(&root)
    }
}

fn clamp_number(spec: &NumberSpec, value: f64) -> f64 {
    if !value.is_finite() {
        return spec.default;
    }
    let scale = 10f64.powi(spec.decimals);
    ((value * scale).round() / scale).clamp(spec.min, spec.max)
}

/// `#rrggbb`（不区分大小写，去掉首尾空白）统一成小写；其他写法返回 `None`。
fn normalize_tint(text: &str) -> Option<String> {
    let text = text.trim();
    let hex = text.strip_prefix('#')?;
    (hex.len() == 6 && hex.chars().all(|c| c.is_ascii_hexdigit()))
        .then(|| format!("#{}", hex.to_ascii_lowercase()))
}

/// 错误提示里的范围：整数不带小数点。
fn format_bound(value: f64) -> String {
    if value.fract() == 0.0 {
        format!("{value:.0}")
    } else {
        value.to_string()
    }
}

fn as_float(value: &Value) -> Option<f64> {
    match value {
        Value::Number(number) => number.as_f64(),
        Value::String(text) => text.trim().parse::<f64>().ok(),
        _ => None,
    }
}

#[cfg(test)]
mod tests;
