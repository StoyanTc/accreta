use std::ffi::CString;
use std::os::raw::c_char;
use std::ptr;

use accreta_ffi::*;

fn cstr(s: &str) -> CString {
    CString::new(s).unwrap()
}

#[test]
fn end_to_end() {
    fun_name();
}

fn fun_name() {
    let builder = accreta_schema_builder_new();
    assert!(!builder.is_null());
    let dim = cstr("host");
    assert_eq!(
        accreta_schema_builder_add_dimension(builder, dim.as_ptr()),
        AccretaStatus::Ok
    );
    let measure_name = cstr("value");
    let kinds = [
        AccretaAggregateKind::Sum,
        AccretaAggregateKind::Count,
        AccretaAggregateKind::Min,
        AccretaAggregateKind::Max,
        AccretaAggregateKind::Average,
    ];
    let status = accreta_schema_builder_add_measure(
        builder,
        measure_name.as_ptr(),
        AccretaMeasureType::F64,
        kinds.as_ptr(),
        kinds.len(),
    );
    assert_eq!(
        status,
        AccretaStatus::Ok,
        "add_measure failed: {:?}",
        last_err()
    );
    let mut schema: *mut AccretaSchema = ptr::null_mut();
    let status = accreta_schema_builder_build(builder, &mut schema);
    assert_eq!(status, AccretaStatus::Ok, "build failed: {:?}", last_err());
    assert!(!schema.is_null());
    assert_eq!(accreta_schema_dimension_count(schema), 1);
    assert_eq!(accreta_schema_measure_count(schema), 1);
    let mut measure_type = AccretaMeasureType::I64;
    assert_eq!(
        accreta_schema_measure_type(schema, 0, &mut measure_type),
        AccretaStatus::Ok
    );
    assert!(matches!(measure_type, AccretaMeasureType::F64));
    let engine = accreta_engine_new(schema);
    assert!(!engine.is_null());
    // t0 = 2026-03-15T10:05:00Z in ms.
    let t0: i64 = 1773568500000;
    // computed below and verified via chrono in this test
    let host_a = cstr("server-a");
    let dims: [*const c_char; 1] = [host_a.as_ptr()];
    let v1 = AccretaMeasureValue {
        tag: AccretaMeasureType::F64,
        value: AccretaMeasureValueData { f64: 12.0 },
    };
    let v2 = AccretaMeasureValue {
        tag: AccretaMeasureType::F64,
        value: AccretaMeasureValueData { f64: 8.0 },
    };
    let status = accreta_engine_ingest(engine, t0, &v1 as *const _, 1, dims.as_ptr(), 1);
    assert_eq!(status, 0, "ingest 1 failed: {:?}", last_err());
    let status = accreta_engine_ingest(engine, t0 + 60_000, &v2 as *const _, 1, dims.as_ptr(), 1);
    assert_eq!(status, 0, "ingest 2 failed: {:?}", last_err());
    accreta_engine_rollup(engine);
    // Query the whole day range at Hour level, ungrouped.
    let mut set: *mut AccretaAggregateSet = ptr::null_mut();
    let status = accreta_engine_query_range(
        engine,
        AccretaBucketLevel::Hour,
        t0 - 3_600_000,
        t0 + 3_600_000,
        0,
        &mut set,
    );
    assert_eq!(status, 0, "query_range failed: {:?}", last_err());
    assert!(!set.is_null());
    let mut sum_val = AccretaMeasureValue {
        tag: AccretaMeasureType::F64,
        value: AccretaMeasureValueData { f64: 0.0 },
    };
    let status = accreta_aggregate_set_get_value(
        set,
        AccretaAggregateKind::Sum,
        AccretaMeasureType::F64,
        &mut sum_val,
    );
    assert_eq!(
        status,
        AccretaStatus::Ok,
        "get sum failed: {:?}",
        last_err()
    );
    assert_eq!(unsafe { sum_val.value.f64 }, 20.0);
    let mut count_val = AccretaMeasureValue {
        tag: AccretaMeasureType::F64,
        value: AccretaMeasureValueData { f64: 0.0 },
    };
    let status = accreta_aggregate_set_get_value(
        set,
        AccretaAggregateKind::Count,
        AccretaMeasureType::F64,
        &mut count_val,
    );
    assert_eq!(
        status,
        AccretaStatus::Ok,
        "get count failed: {:?}",
        last_err()
    );
    assert_eq!(unsafe { count_val.value.u64 }, 2);
    let mut avg_val = AccretaMeasureValue {
        tag: AccretaMeasureType::F64,
        value: AccretaMeasureValueData { f64: 0.0 },
    };
    let status = accreta_aggregate_set_get_value(
        set,
        AccretaAggregateKind::Average,
        AccretaMeasureType::F64,
        &mut avg_val,
    );
    assert_eq!(
        status,
        AccretaStatus::Ok,
        "get average failed: {:?}",
        last_err()
    );
    assert_eq!(unsafe { avg_val.value.f64 }, 10.0);
    accreta_aggregate_set_free(set);
    // Bucket + group cursor round trip.
    let mut bucket: *mut AccretaBucket = ptr::null_mut();
    let hour_start = t0 - (t0 % 3_600_000);
    let status = accreta_engine_bucket(engine, AccretaBucketLevel::Hour, hour_start, &mut bucket);
    assert_eq!(status, 0, "engine_bucket failed: {:?}", last_err());
    assert_eq!(accreta_bucket_group_count(bucket), 1);
    let cursor = accreta_bucket_groups_cursor(bucket);
    assert!(!cursor.is_null());
    let mut key: *mut AccretaDimensionKey = ptr::null_mut();
    let mut sets: *mut AccretaAggregateSetList = ptr::null_mut();
    let has_next = accreta_group_cursor_next(cursor, &mut key, &mut sets);
    assert!(has_next);
    assert_eq!(accreta_dimension_key_len(key), 1);
    let mut dim_value: u32 = 0;
    assert_eq!(
        accreta_dimension_key_get(key, 0, &mut dim_value),
        AccretaStatus::Ok
    );
    assert_eq!(accreta_aggregate_set_list_len(sets), 1);
    let set0 = accreta_aggregate_set_list_get(sets, 0);
    assert!(!set0.is_null());
    let mut v = AccretaMeasureValue {
        tag: AccretaMeasureType::F64,
        value: AccretaMeasureValueData { f64: 0.0 },
    };
    assert_eq!(
        accreta_aggregate_set_get_value(
            set0,
            AccretaAggregateKind::Max,
            AccretaMeasureType::F64,
            &mut v
        ),
        AccretaStatus::Ok
    );
    assert_eq!(unsafe { v.value.f64 }, 12.0);
    accreta_dimension_key_free(key);
    accreta_aggregate_set_list_free(sets);
    let has_next2 = accreta_group_cursor_next(cursor, &mut key, &mut sets);
    assert!(!has_next2);
    accreta_group_cursor_free(cursor);
    accreta_bucket_free(bucket);
    // grouped query round trip
    let mut grouped_cursor: *mut AccretaGroupedQueryCursor = ptr::null_mut();
    let status = accreta_engine_query_range_grouped(
        engine,
        AccretaBucketLevel::Hour,
        t0 - 3_600_000,
        t0 + 3_600_000,
        0,
        1, // bit 0 => the "host" dimension
        &mut grouped_cursor,
    );
    assert_eq!(status, 0, "query_range_grouped failed: {:?}", last_err());
    let mut gkey: *mut AccretaDimensionKey = ptr::null_mut();
    let mut gset: *mut AccretaAggregateSet = ptr::null_mut();
    assert!(accreta_grouped_query_cursor_next(
        grouped_cursor,
        &mut gkey,
        &mut gset
    ));
    accreta_dimension_key_free(gkey);
    accreta_aggregate_set_free(gset);
    assert!(!accreta_grouped_query_cursor_next(
        grouped_cursor,
        &mut gkey,
        &mut gset
    ));
    accreta_grouped_query_cursor_free(grouped_cursor);
    // retention + prune smoke test
    let retention = accreta_retention_new();
    assert_eq!(
        accreta_retention_keep(retention, AccretaBucketLevel::Minute, 0),
        0
    );
    accreta_retention_free(retention);
    accreta_engine_prune(engine);
    // error path: null pointer
    let status = accreta_schema_builder_add_dimension(ptr::null_mut(), dim.as_ptr());
    assert_eq!(status, AccretaStatus::NullPointer);
    assert!(last_err().is_some());
    accreta_engine_free(engine);
    accreta_schema_free(schema);
}

fn last_err() -> Option<String> {
    unsafe {
        let ptr = accreta_last_error_message();
        if ptr.is_null() {
            None
        } else {
            Some(std::ffi::CStr::from_ptr(ptr).to_string_lossy().into_owned())
        }
    }
}
