//! 声明行清理的测试。样例里的「歌词」都是为测试编的句子，不是任何真实歌曲的歌词。

use super::*;
use crate::lyrics::parser;

#[test]
fn recognizes_platform_notices_in_their_usual_forms() {
    for line in [
        "QQ音乐享有本翻译作品的著作权",
        "TME享有本翻译作品的著作权",
        "网易云音乐享有本翻译作品的著作权",
        "享有本翻译作品的著作权",
        "以下歌词翻译由文曲大模型提供",
        "以下歌词由某某字幕组提供",
        "以下歌词来自网友甲提供",
        "未经许可,不得翻唱或使用",
        "未经著作权人许可 不得翻唱 翻录或使用",
        "未经授权，禁止转载",
        "翻译授权网易云音乐使用",
        "翻译水平有限 授权网易云音乐使用",
        "翻译水平有限，授权QQ音乐使用",
        "感谢某某提供原歌词",
        "感谢某某提供原歌",
        "歌詞 / Eng Subs",
    ] {
        assert!(is_notice(line), "应当认作声明：{line}");
    }
}

#[test]
fn time_tags_brackets_width_case_and_spacing_do_not_matter() {
    for line in [
        // 行首与行尾都带时间标签（平台给声明行标的起止时间）
        "[00:00.000]QQ音乐享有本翻译作品的著作权[00:12.400]",
        "[00:00.121]TME享有本翻译作品的著作权[00:01.960]",
        "[00:02.020]以下歌词翻译由文曲大模型提供[00:03.260]",
        // 增强 LRC 的词标签
        "[00:01.00]<00:01.00>未经许可,<00:02.00>不得翻唱或使用<00:03.00>",
        // 括号包住、可以套多层
        "[00:12.500]【未经著作权人许可 不得翻唱 翻录或使用】",
        "[02:12.240]【感谢某某提供原歌词】[02:24.950]",
        "（「TME享有本翻译作品的著作权」）",
        // 全角标点、全角字母、字间空格、大小写
        "未经许可，不得翻唱或使用",
        "ＱＱ音乐享有本翻译作品的著作权",
        "qq音乐 享有 本翻译作品 的著作权",
        "tme享有本翻译作品的著作权",
        "歌詞/ENG SUBS",
    ] {
        assert!(is_notice(line), "应当认作声明：{line}");
    }
}

#[test]
fn traditional_chinese_spellings_are_recognized_too() {
    for line in [
        "QQ音樂享有本翻譯作品的著作權",
        "以下歌詞翻譯由文曲大模型提供",
        "未經著作權人許可 不得翻唱 翻錄或使用",
        "翻譯授權網易雲音樂使用",
        "感謝某某提供原歌詞",
    ] {
        assert!(is_notice(line), "应当认作声明：{line}");
    }
}

/// 最要紧的一条：带着「感谢 / 提供 / 声明 / 翻译 / 许可 / 著作权」这些词的歌词不能被删。
#[test]
fn lyrics_that_merely_contain_the_keywords_are_kept() {
    for line in [
        "感谢你陪我走过这一段",
        "他们提供的答案我都不想要",
        "这不是一份迟到的声明",
        "翻译不出口的心情",
        "未经许可的心动",
        "没有人享有这片星空的著作权吧",
        "以下是我想说的话",
        "我把歌词写在了掌心",
        // 声明的后面还跟着别的内容：不是整行声明，不删
        "QQ音乐享有本翻译作品的著作权 但这句是我编的歌词",
        // 只是被括号包着的普通歌词
        "（轻轻地说声谢谢）",
        "",
        "   ",
        "[00:10.00]",
    ] {
        assert!(!is_notice(line), "不应当认作声明：{line}");
    }
}

/// 脚本里默认保留的两类，这里同样保留。
#[test]
fn pure_music_hints_and_year_tags_are_kept() {
    for line in [
        "纯音乐，请欣赏",
        "此歌曲为没有填词的纯音乐，请您欣赏",
        "(2021年作品)",
        "作词 : 某某",
        "作曲 : 某某",
    ] {
        assert!(!is_notice(line), "不应当认作声明：{line}");
    }
}

#[test]
fn parse_online_drops_notice_lines_and_keeps_everything_else() {
    let raw = RawLyrics {
        lrc: Some(
            "[ti:测试]\n\
             [00:00.00]作词 : 某某\n\
             [00:01.00]未经许可,不得翻唱或使用\n\
             [00:05.00]第一句歌词\n\
             [00:09.00]感谢你陪我走过这一段\n\
             [00:13.00]【感谢某某提供原歌词】\n"
                .to_owned(),
        ),
        ..RawLyrics::default()
    };

    // 不清理的解析器照旧保留全部 5 行（与 C# 版一致）。
    assert_eq!(parser::parse(&raw).lines.len(), 5);

    let document = parse_online(&raw);
    assert_eq!(
        parser::to_plain_lrc(&document, true),
        "[00:00.000]作词 : 某某\n[00:05.000]第一句歌词\n[00:09.000]感谢你陪我走过这一段"
    );
}

