use super::*;
use crate::logs::LogLevel;
use crate::netease::http::{HttpMethod, TransportError};
use crate::test_util::FakeTransport;

fn v(text: &str) -> Version {
    Version::parse(text).unwrap_or_else(|| panic!("{text} 应能解析"))
}

fn release_json(tag: &str) -> String {
    serde_json::json!({
        "tag_name": tag,
        "name": format!("Lunatic Princess {tag}"),
        "body": "改进与修复：\r\n- 新功能 A\r\n- 修复 B\r\n\r\n**Full Changelog**: https://github.com/Tsukimi-Mizuki/A-Multi-functional-Playlist-Syncer/compare/v2.0...v2.1",
        "html_url": format!("{RELEASES_PAGE}/tag/{tag}"),
        "published_at": "2026-11-05T08:30:00Z",
        "draft": false,
        "prerelease": false,
        "assets": []
    })
    .to_string()
}

fn checker(transport: &Arc<FakeTransport>, current: &str) -> UpdateChecker {
    UpdateChecker::new(Arc::clone(transport) as Arc<dyn HttpTransport>, current)
}

fn request(manual: bool, auto_check: bool, ignored: &str) -> UpdateRequest {
    UpdateRequest {
        manual,
        auto_check,
        ignored_version: ignored.to_owned(),
    }
}

#[test]
fn tags_are_normalized_like_the_release_script() {
    assert_eq!(v("v2.1").to_string(), "2.1.0");
    assert_eq!(v("v2.0").to_string(), "2.0.0");
    assert_eq!(v("V2.0.0-rc.1").to_string(), "2.0.0-rc.1");
    assert_eq!(v("v.1.0.0").to_string(), "1.0.0");
    assert_eq!(v("refs/tags/v1.3").to_string(), "1.3.0");
    assert_eq!(v(" 3 ").to_string(), "3.0.0");
    assert_eq!(v("1.2.3-rc-1").to_string(), "1.2.3-rc-1");
    // 构建元数据不参与比较，读取时丢掉。
    assert_eq!(v("1.2.3+build.7").to_string(), "1.2.3");
    assert_eq!(v("1.2.3+build.7"), v("1.2.3"));
}

#[test]
fn text_that_is_not_semver_is_rejected() {
    for text in [
        "",
        "v",
        "abc",
        "1.2.3.4",
        "01.2",
        "1.02",
        "1..2",
        "1.",
        ".",
        "1.2-",
        "1.2-rc..1",
        "1.2+",
        "1.2-rc_1",
        "v1.2 beta",
        "99999999999999999999",
    ] {
        assert_eq!(Version::parse(text), None, "{text:?} 不该被接受");
    }
}

#[test]
fn versions_follow_semver_precedence() {
    let ordered = [
        "1.0.0-alpha",
        "1.0.0-alpha.1",
        "1.0.0-alpha.beta",
        "1.0.0-beta",
        "1.0.0-beta.2",
        "1.0.0-beta.11",
        "1.0.0-rc.1",
        "1.0.0",
        "1.0.9",
        "1.0.10",
        "1.3.0",
        "2.0.0-rc.1",
        "2.0.0",
        "2.1.0",
    ];
    for pair in ordered.windows(2) {
        assert!(v(pair[0]) < v(pair[1]), "{} 应低于 {}", pair[0], pair[1]);
    }
    assert_eq!(v("v2.1").cmp(&v("2.1.0")), Ordering::Equal);
    assert!(v("2.1.0-manual.7").is_prerelease());
}

#[test]
fn only_a_newer_release_counts_as_an_update() {
    assert_eq!(
        classify(&v("2.1.0"), &v("2.2.0"), ""),
        UpdateStatus::Available
    );
    assert_eq!(
        classify(&v("2.1.0"), &v("2.1.0"), ""),
        UpdateStatus::UpToDate
    );
    // 自己构建的更高预发布版不会被“降级提醒”。
    assert_eq!(
        classify(&v("2.2.0-rc.1"), &v("2.1.0"), ""),
        UpdateStatus::UpToDate
    );
    // 同号预发布在正式版面前是旧版本。
    assert_eq!(
        classify(&v("2.1.0-manual.7"), &v("2.1.0"), ""),
        UpdateStatus::Available
    );
}

#[test]
fn an_ignored_version_is_skipped_until_a_newer_one_ships() {
    assert_eq!(
        classify(&v("2.1.0"), &v("2.2.0"), "2.2.0"),
        UpdateStatus::Ignored
    );
    // 记住的是 tag 写法也认得。
    assert_eq!(
        classify(&v("2.1.0"), &v("2.2.0"), "v2.2"),
        UpdateStatus::Ignored
    );
    assert_eq!(
        classify(&v("2.1.0"), &v("2.3.0"), "2.2.0"),
        UpdateStatus::Available
    );
    assert_eq!(
        classify(&v("2.1.0"), &v("2.2.0"), "不是版本号"),
        UpdateStatus::Available
    );
    // 已经是最新版时忽略记录无关紧要。
    assert_eq!(
        classify(&v("2.2.0"), &v("2.2.0"), "2.2.0"),
        UpdateStatus::UpToDate
    );
}

