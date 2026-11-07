use serde_json::json;

use super::*;

#[test]
fn defaults_follow_the_reference_except_for_a_readable_blur() {
    let glass = LiquidGlass::default();
    assert_eq!(glass.ref_thickness, 20.0);
    assert_eq!(glass.ref_distance, 0.05);
    assert_eq!(glass.ref_factor, 1.4);
    assert_eq!(glass.ref_dispersion, 7.0);
    assert_eq!(glass.glare_angle, -45.0);
    assert_eq!(glass.glare_factor, 90.0);
    // 界面上有文字：比参考实现的 1 px 模糊得多。
    assert_eq!(glass.blur_radius, 24.0);
    assert_eq!(glass.shadow_offset_y, 10.0, "阴影默认落在下方");
    assert_eq!(glass.corner_radius, 16.0);
    assert_eq!(glass.superellipse, 5.0);
    assert!(!glass.blur_edge, "默认清晰的折射边缘");
    assert_eq!(glass.tint_color, "");
    // 默认值本身就在范围内。
    let mut normalized = glass.clone();
    normalized.normalize();
    assert_eq!(normalized, glass);
}

#[test]
fn serializes_every_parameter_with_camel_case_keys() {
    let value = serde_json::to_value(LiquidGlass::default()).unwrap();
    let object = value.as_object().unwrap();
    assert_eq!(object.len(), NUMBERS.len() + 2);
    for key in [
        "refThickness",
        "refDistance",
        "refFactor",
        "refDispersion",
        "fresnelRange",
        "fresnelHardness",
        "fresnelFactor",
        "glareRange",
        "glareHardness",
        "glareFactor",
        "glareConvergence",
        "glareOppositeFactor",
        "glareAngle",
        "blurRadius",
        "shadowExpand",
        "shadowFactor",
        "shadowOffsetX",
        "shadowOffsetY",
        "cornerRadius",
        "superellipse",
        "blurEdge",
        "tintColor",
    ] {
        assert!(object.contains_key(key), "缺少参数 {key}");
    }
}

#[test]
fn reading_is_lenient_field_by_field() {
    let glass = LiquidGlass::from_json(&json!({
        "REFTHICKNESS": 33.333,
        "refDistance": 0.12345,
        "glareAngle": -720,
        "blurRadius": 7.6,
        "cornerRadius": "12",
        "superellipse": "方",
        "fresnelFactor": null,
        "blurEdge": "yes",
        "tintColor": " #A0B1C2 ",
        "unknownKnob": 3,
    }));
    assert_eq!(glass.ref_thickness, 33.33);
    assert_eq!(glass.ref_distance, 0.123);
    assert_eq!(glass.glare_angle, -180.0, "越界夹到下限");
    assert_eq!(glass.blur_radius, 8.0, "模糊半径取整");
    assert_eq!(glass.corner_radius, 12.0, "带引号的数字也接受");
    assert_eq!(glass.superellipse, 5.0, "无法识别的值回落默认");
    assert_eq!(glass.fresnel_factor, 20.0);
    assert!(!glass.blur_edge, "类型不对的开关保持默认");
    assert_eq!(glass.tint_color, "#a0b1c2");
    assert_eq!(
        LiquidGlass::from_json(&json!("不是对象")),
        LiquidGlass::default()
    );
}

#[test]
fn an_invalid_tint_falls_back_to_the_theme() {
    for tint in ["red", "#12345", "#1234567", "#GGGGGG", "123456"] {
        let glass = LiquidGlass::from_json(&json!({ "tintColor": tint }));
        assert_eq!(glass.tint_color, "", "{tint}");
    }
}

#[test]
fn patches_are_validated_strictly() {
    assert!(
        LiquidGlass::validate_patch(
            &json!({ "refThickness": 80, "glareAngle": -180.0, "blurEdge": false, "tintColor": "" })
        )
        .is_ok()
    );
    assert!(LiquidGlass::validate_patch(&json!({ "tintColor": "#FFAA00" })).is_ok());

    let error = LiquidGlass::validate_patch(&json!({ "refFactor": 4.01 })).unwrap_err();
    assert_eq!(error, "折射系数必须在 1–4 之间");
    let error = LiquidGlass::validate_patch(&json!({ "refDistance": -0.1 })).unwrap_err();
    assert_eq!(error, "背景距离必须在 0–0.2 之间");
    assert!(LiquidGlass::validate_patch(&json!({ "blurRadius": "24" })).is_err());
    assert!(LiquidGlass::validate_patch(&json!({ "blurEdge": 1 })).is_err());
    let error = LiquidGlass::validate_patch(&json!({ "tintColor": "blue" })).unwrap_err();
    assert!(error.contains("#RRGGBB"));
    let error = LiquidGlass::validate_patch(&json!({ "shapeWidth": 200 })).unwrap_err();
    assert_eq!(error, "未知的液态玻璃参数：shapeWidth");
    assert!(LiquidGlass::validate_patch(&json!([1, 2])).is_err());
}

#[test]
fn merging_a_patch_keeps_the_parameters_it_does_not_mention() {
    let current = LiquidGlass {
        glare_angle: 30.0,
        tint_color: "#112233".to_owned(),
        ..LiquidGlass::default()
    };
    let patch = json!({ "refDispersion": 12.5, "tintColor": "" });

    let next = current.merged(patch.as_object().unwrap());

    assert_eq!(next.ref_dispersion, 12.5);
    assert_eq!(next.tint_color, "");
    assert_eq!(next.glare_angle, 30.0);
    assert_eq!(next.ref_thickness, current.ref_thickness);
}
