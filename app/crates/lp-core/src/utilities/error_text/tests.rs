use super::*;

const DENIED_MESSAGE: &str = r"Access to the path 'D:\音乐\中转\.nmm-1a2b3c.part' is denied.";

fn denied() -> Error {
    Error::new(ErrorKind::PermissionDenied, DENIED_MESSAGE)
}

#[test]
fn describe_write_prefers_target_directory_and_explains_what_to_do() {
    let message = describe_write(&denied(), Some(r"D:\音乐\中转"));

    assert!(message.contains("没有写入权限"));
    assert!(message.contains(r"D:\音乐\中转"));
    assert!(message.contains("管理员身份"));
    assert!(!message.contains("is denied"));
}

#[test]
fn describe_write_falls_back_to_the_path_in_the_message() {
    let message = describe_write(&denied(), None);

    assert!(message.contains(r"D:\音乐\中转\.nmm-1a2b3c.part"));
    assert!(message.contains("没有写入权限"));
}

#[test]
fn describe_write_without_any_path_still_says_what_to_do() {
    let error = Error::from(ErrorKind::PermissionDenied);

    let message = describe_write(&error, Some("  "));

    assert!(message.starts_with("无法写入目标文件夹"));
    assert!(message.contains("管理员身份"));
}

#[test]
fn describe_extracts_path_from_english_denied_message() {
    let message = describe(&denied());

    assert!(message.contains("权限不足"));
    assert!(message.contains(r"D:\音乐\中转\.nmm-1a2b3c.part"));
}

#[test]
fn describe_extracts_path_from_localized_denied_message() {
    let error = Error::other(r"对路径“D:\音乐\新歌.mp3”的访问被拒绝。");

    let message = describe(&error);

    assert!(message.contains("权限不足"));
    assert!(message.contains(r"D:\音乐\新歌.mp3"));
}

#[test]
fn describe_handles_denied_error_without_path() {
    let message = describe(&Error::other("Access is denied."));

    assert!(message.contains("权限不足"));
    assert!(message.contains("管理员身份"));
}

#[test]
fn describe_recognizes_the_raw_windows_permission_code() {
    let error = Error::from_raw_os_error(ACCESS_DENIED);

    assert!(is_permission_error(&error));
    assert!(describe(&error).contains("权限不足"));
}

#[test]
fn describe_reports_disk_full() {
    let error = Error::from_raw_os_error(DISK_FULL);

    assert!(is_disk_full(&error));
    assert!(describe(&error).contains("磁盘空间不足"));
}

#[test]
fn describe_reports_file_in_use() {
    for code in [SHARING_VIOLATION, LOCK_VIOLATION] {
        let error = Error::from_raw_os_error(code);

        assert!(is_file_in_use(&error));
        assert!(describe(&error).contains("正被其他程序占用"));
    }
}

#[test]
fn describe_reports_path_too_long() {
    let error = Error::from_raw_os_error(PATH_TOO_LONG);

    assert!(is_path_too_long(&error));
    assert!(describe(&error).contains("路径过长"));
}

#[test]
fn describe_mentions_the_path_for_disk_full_and_in_use_when_known() {
    let full = Error::other(r"There is not enough space on the disk. 'D:\音乐\a.mp3'");
    let busy = Error::other(
        r"The process cannot access the file 'D:\音乐\a.mp3' because it is being used by another process.",
    );

    assert!(describe(&full).contains(r"D:\音乐\a.mp3"));
    assert!(describe(&busy).contains(r"D:\音乐\a.mp3"));
}

#[test]
fn describe_write_falls_back_to_generic_description_for_other_errors() {
    let error = Error::other("接口返回 500");

    assert_eq!(describe_write(&error, Some(r"D:\音乐")), "接口返回 500");
}

#[test]
fn describe_keeps_unknown_message() {
    assert_eq!(describe(&Error::other("something odd")), "something odd");
}

#[test]
fn is_known_io_error_only_matches_actionable_errors() {
    assert!(is_known_io_error(&denied()));
    assert!(is_known_io_error(&Error::from_raw_os_error(PATH_TOO_LONG)));
    assert!(!is_known_io_error(&Error::other("not io")));
}

#[test]
fn try_get_path_handles_missing_and_unquoted_paths() {
    assert_eq!(try_get_path(None), None);
    assert_eq!(try_get_path(Some("   ")), None);
    assert_eq!(try_get_path(Some("没有路径")), None);
    assert_eq!(
        try_get_path(Some(r"无法访问 D:\音乐\新歌.mp3 请重试")).as_deref(),
        Some(r"D:\音乐\新歌.mp3")
    );
}
