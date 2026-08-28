#include <stdio.h>
#include <stdint.h>
#include <stdlib.h>

#include "accreta_ffi.h"

static void check(int32_t status, const char *operation)
{
    if (status == ACCRETA_STATUS_OK)
        return;

    fprintf(stderr,
            "%s failed: status=%d: %s\n",
            operation,
            status,
            accreta_last_error_message());

    exit(EXIT_FAILURE);
}

static AccretaMeasureValue make_f64(double value)
{
    AccretaMeasureValue result;
    result.tag = ACCRETA_MEASURE_TYPE_F64;
    result.value.f64 = value;
    return result;
}

static AccretaMeasureValue make_u64(uint64_t value)
{
    AccretaMeasureValue result;
    result.tag = ACCRETA_MEASURE_TYPE_U64;
    result.value.u64 = value;
    return result;
}

int main(void)
{
    /* Build a schema with host/region dimensions and two measures. */
    AccretaSchemaBuilder *builder = accreta_schema_builder_new();
    if (builder == NULL) {
        fprintf(stderr, "failed to create schema builder\n");
        return EXIT_FAILURE;
    }

    check(accreta_schema_builder_add_dimension(builder, "host"),
          "add host dimension");
    check(accreta_schema_builder_add_dimension(builder, "region"),
          "add region dimension");

    /* Kind list order doesn't matter, but it can't contain duplicates
     * (accreta_schema_builder_add_measure panics -> ACCRETA_STATUS_PANIC on that). */
    const AccretaAggregateKind cpu_aggregates[] = {
        ACCRETA_AGGREGATE_KIND_SUM,
        ACCRETA_AGGREGATE_KIND_COUNT,
        ACCRETA_AGGREGATE_KIND_MIN,
        ACCRETA_AGGREGATE_KIND_MAX,
        ACCRETA_AGGREGATE_KIND_AVERAGE
    };

    /* measure id 0 = "cpu" (registration order determines the id you pass to
     * accreta_engine_query_range / accreta_engine_ingest's measures array). */
    check(accreta_schema_builder_add_measure(
              builder, "cpu", ACCRETA_MEASURE_TYPE_F64,
              cpu_aggregates,
              sizeof(cpu_aggregates) / sizeof(cpu_aggregates[0])),
          "add cpu measure");

    const AccretaAggregateKind request_aggregates[] = {
        ACCRETA_AGGREGATE_KIND_SUM,
        ACCRETA_AGGREGATE_KIND_COUNT
    };

    /* measure id 1 = "requests" */
    check(accreta_schema_builder_add_measure(
              builder, "requests", ACCRETA_MEASURE_TYPE_U64,
              request_aggregates,
              sizeof(request_aggregates) / sizeof(request_aggregates[0])),
          "add requests measure");

    AccretaSchema *schema = NULL;
    check(accreta_schema_builder_build(builder, &schema),
          "build schema");

    printf("dimensions: %zu\n", accreta_schema_dimension_count(schema));
    printf("measures:   %zu\n", accreta_schema_measure_count(schema));

    /* accreta_engine_new clones the schema internally, so it's fine to free our
     * handle right after — the engine doesn't borrow it. */
    AccretaEngine *engine = accreta_engine_new(schema);
    if (engine == NULL) {
        fprintf(stderr, "failed to create engine\n");
        accreta_schema_free(schema);
        return EXIT_FAILURE;
    }
    accreta_schema_free(schema);

    /*
     * Fixed Unix timestamp in milliseconds.
     * Dimension order: 0 = host, 1 = region.
     * Measure order:   0 = cpu,  1 = requests.
     */
    const int64_t t0 = 1787652000000LL;

    {
        AccretaMeasureValue measures[] = { make_f64(20.0), make_u64(100) };
        const char *dimensions[] = { "server-01", "eu-west" };

        check(accreta_engine_ingest(engine, t0, measures, 2, dimensions, 2),
              "ingest sample 1");
    }

    {
        AccretaMeasureValue measures[] = { make_f64(40.0), make_u64(200) };
        const char *dimensions[] = { "server-01", "eu-west" };

        check(accreta_engine_ingest(engine, t0 + 30 * 1000, measures, 2, dimensions, 2),
              "ingest sample 2");
    }

    {
        AccretaMeasureValue measures[] = { make_f64(60.0), make_u64(300) };
        const char *dimensions[] = { "server-02", "eu-west" };

        check(accreta_engine_ingest(engine, t0 + 45 * 1000, measures, 2, dimensions, 2),
              "ingest sample 3");
    }

    accreta_engine_rollup(engine);

    /* Query CPU (measure id 0) for the hour, ungrouped. */
    AccretaAggregateSet *result = NULL;

    check(accreta_engine_query_range(
              engine,
              ACCRETA_BUCKET_LEVEL_HOUR,
              t0,
              t0 + 60 * 60 * 1000LL,
              0,
              &result),
          "query cpu");

    AccretaMeasureValue value;

    check(accreta_aggregate_set_get_value(
              result, ACCRETA_AGGREGATE_KIND_SUM, ACCRETA_MEASURE_TYPE_F64, &value),
          "get cpu sum");
    printf("CPU sum: %.2f\n", value.value.f64);

    /* Count is always u64, regardless of the measure's own type. */
    check(accreta_aggregate_set_get_value(
              result, ACCRETA_AGGREGATE_KIND_COUNT, ACCRETA_MEASURE_TYPE_F64, &value),
          "get cpu count");
    printf("CPU count: %llu\n", (unsigned long long)value.value.u64);

    /* Average is always f64, regardless of the measure's own type. */
    check(accreta_aggregate_set_get_value(
              result, ACCRETA_AGGREGATE_KIND_AVERAGE, ACCRETA_MEASURE_TYPE_F64, &value),
          "get cpu average");
    printf("CPU average: %.2f\n", value.value.f64);

    accreta_aggregate_set_free(result);

    /* Group CPU by host. Dimension 0 is host, so use bit 0. */
    AccretaGroupedQueryCursor *cursor = NULL;

    check(accreta_engine_query_range_grouped(
              engine,
              ACCRETA_BUCKET_LEVEL_HOUR,
              t0,
              t0 + 60 * 60 * 1000LL,
              0,
              1ULL << 0,
              &cursor),
          "group cpu by host");

    for (;;) {
        AccretaDimensionKey *key = NULL;
        AccretaAggregateSet *group = NULL;

        if (!accreta_grouped_query_cursor_next(cursor, &key, &group))
            break;

        printf("group:");

        uintptr_t key_len = accreta_dimension_key_len(key);
        for (uintptr_t i = 0; i < key_len; ++i) {
            uint32_t value_id = 0;
            check(accreta_dimension_key_get(key, i, &value_id), "get dimension value");
            printf(" %u", (unsigned)value_id);
        }

        check(accreta_aggregate_set_get_value(
                  group, ACCRETA_AGGREGATE_KIND_SUM, ACCRETA_MEASURE_TYPE_F64, &value),
              "get grouped cpu sum");
        printf(" -> cpu_sum=%.2f\n", value.value.f64);

        accreta_dimension_key_free(key);
        accreta_aggregate_set_free(group);
    }

    accreta_grouped_query_cursor_free(cursor);
    accreta_engine_free(engine);

    return EXIT_SUCCESS;
}
