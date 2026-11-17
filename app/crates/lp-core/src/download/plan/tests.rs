use super::*;

fn local(song_id: i64, availability: &'static str) -> LocalMatchView {
    LocalMatchView {
        song_id,
        availability,
        method: "none",
        confidence: 0.0,
        path: None,
        file_name: None,
        format: None,
        local_title: None,
        local_artist: None,
    }
}

#[test]
fn unique_rows_keeps_first_occurrence_in_order() {
    assert_eq!(unique_rows([5, 3, 5, 9, 3, 1]), vec![0, 1, 3, 5]);
    assert!(unique_rows(Vec::<i64>::new()).is_empty());
}

#[test]
fn plan_counts_each_availability() {
    let ids = [1, 2, 3, 4];
    let matches = [
        local(1, "available"),
        local(2, "ncm"),
        local(3, "missing"),
        local(4, "missing"),
    ];
    let plan = plan_transfer(&ids, Some(&matches));
    assert_eq!(plan.total, 4);
    assert_eq!(plan.available, 1);
    assert_eq!(plan.ncm, 1);
    assert_eq!(plan.missing_rows, vec![2, 3]);
}

#[test]
fn plan_uses_first_occurrence_of_duplicates() {
    let ids = [7, 8, 7];
    // 第 3 行重复，即使它的对比结果是「已有」也不算数。
    let matches = [
        local(7, "missing"),
        local(8, "available"),
        local(7, "available"),
    ];
    let plan = plan_transfer(&ids, Some(&matches));
    assert_eq!(plan.total, 2);
    assert_eq!(plan.available, 1);
    assert_eq!(plan.ncm, 0);
    assert_eq!(plan.missing_rows, vec![0]);
}

#[test]
fn plan_without_matches_counts_everything_missing() {
    let plan = plan_transfer(&[1, 2, 1], None);
    assert_eq!(plan.total, 2);
    assert_eq!(plan.available, 0);
    assert_eq!(plan.ncm, 0);
    assert_eq!(plan.missing_rows, vec![0, 1]);
}

#[test]
fn plan_treats_rows_beyond_matches_as_missing() {
    let matches = [local(1, "available")];
    let plan = plan_transfer(&[1, 2, 3], Some(&matches));
    assert_eq!(plan.total, 3);
    assert_eq!(plan.available, 1);
    assert_eq!(plan.missing_rows, vec![1, 2]);
}

#[test]
fn plan_treats_unknown_availability_as_missing() {
    let matches = [local(1, "weird"), local(2, "")];
    let plan = plan_transfer(&[1, 2], Some(&matches));
    assert_eq!(plan.available, 0);
    assert_eq!(plan.ncm, 0);
    assert_eq!(plan.missing_rows, vec![0, 1]);
}

#[test]
fn plan_of_empty_playlist_is_empty() {
    assert_eq!(plan_transfer(&[], Some(&[])), TransferPlan::default());
}

#[test]
fn plan_serializes_in_camel_case() {
    let json = serde_json::to_value(plan_transfer(&[1], None)).unwrap();
    assert_eq!(json["missingRows"], serde_json::json!([0]));
}
