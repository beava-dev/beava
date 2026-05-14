/// Plan 19.2-05 (D-04b): Integration tests for EventTypeMix AHashSet allowlist +
/// Cow non-allocation path.
///
/// TDD RED: these tests reference APIs that don't exist yet:
///   - `state.allowed_set_for_test() -> Option<&AHashSet<String>>`
///   - `str_from_row` returning `Cow<'_, str>` (not `String`)
///   - `value_to_key_string` returning `Cow<'_, str>` (not `String`)
///   - `EventTypeMixState::update_at(extracted, field_idx, event_time_ms, where_matched)`
///
/// Task 1.a commit is the RED commit. Task 1.b + 2.b make them GREEN.
use beava_core::agg_buffer::{str_from_row, EventTypeMixState};
use beava_core::agg_state::value_to_key_string;
use beava_core::row::{Row, Value};

// ─── Test 1: EventTypeMix AHashSet allowlist present at new() ────────────────

#[test]
fn test_event_type_mix_uses_hashset_allowlist() {
    let state = EventTypeMixState::new(
        10,
        Some(vec!["click".into(), "view".into(), "purchase".into()]),
    );
    // This accessor returns the AHashSet — will fail to compile until Task 1.b
    // adds `allowed_set: Option<AHashSet<String>>` + `allowed_set_for_test()`.
    let set = state
        .allowed_set_for_test()
        .expect("allowed_set should be Some after new() with allowed categories");
    assert!(set.contains("click"), "click must be in the allowlist set");
    assert!(set.contains("view"), "view must be in the allowlist set");
    assert!(
        set.contains("purchase"),
        "purchase must be in the allowlist set"
    );
    assert!(
        !set.contains("scroll"),
        "scroll must NOT be in the allowlist set"
    );
    assert_eq!(set.len(), 3, "set must have exactly 3 entries");
}

// ─── Test 2: value_to_key_string returns Cow — borrow on Str, own on I64 ─────

#[test]
fn test_value_to_key_string_returns_cow() {
    use std::borrow::Cow;

    // Value::Str path: must return Cow::Borrowed (no allocation)
    let val_str = Value::Str("hello".into());
    let result = value_to_key_string(&val_str).expect("Str should produce Some");
    match result {
        Cow::Borrowed(s) => assert_eq!(s, "hello", "borrowed str must equal input"),
        Cow::Owned(_) => panic!("Value::Str should produce Cow::Borrowed, not Cow::Owned"),
    }

    // Value::I64 path: must return Cow::Owned (alloc required)
    let val_i64 = Value::I64(42);
    let result_i64 = value_to_key_string(&val_i64).expect("I64 should produce Some");
    match result_i64 {
        Cow::Owned(s) => assert_eq!(s, "42", "owned string must be the stringified number"),
        Cow::Borrowed(_) => panic!("Value::I64 should produce Cow::Owned, not Cow::Borrowed"),
    }

    // Value::Null path: must return None
    assert!(
        value_to_key_string(&Value::Null).is_none(),
        "Null must return None"
    );
}

// ─── Test 3: str_from_row returns Cow — borrow on Str, own on I64 ────────────

#[test]
fn test_str_from_row_returns_cow() {
    use std::borrow::Cow;

    // Str field: Cow::Borrowed
    let row_str = Row::new().with_field("cat", Value::Str("click".into()));
    let result = str_from_row(&row_str, "cat").expect("Str field should produce Some");
    match result {
        Cow::Borrowed(s) => assert_eq!(s, "click"),
        Cow::Owned(_) => panic!("Value::Str should produce Cow::Borrowed"),
    }

    // I64 field: Cow::Owned
    let row_i64 = Row::new().with_field("n", Value::I64(7));
    let result_i64 = str_from_row(&row_i64, "n").expect("I64 field should produce Some");
    match result_i64 {
        Cow::Owned(s) => assert_eq!(s, "7"),
        Cow::Borrowed(_) => panic!("Value::I64 should produce Cow::Owned"),
    }

    // Missing field: None
    let row_empty = Row::new();
    assert!(str_from_row(&row_empty, "missing").is_none());
}