/// 声明在译文里：配到歌词行上的只去掉译文，歌词行留着；没配上的译文行本来就不会写出来。
#[test]
fn notices_in_the_translation_only_lose_the_translation() {
    let raw = RawLyrics {
        lrc: Some("[00:00.00]first line\n[00:02.00]second line\n[00:06.00]third line\n".to_owned()),
        translated_lrc: Some(
            "[00:00.00]QQ音乐享有本翻译作品的著作权\n\
             [00:02.00]以下歌词翻译由文曲大模型提供\n\
             [00:06.00]第三句的译文\n"
                .to_owned(),
        ),
        ..RawLyrics::default()
    };

    let document = parse_online(&raw);
    assert_eq!(document.lines.len(), 3);
    assert_eq!(document.lines[0].translation, None);
    assert_eq!(document.lines[1].translation, None);
    assert_eq!(
        document.lines[2].translation.as_deref(),
        Some("第三句的译文")
    );
    assert_eq!(
        parser::to_plain_lrc(&document, true),
        "[00:00.000]first line\n[00:02.000]second line\n[00:06.000]third line\n[00:06.000]第三句的译文"
    );
    // 留着的原始译文文本里也没有声明了。
    assert_eq!(
        document.translation.as_deref(),
        Some("[00:06.00]第三句的译文\n")
    );
}

#[test]
fn a_translation_made_only_of_notices_counts_as_no_translation() {
    let raw = RawLyrics {
        lrc: Some("[00:00.00]first line\n[00:04.00]second line\n".to_owned()),
        translated_lrc: Some("[00:00.00]TME享有本翻译作品的著作权\n".to_owned()),
        ..RawLyrics::default()
    };

    let document = parse_online(&raw);
    assert!(!document.has_translation());
    assert_eq!(document.translation, None);
}

#[test]
fn word_level_lines_are_judged_by_their_whole_text() {
    // QRC：`[行起始,行时长]词(词起始,词时长)`
    let raw = RawLyrics {
        qrc: Some(
            "[0,2000]未经许可,(0,1000)不得翻唱或使用(1000,1000)\n\
             [3000,2000]第一句(3000,1000)歌词(4000,1000)\n"
                .to_owned(),
        ),
        ..RawLyrics::default()
    };

    let document = parse_online(&raw);
    assert_eq!(document.lines.len(), 1);
    assert_eq!(document.lines[0].text, "第一句歌词");
    assert!(document.has_word_level());
}

#[test]
fn lyrics_made_only_of_notices_become_empty() {
    let raw = RawLyrics::from_lrc(
        "[00:00.00]QQ音乐享有本翻译作品的著作权\n[00:01.00]未经许可,不得翻唱或使用\n",
    );
    let document = parse_online(&raw);
    assert!(document.is_empty());
    assert_eq!(parser::to_plain_lrc(&document, true), "");
}

#[test]
fn strip_document_reports_how_many_places_it_cleaned() {
    let mut document = parser::parse(&RawLyrics {
        lrc: Some("[00:00.00]未经许可,不得翻唱或使用\n[00:03.00]一句歌词\n".to_owned()),
        translated_lrc: Some("[00:03.00]TME享有本翻译作品的著作权\n".to_owned()),
        ..RawLyrics::default()
    });
    assert_eq!(strip_document(&mut document), 2);
    assert_eq!(strip_document(&mut document), 0);
}

#[test]
fn strip_lrc_text_removes_only_notice_lines_and_keeps_line_endings() {
    let text = "[ti:测试]\r\n\
                [00:00.000]QQ音乐享有本翻译作品的著作权[00:12.400]\r\n\
                未经许可,不得翻唱或使用\r\n\
                [00:12.50]一句歌词\r\n\
                \r\n\
                [00:15.00]感谢你陪我走过这一段";
    assert_eq!(
        strip_lrc_text(text),
        "[ti:测试]\r\n[00:12.50]一句歌词\r\n\r\n[00:15.00]感谢你陪我走过这一段"
    );
    assert_eq!(strip_lrc_text(""), "");
    assert_eq!(
        strip_lrc_text("[00:01.00]一句歌词\n"),
        "[00:01.00]一句歌词\n"
    );
}