#[test]
fn the_latest_release_is_parsed_into_what_the_dialog_shows() {
    let (version, info) = parse_release(&release_json("v2.2")).unwrap();
    assert_eq!(version, v("2.2.0"));
    assert_eq!(info.version, "2.2.0");
    assert_eq!(info.tag, "v2.2");
    assert_eq!(info.notes, "改进与修复：\n- 新功能 A\n- 修复 B");
    assert_eq!(info.url, format!("{RELEASES_PAGE}/tag/v2.2"));
    assert_eq!(info.published_on.as_deref(), Some("2026-11-05"));

    let json = serde_json::to_value(&info).unwrap();
    assert_eq!(json["publishedOn"], "2026-11-05");
}

#[test]
fn missing_or_foreign_fields_fall_back_to_safe_values() {
    let json = serde_json::json!({
        "tag_name": " v2.2 ",
        "name": "  ",
        "body": null,
        "html_url": "https://evil.example.com/releases/tag/v2.2",
        "published_at": "yesterday"
    })
    .to_string();
    let (_, info) = parse_release(&json).unwrap();
    assert_eq!(info.tag, "v2.2");
    assert_eq!(info.notes, "");
    assert_eq!(info.url, format!("{RELEASES_PAGE}/latest"));
    assert_eq!(info.published_on, None);
}

#[test]
fn unusable_responses_are_reported_in_chinese() {
    let error = parse_release("<html>").unwrap_err();
    assert_eq!(error.kind, UpdateErrorKind::Invalid);
    assert!(error.message.starts_with("GitHub 返回的版本信息无法识别"));

    let error = parse_release(&release_json("nightly")).unwrap_err();
    assert_eq!(error.kind, UpdateErrorKind::Invalid);
    assert!(error.message.contains("tag「nightly」不是版本号"));

    let prerelease =
        serde_json::json!({ "tag_name": "v3.0.0-rc.1", "prerelease": true }).to_string();
    assert_eq!(
        parse_release(&prerelease).unwrap_err().kind,
        UpdateErrorKind::NoRelease
    );
}

#[test]
fn release_notes_become_plain_text() {
    let body = "## 改进\r\n\r\n\r\n- A\r\n#话题不是标题\r\n### 修复  \n- B\n\n\n**Full Changelog**: https://x\n\n";
    assert_eq!(
        release_notes_text(body),
        "改进\n\n- A\n#话题不是标题\n修复\n- B"
    );
    assert_eq!(release_notes_text("\n\n"), "");

    let long = "长".repeat(MAX_NOTES_CHARS + 10);
    let cut = release_notes_text(&long);
    assert_eq!(cut.chars().count(), MAX_NOTES_CHARS + 1);
    assert!(cut.ends_with('…'));
}

#[test]
fn only_this_projects_release_pages_may_be_opened() {
    for url in [
        RELEASES_PAGE.to_owned(),
        format!("{RELEASES_PAGE}/latest"),
        format!("{RELEASES_PAGE}/tag/v2.1"),
        format!("{RELEASES_PAGE}/tag/v2.0.0-rc.1"),
    ] {
        assert!(is_release_page(&url), "{url} 应允许打开");
    }
    for url in [
        "https://github.com/someone/else/releases/tag/v2.1",
        "http://github.com/Tsukimi-Mizuki/A-Multi-functional-Playlist-Syncer/releases",
        "https://github.com/Tsukimi-Mizuki/A-Multi-functional-Playlist-Syncer/releasesx",
        "https://github.com/Tsukimi-Mizuki/A-Multi-functional-Playlist-Syncer/releases.evil.com",
        "https://github.com/Tsukimi-Mizuki/A-Multi-functional-Playlist-Syncer/releases/tag/v2 1",
        "https://github.com/Tsukimi-Mizuki/A-Multi-functional-Playlist-Syncer/releases/@evil.com",
        "https://github.com/Tsukimi-Mizuki/A-Multi-functional-Playlist-Syncer/releases/x?y=\"z\"",
        "javascript:alert(1)",
        "",
    ] {
        assert!(!is_release_page(url), "{url} 不该允许打开");
    }
    // 不合格的地址在调用系统外壳之前就被拒绝。
    assert_eq!(
        open_release_page("https://evil.example.com/"),
        Err("只能打开本项目在 GitHub 上的发布页".to_owned())
    );
}

#[tokio::test]
async fn check_asks_github_for_the_latest_release() {
    let transport = FakeTransport::new();
    transport.enqueue_json(&release_json("v2.2"));

    let check = checker(&transport, "2.1.0").check("").await.unwrap();

    assert_eq!(check.current_version, "2.1.0");
    assert_eq!(check.status, UpdateStatus::Available);
    assert_eq!(check.release.version, "2.2.0");
    let requests = transport.requests();
    assert_eq!(requests.len(), 1);
    let request = &requests[0];
    assert_eq!(request.method, HttpMethod::Get);
    assert_eq!(request.url, LATEST_RELEASE_API);
    assert_eq!(
        request.header("Accept"),
        Some("application/vnd.github+json")
    );
    assert_eq!(request.header("User-Agent"), Some("LunaticPrincess/2.1.0"));
    assert_eq!(request.header("Cookie"), None);

    let json = serde_json::to_value(&check).unwrap();
    assert_eq!(json["currentVersion"], "2.1.0");
    assert_eq!(json["status"], "available");
}

