//! HTML 实体解码，行为对齐 .NET 的 `WebUtility.HtmlDecode`（QRC 外壳里的 `&amp;` 等需要还原）。

/// 0xA0..=0xFF 的 Latin-1 实体名，按码位顺序排列。
const LATIN1: [&str; 96] = [
    "nbsp", "iexcl", "cent", "pound", "curren", "yen", "brvbar", "sect", "uml", "copy", "ordf",
    "laquo", "not", "shy", "reg", "macr", "deg", "plusmn", "sup2", "sup3", "acute", "micro",
    "para", "middot", "cedil", "sup1", "ordm", "raquo", "frac14", "frac12", "frac34", "iquest",
    "Agrave", "Aacute", "Acirc", "Atilde", "Auml", "Aring", "AElig", "Ccedil", "Egrave", "Eacute",
    "Ecirc", "Euml", "Igrave", "Iacute", "Icirc", "Iuml", "ETH", "Ntilde", "Ograve", "Oacute",
    "Ocirc", "Otilde", "Ouml", "times", "Oslash", "Ugrave", "Uacute", "Ucirc", "Uuml", "Yacute",
    "THORN", "szlig", "agrave", "aacute", "acirc", "atilde", "auml", "aring", "aelig", "ccedil",
    "egrave", "eacute", "ecirc", "euml", "igrave", "iacute", "icirc", "iuml", "eth", "ntilde",
    "ograve", "oacute", "ocirc", "otilde", "ouml", "divide", "oslash", "ugrave", "uacute", "ucirc",
    "uuml", "yacute", "thorn", "yuml",
];

/// 其余实体（HTML 4.01 的特殊字符、希腊字母、符号，外加 `&apos;`）。
const OTHERS: &[(&str, u32)] = &[
    ("quot", 34),
    ("amp", 38),
    ("apos", 39),
    ("lt", 60),
    ("gt", 62),
    ("OElig", 338),
    ("oelig", 339),
    ("Scaron", 352),
    ("scaron", 353),
    ("Yuml", 376),
    ("fnof", 402),
    ("circ", 710),
    ("tilde", 732),
    ("Alpha", 913),
    ("Beta", 914),
    ("Gamma", 915),
    ("Delta", 916),
    ("Epsilon", 917),
    ("Zeta", 918),
    ("Eta", 919),
    ("Theta", 920),
    ("Iota", 921),
    ("Kappa", 922),
    ("Lambda", 923),
    ("Mu", 924),
    ("Nu", 925),
    ("Xi", 926),
    ("Omicron", 927),
    ("Pi", 928),
    ("Rho", 929),
    ("Sigma", 931),
    ("Tau", 932),
    ("Upsilon", 933),
    ("Phi", 934),
    ("Chi", 935),
    ("Psi", 936),
    ("Omega", 937),
    ("alpha", 945),
    ("beta", 946),
    ("gamma", 947),
    ("delta", 948),
    ("epsilon", 949),
    ("zeta", 950),
    ("eta", 951),
    ("theta", 952),
    ("iota", 953),
    ("kappa", 954),
    ("lambda", 955),
    ("mu", 956),
    ("nu", 957),
    ("xi", 958),
    ("omicron", 959),
    ("pi", 960),
    ("rho", 961),
    ("sigmaf", 962),
    ("sigma", 963),
    ("tau", 964),
    ("upsilon", 965),
    ("phi", 966),
    ("chi", 967),
    ("psi", 968),
    ("omega", 969),
    ("thetasym", 977),
    ("upsih", 978),
    ("piv", 982),
    ("ensp", 8194),
    ("emsp", 8195),
    ("thinsp", 8201),
    ("zwnj", 8204),
    ("zwj", 8205),
    ("lrm", 8206),
    ("rlm", 8207),
    ("ndash", 8211),
    ("mdash", 8212),
    ("lsquo", 8216),
    ("rsquo", 8217),
    ("sbquo", 8218),
    ("ldquo", 8220),
    ("rdquo", 8221),
    ("bdquo", 8222),
    ("dagger", 8224),
    ("Dagger", 8225),
    ("bull", 8226),
    ("hellip", 8230),
    ("permil", 8240),
    ("prime", 8242),
    ("Prime", 8243),
    ("lsaquo", 8249),
    ("rsaquo", 8250),
    ("oline", 8254),
    ("frasl", 8260),
    ("euro", 8364),
    ("image", 8465),
    ("weierp", 8472),
    ("real", 8476),
    ("trade", 8482),
    ("alefsym", 8501),
    ("larr", 8592),
    ("uarr", 8593),
    ("rarr", 8594),
    ("darr", 8595),
    ("harr", 8596),
    ("crarr", 8629),
    ("lArr", 8656),
    ("uArr", 8657),
    ("rArr", 8658),
    ("dArr", 8659),
    ("hArr", 8660),
    ("forall", 8704),
    ("part", 8706),
    ("exist", 8707),
    ("empty", 8709),
    ("nabla", 8711),
    ("isin", 8712),
    ("notin", 8713),
    ("ni", 8715),
    ("prod", 8719),
    ("sum", 8721),
    ("minus", 8722),
    ("lowast", 8727),
    ("radic", 8730),
    ("prop", 8733),
    ("infin", 8734),
    ("ang", 8736),
    ("and", 8743),
    ("or", 8744),
    ("cap", 8745),
    ("cup", 8746),
    ("int", 8747),
    ("there4", 8756),
    ("sim", 8764),
    ("cong", 8773),
    ("asymp", 8776),
    ("ne", 8800),
    ("equiv", 8801),
    ("le", 8804),
    ("ge", 8805),
    ("sub", 8834),
    ("sup", 8835),
    ("nsub", 8836),
    ("sube", 8838),
    ("supe", 8839),
    ("oplus", 8853),
    ("otimes", 8855),
    ("perp", 8869),
    ("sdot", 8901),
    ("lceil", 8968),
    ("rceil", 8969),
    ("lfloor", 8970),
    ("rfloor", 8971),
    ("lang", 9001),
    ("rang", 9002),
    ("loz", 9674),
    ("spades", 9824),
    ("clubs", 9827),
    ("hearts", 9829),
    ("diams", 9830),
];