// ─── Test 4: EventTypeMix functional parity (allowed categories) ─────────────

#[test]
fn test_event_type_mix_functional_parity() {
    let mut state = EventTypeMixState::new(10, Some(vec!["click".into(), "view".into()]));

    let row_click = Row::new().with_field("type", Value::Str("click".into()));
    let row_view = Row::new().with_field("type", Value::Str("view".into()));
    let row_scroll = Row::new().with_field("type", Value::Str("scroll".into()));

    // 5 clicks
    for _ in 0..5 {
        state.update(&row_click, Some("type"), true);
    }
    // 3 views
    for _ in 0..3 {
        state.update(&row_view, Some("type"), true);
    }
    // 2 scrolls (not in allowed — rejected into total only)
    for _ in 0..2 {
        state.update(&row_scroll, Some("type"), true);
    }

    // Semantic contract: total=10 (rejected events still increment total),
    // counts={click:5, view:3} only.
    assert_eq!(state.total, 10, "total should include rejected events");
    assert_eq!(
        state.counts.get("click").copied(),
        Some(5),
        "click count must be 5"
    );
    assert_eq!(
        state.counts.get("view").copied(),
        Some(3),
        "view count must be 3"
    );
    assert!(
        !state.counts.contains_key("scroll"),
        "scroll must not be in counts (rejected by allowlist)"
    );
}

// ─── Test 5: update_at consumes pre-extracted Value (no row.get linear scan) ──

#[test]
fn test_event_type_mix_update_at_consumes_extracted_value() {
    let mut state = EventTypeMixState::new(10, None);

    // update_at consumes the dispatcher-resolved pre_val directly — no
    // row.get scan, no re-extraction from extracted[field_idx].
    let val = Value::Str("click".into());
    state.update_at(Some(&val), true);

    // Functional assertion: counts["click"] == 1
    assert_eq!(
        state.counts.get("click").copied(),
        Some(1),
        "update_at must count the event"
    );
    assert_eq!(state.total, 1, "total should be 1");

    // pre_val == None must be a no-op (fieldless invocation, or field not
    // present in this event after the dispatcher's remap).
    state.update_at(None, true);
    assert_eq!(state.total, 1, "pre_val == None must be a no-op");
}

// ─── Test 6: Bloom insert receives Cow::Borrowed &str (no allocation) ─────────

#[test]
fn test_bloom_consumes_cow_no_alloc() {
    use beava_core::agg_state::BloomMemberStateWrap;
    // Plan 19.2-05 (D-04b): verify that BloomFilter::insert receives the &str
    // borrowed DIRECTLY from the Value::Str's CompactString (no intermediate
    // String allocation). Uses a test-only thread-local pointer probe added to
    // bloom.rs in Task 2.b.
    //
    // The probe: `beava_core::sketches::bloom::_last_bloom_insert_ptr()` returns
    // the `as_ptr()` of the last &str passed to BloomFilter::insert. We compare
    // it against the `as_ptr()` of the str slice stored inside the Value::Str —
    // pointer equality proves the borrow was not copied through a new allocation.
    //
    // Task 2.a: this function doesn't exist yet → RED.
    let raw_str = "hello_bloom_long_enough_to_heap_alloc";
    let row = Row::new().with_field("token", Value::Str(raw_str.into()));

    // Capture the pointer of the CompactString INSIDE the row (after the move).
    // row.get returns &Value so we can borrow the underlying str slice.
    let val_ptr = match row.get("token").expect("token must exist") {
        Value::Str(s) => s.as_str().as_ptr() as usize,
        _ => unreachable!(),
    };

    let mut bloom_state = BloomMemberStateWrap::default();
    bloom_state.update(&row, 0, Some("token"), true);

    // _last_bloom_insert_ptr() must equal val_ptr → proves zero-alloc borrow.
    let last_ptr = beava_core::sketches::bloom::_last_bloom_insert_ptr();
    assert_eq!(
        last_ptr, val_ptr,
        "BloomFilter::insert must receive a &str borrowed from the Value::Str CompactString, \
         not a newly allocated String (ptr mismatch proves an intermediate allocation occurred)"
    );
}