#[tokio::test]
async fn http_failures_map_to_error_kinds() {
    let cases: [(u16, UpdateErrorKind, &str); 4] = [
        (
            403,
            UpdateErrorKind::RateLimited,
            "GitHub 暂时限制了访问频率，请过一会儿再检查更新",
        ),
        (
            429,
            UpdateErrorKind::RateLimited,
            "GitHub 暂时限制了访问频率，请过一会儿再检查更新",
        ),
        (
            404,
            UpdateErrorKind::NoRelease,
            "GitHub 上还没有正式发布的版本",
        ),
        (
            502,
            UpdateErrorKind::Network,
            "检查更新失败：GitHub 返回 HTTP 502，请稍后再试",
        ),
    ];
    for (status, kind, message) in cases {
        let transport = FakeTransport::new();
        transport.enqueue_status(status);
        let error = checker(&transport, "2.1.0").check("").await.unwrap_err();
        assert_eq!((error.kind, error.message.as_str()), (kind, message));
    }

    let transport = FakeTransport::new();
    transport.enqueue_error(TransportError::Network("连接被重置".to_owned()));
    let error = checker(&transport, "2.1.0").check("").await.unwrap_err();
    assert_eq!(error.kind, UpdateErrorKind::Network);
    assert_eq!(
        error.message,
        "检查更新失败：网络请求失败：连接被重置，请检查网络后重试"
    );
}

#[tokio::test]
async fn automatic_checks_stay_offline_when_switched_off() {
    let transport = FakeTransport::new();
    let logs = LogHub::new();

    let result = checker(&transport, "2.1.0")
        .run(&request(false, false, ""), &logs)
        .await;

    assert_eq!(result, Ok(None));
    assert!(transport.requests().is_empty());
    assert!(logs.recent().is_empty());
}

#[tokio::test]
async fn a_manual_check_runs_even_when_automatic_checks_are_off() {
    let transport = FakeTransport::new();
    transport.enqueue_json(&release_json("v2.2"));
    let logs = LogHub::new();

    let check = checker(&transport, "2.1.0")
        .run(&request(true, false, "2.2.0"), &logs)
        .await
        .unwrap()
        .unwrap();

    // 手动检查也会如实报告「已忽略」，由界面决定照样展示。
    assert_eq!(check.status, UpdateStatus::Ignored);
    let entries = logs.recent();
    assert_eq!(entries.len(), 1);
    assert_eq!(
        entries[0].message,
        "GitHub 上的最新版本 2.2.0 已被忽略，有更新的版本时再提醒"
    );
}

#[tokio::test]
async fn each_outcome_leaves_one_log_line() {
    let transport = FakeTransport::new();
    transport.enqueue_json(&release_json("v2.2"));
    transport.enqueue_json(&release_json("v2.1"));
    let logs = LogHub::new();
    let checker = checker(&transport, "2.1.0");

    checker.run(&request(false, true, ""), &logs).await.unwrap();
    checker.run(&request(false, true, ""), &logs).await.unwrap();

    let messages: Vec<String> = logs
        .recent()
        .into_iter()
        .map(|entry| entry.message)
        .collect();
    assert_eq!(
        messages,
        [
            "发现新版本 2.2.0（当前 2.1.0），可以到 GitHub 发布页下载",
            "已是最新版本（2.1.0）",
        ]
    );
}

#[tokio::test]
async fn automatic_failures_are_quiet_but_manual_failures_warn() {
    let transport = FakeTransport::new();
    transport.enqueue_status(503);
    transport.enqueue_status(503);
    let logs = LogHub::new();
    let checker = checker(&transport, "2.1.0");

    let auto = checker.run(&request(false, true, ""), &logs).await;
    let manual = checker.run(&request(true, true, ""), &logs).await;

    assert!(auto.is_err() && manual.is_err());
    let entries = logs.recent();
    assert_eq!(entries[0].level, LogLevel::Info);
    assert_eq!(
        entries[0].message,
        "自动检查更新没有完成：检查更新失败：GitHub 返回 HTTP 503，请稍后再试"
    );
    assert_eq!(entries[1].level, LogLevel::Warning);
    assert_eq!(
        entries[1].message,
        "检查更新失败：GitHub 返回 HTTP 503，请稍后再试"
    );
}

#[test]
fn an_unparsable_current_version_treats_every_release_as_new() {
    let transport = FakeTransport::new();
    assert_eq!(checker(&transport, "dev").current, Version::default());
    assert_eq!(
        classify(&Version::default(), &v("0.0.1"), ""),
        UpdateStatus::Available
    );
}