/// 命名实体到字符；大小写敏感。
fn named_entity(name: &str) -> Option<char> {
    if let Some(index) = LATIN1.iter().position(|candidate| *candidate == name) {
        return char::from_u32(160 + index as u32);
    }
    OTHERS
        .iter()
        .find(|(candidate, _)| *candidate == name)
        .and_then(|(_, code)| char::from_u32(*code))
}

fn parse_unsigned(text: &str, radix: u32) -> Option<u32> {
    if text.is_empty() || !text.chars().all(|ch| ch.is_digit(radix)) {
        return None;
    }
    u32::from_str_radix(text, radix).ok()
}

/// 解析 `&#123;` / `&#x1F;` 里 `#` 之后的部分；0、代理项与超出 Unicode 范围一律无效（与 .NET 一致）。
fn numeric_entity(body: &str) -> Option<char> {
    let value = match body.strip_prefix(['x', 'X']) {
        Some(hex) => parse_unsigned(hex, 16)?,
        None => parse_unsigned(body, 10)?,
    };
    if value == 0 || value > 0x10FFFF || (0xD800..=0xDFFF).contains(&value) {
        return None;
    }
    char::from_u32(value)
}

/// 还原 HTML 实体；不认识的实体原样保留。
///
/// 与 .NET 一致：`&` 之后、下一个 `;` 或 `&` 之前才算实体名；数值实体无效时只输出 `&`，其余按普通文本继续。
pub fn html_decode(value: &str) -> String {
    if !value.contains('&') {
        return value.to_owned();
    }

    let mut output = String::with_capacity(value.len());
    let mut rest = value;
    while let Some(amp) = rest.find('&') {
        output.push_str(&rest[..amp]);
        let after = &rest[amp + 1..];
        let terminated = after
            .find([';', '&'])
            .filter(|end| *end > 0 && after.as_bytes()[*end] == b';');
        let Some(end) = terminated else {
            output.push('&');
            rest = after;
            continue;
        };

        let entity = &after[..end];
        if let Some(body) = entity.strip_prefix('#') {
            match numeric_entity(body) {
                Some(ch) => {
                    output.push(ch);
                    rest = &after[end + 1..];
                }
                None => {
                    output.push('&');
                    rest = after;
                }
            }
        } else if let Some(ch) = named_entity(entity) {
            output.push(ch);
            rest = &after[end + 1..];
        } else {
            output.push('&');
            output.push_str(entity);
            output.push(';');
            rest = &after[end + 1..];
        }
    }
    output.push_str(rest);
    output
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decodes_the_xml_basics() {
        assert_eq!(html_decode("Tom &amp; Jerry"), "Tom & Jerry");
        assert_eq!(
            html_decode("&lt;b&gt; &quot;x&quot; &apos;y&apos;"),
            "<b> \"x\" 'y'"
        );
    }

    #[test]
    fn decodes_numeric_entities() {
        assert_eq!(html_decode("&#39;&#x41;&#X42;"), "'AB");
        assert_eq!(html_decode("&#x1F600;"), "\u{1F600}");
    }

    #[test]
    fn decodes_named_latin_and_symbol_entities() {
        assert_eq!(
            html_decode("caf&eacute; &copy; &hellip; &euro;"),
            "café © … €"
        );
        assert_eq!(html_decode("&Eacute;&eacute;&yuml;&nbsp;"), "Ééÿ\u{a0}");
    }

    #[test]
    fn keeps_unknown_or_unterminated_entities() {
        assert_eq!(html_decode("&unknown; &amp"), "&unknown; &amp");
        assert_eq!(html_decode("a & b"), "a & b");
        assert_eq!(html_decode("&;"), "&;");
        assert_eq!(html_decode("&#;"), "&#;");
    }

    #[test]
    fn invalid_numeric_entities_are_left_alone() {
        assert_eq!(html_decode("&#0;"), "&#0;");
        assert_eq!(html_decode("&#xD800;"), "&#xD800;");
        assert_eq!(html_decode("&#99999999;"), "&#99999999;");
        assert_eq!(html_decode("&#abc;"), "&#abc;");
    }

    #[test]
    fn a_new_ampersand_restarts_entity_scanning() {
        assert_eq!(html_decode("&&amp;"), "&&");
        assert_eq!(html_decode("&am&amp;"), "&am&");
    }

    #[test]
    fn text_without_ampersands_is_returned_unchanged() {
        assert_eq!(html_decode("普通文本"), "普通文本");
    }
}
