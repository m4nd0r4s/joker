/*
 * jdp.h — libjdp, the C SDK for Joker tier X drivers (Joker Driver Protocol, JDP 1).
 *
 * Normative reference: docs/ref/06-drivers.md, §5.8. Wire protocol: spec/cddl/jdp.cddl and
 * ref/06 §5.3–§5.5. This header is the stable ABI of libjdp.so / libjdp.a.
 *
 * Model:
 *  - the driver fills a jdp_driver_t with callbacks and calls jdp_main();
 *  - libjdp speaks JDP with jokerd (handshake, control socket, memfd rings, health, logs);
 *  - each driver instance (a device, or a bus) has its own worker thread; callbacks of one instance
 *    are called one at a time, from that thread, and MAY block until the deadline they receive;
 *  - jdp_push(), jdp_report_link(), jdp_log() and jdp_progress() are thread-safe;
 *  - every pointer passed to a callback is valid only until the callback returns;
 *  - no function of libjdp calls exit() or abort(); fatal protocol errors make jdp_main() return.
 *
 * Versioning (ref/00 §1): JDP_ABI_VERSION_MAJOR changes only with an incompatible ABI change;
 * a minor only adds functions and appends fields at the end of structs whose first field is
 * `size` (the caller sets it to sizeof).
 */
#ifndef JDP_H
#define JDP_H

#include <stdbool.h>
#include <stddef.h>
#include <stdint.h>

#ifdef __cplusplus
extern "C" {
#endif

#define JDP_PROTOCOL_VERSION 1u
#define JDP_ABI_VERSION_MAJOR 1u
#define JDP_ABI_VERSION_MINOR 0u

/* ---- status ------------------------------------------------------------------------------- */

typedef enum jdp_status {
    JDP_OK = 0,
    JDP_E_ERROR = -1,       /* generic failure; see the J-code set with jdp_*_fail */
    JDP_E_INVALID = -2,     /* invalid argument */
    JDP_E_UNSUPPORTED = -3, /* the operation is not implemented by this driver */
    JDP_E_TIMEOUT = -4,     /* deadline reached */
    JDP_E_FULL = -5,        /* ring full: the sample was dropped and counted */
    JDP_E_CLOSED = -6,      /* the instance or the connection to jokerd is gone */
    JDP_E_NOMEM = -7,
    JDP_E_PROTOCOL = -8,    /* protocol violation (J0863) */
    JDP_E_CANCELLED = -9,
    JDP_E_TOO_LARGE = -10   /* value above the limits of ref/06 §2.15 */
} jdp_status_t;

/* ---- shared enums (numeric values = ref/00 §4.3, §6 and ref/06 §2) ------------------------ */

typedef enum jdp_quality_code { JDP_GOOD = 0, JDP_UNCERTAIN = 1, JDP_BAD = 2 } jdp_quality_code_t;

/* Quality reasons a driver may report (ref/06 §2.8). 240–255 are private to the driver. */
enum {
    JDP_R_NONE = 0,
    JDP_R_OVERRIDDEN = 1,
    JDP_R_LOCAL_OVERRIDE = 2,
    JDP_R_STALE = 3,
    JDP_R_SUBSTITUTED = 5,
    JDP_R_SENSOR_CAL = 7,
    JDP_R_CLOCK_UNSYNCED = 8,
    JDP_R_INITIAL = 9,
    JDP_R_COMM_FAILURE = 12,
    JDP_R_DEVICE_FAULT = 13,
    JDP_R_OUT_OF_RANGE = 14,
    JDP_R_CONFIG_ERROR = 15,
    JDP_R_NOT_CONNECTED = 16,
    JDP_R_PRIVATE_FIRST = 240
};

typedef enum jdp_raw_kind {
    JDP_RAW_NONE = 0,
    JDP_RAW_BOOL = 1,
    JDP_RAW_INT = 2,
    JDP_RAW_UINT = 3,
    JDP_RAW_FLOAT = 4,
    JDP_RAW_REGS = 5,
    JDP_RAW_BYTES = 6,
    JDP_RAW_TEXT = 7
} jdp_raw_kind_t;

/* What a driver may report (ref/06 §2.12). The full link_state of ref/00 §6 adds drift = 4 and
 * stopped = 5, which only the core sets; a driver never reports them. */
typedef enum jdp_link_state {
    JDP_LINK_CONNECTING = 0,
    JDP_LINK_UP = 1,
    JDP_LINK_DEGRADED = 2,
    JDP_LINK_DOWN = 3
} jdp_link_state_t;

typedef enum jdp_direction { JDP_DIR_INPUT = 0, JDP_DIR_OUTPUT = 1, JDP_DIR_PARAM = 2 } jdp_direction_t;

typedef enum jdp_batch_kind {
    JDP_BATCH_POLL = 0,
    JDP_BATCH_ON_DEMAND = 1,
    JDP_BATCH_PUSH = 2
} jdp_batch_kind_t;

typedef enum jdp_batch_failure {
    JDP_FAIL_NONE = -1, /* not a failure: the batch succeeded */
    JDP_FAIL_TIMEOUT = 0,
    JDP_FAIL_COMM = 1,
    JDP_FAIL_DEVICE = 2,
    JDP_FAIL_NOT_CONNECTED = 3,
    JDP_FAIL_CONFIG = 4
} jdp_batch_failure_t;

typedef enum jdp_write_kind {
    JDP_WRITE_SET = 0,
    JDP_WRITE_KEEPALIVE = 1,
    JDP_WRITE_FORCE_SAFE = 2,
    JDP_WRITE_RELINQUISH = 3,
    JDP_WRITE_RESYNC = 4
} jdp_write_kind_t;

typedef enum jdp_write_status {
    JDP_WS_OK = 0,
    JDP_WS_TIMEOUT = 1,
    JDP_WS_COMM_FAILURE = 2,
    JDP_WS_DEVICE_REJECTED = 3,
    JDP_WS_NOT_CONNECTED = 4,
    JDP_WS_UNSUPPORTED = 5,
    JDP_WS_LATE = 6,
    JDP_WS_SKIPPED = 7
} jdp_write_status_t;

typedef enum jdp_action_failure {
    JDP_AF_TIMEOUT = 0,
    JDP_AF_DEVICE = 1,
    JDP_AF_INVALID = 2,
    JDP_AF_NOT_CONNECTED = 3,
    JDP_AF_UNSUPPORTED = 4,
    JDP_AF_CANCELLED = 5
} jdp_action_failure_t;

typedef enum jdp_log_level {
    JDP_LOG_TRACE = 0,
    JDP_LOG_DEBUG = 1,
    JDP_LOG_INFO = 2,
    JDP_LOG_WARN = 3,
    JDP_LOG_ERROR = 4
} jdp_log_level_t;

typedef enum jdp_link_kind {
    JDP_LINK_KIND_NONE = 0,
    JDP_LINK_KIND_SERIAL = 1,
    JDP_LINK_KIND_TCP = 2,
    JDP_LINK_KIND_UDP = 3,
    JDP_LINK_KIND_I2C = 4,
    JDP_LINK_KIND_SPI = 5,
    JDP_LINK_KIND_CAN = 6,
    JDP_LINK_KIND_PATHS = 7
} jdp_link_kind_t;

/* ---- plain data ---------------------------------------------------------------------------- */

typedef struct jdp_bytes {
    const uint8_t *ptr;
    size_t len;
} jdp_bytes_t;

typedef struct jdp_str {
    const char *ptr; /* UTF-8, not NUL-terminated */
    size_t len;
} jdp_str_t;

typedef struct jdp_raw_value {
    jdp_raw_kind_t kind;
    union {
        bool b;
        int64_t i;
        uint64_t u;
        double f;
        struct {
            const uint16_t *ptr;
            size_t len; /* number of registers */
        } regs;
        jdp_bytes_t bytes;
        jdp_str_t text;
    } as;
} jdp_raw_value_t;

typedef struct jdp_raw_status {
    uint8_t code;   /* jdp_quality_code_t */
    uint8_t reason; /* JDP_R_* */
} jdp_raw_status_t;

#define JDP_SAMPLE_HAS_TS_SRC 0x1u
#define JDP_SAMPLE_HAS_TS_HW 0x2u

typedef struct jdp_sample {
    uint32_t point;
    uint32_t flags; /* JDP_SAMPLE_HAS_* */
    jdp_raw_value_t value;
    jdp_raw_status_t status;
    int64_t ts_src; /* UTC ns */
    uint64_t ts_hw; /* monotonic ns */
} jdp_sample_t;

typedef struct jdp_point {
    uint32_t id;
    uint16_t device;
    uint16_t width;
    jdp_str_t path;
    jdp_direction_t direction;
    jdp_raw_kind_t expect;
    jdp_str_t type_name;
    jdp_bytes_t addr; /* deterministic CBOR */
    uint8_t poll_kind; /* 0 period, 1 on-demand, 2 push */
    uint64_t period_ns;
    uint8_t read_class; /* 0 cycle, 1 telemetry, 2 diagnostic */
    bool has_coherent;
    uint32_t coherent;
    uint64_t debounce_ns;
    bool timed;
} jdp_point_t;

typedef struct jdp_action_spec {
    uint32_t id;
    uint16_t device;
    jdp_str_t name;
    jdp_bytes_t native;
    uint64_t timeout_ns;
    bool is_long;
} jdp_action_spec_t;

typedef struct jdp_device_spec {
    uint16_t index;
    jdp_str_t name;
    jdp_bytes_t config;
    jdp_bytes_t profile_config;
    jdp_bytes_t quirks;
    bool has_discovered;
    jdp_bytes_t discovered;
    uint64_t timeout_ns;
} jdp_device_spec_t;

typedef struct jdp_plan_request {
    bool has_bus;
    jdp_str_t bus_name;
    uint8_t bus_kind; /* 0 none, 1 ethernet, 2 serial, 3 can, 4 ethercat, 5 i2c, 6 spi */
    jdp_bytes_t bus_params;
    const jdp_device_spec_t *devices;
    size_t n_devices;
    const jdp_point_t *points;
    size_t n_points;
    const jdp_action_spec_t *actions;
    size_t n_actions;
} jdp_plan_request_t;

typedef struct jdp_batch {
    uint32_t id;
    uint16_t device;
    jdp_batch_kind_t kind;
    const uint32_t *points;
    size_t n_points;
    uint64_t period_ns;
    uint64_t cost_ns;
    jdp_bytes_t private_data;
} jdp_batch_t;

typedef struct jdp_plan_view {
    const jdp_batch_t *batches;
    size_t n_batches;
    uint64_t write_cost_ns;
    jdp_bytes_t private_data;
} jdp_plan_view_t;

typedef struct jdp_serial_params {
    uint32_t baud;
    uint8_t data_bits;
    uint8_t parity; /* 0 none, 1 even, 2 odd */
    uint8_t stop_bits;
    uint64_t turnaround_ns;
} jdp_serial_params_t;

typedef struct jdp_link {
    jdp_link_kind_t kind;
    jdp_str_t target; /* device path, "host:port" or interface name */
    jdp_serial_params_t serial;
    const jdp_str_t *allowed; /* UDP networks (CIDR) or granted paths */
    size_t n_allowed;
    const uint16_t *i2c_addrs;
    size_t n_i2c_addrs;
} jdp_link_t;

#define JDP_WRITE_HAS_AT 0x1u
#define JDP_WRITE_ATOMIC 0x2u

typedef struct jdp_write {
    uint64_t seq;
    uint32_t point;
    uint32_t flags; /* JDP_WRITE_HAS_AT, JDP_WRITE_ATOMIC */
    jdp_raw_value_t value;
    jdp_write_kind_t kind;
    uint64_t at_mono_ns;
    uint32_t atomic_group;
    uint16_t atomic_index;
    uint16_t atomic_count;
} jdp_write_t;

typedef struct jdp_device_info {
    size_t size; /* sizeof(jdp_device_info_t) */
    const char *vendor; /* NUL-terminated or NULL */
    const char *model;
    const char *serial;
    const char *firmware;
    const char *hw_revision;
    const uint8_t *cert_sha256; /* 32 bytes or NULL */
    jdp_bytes_t extra;          /* CBOR map, may be empty */
} jdp_device_info_t;

/* ---- opaque handles ------------------------------------------------------------------------ */

typedef struct jdp_inst jdp_inst_t;             /* one driver instance */
typedef struct jdp_plan_builder jdp_plan_builder_t;
typedef struct jdp_sink jdp_sink_t;             /* samples of one read */
typedef struct jdp_acks jdp_acks_t;             /* acks of one write call */
typedef struct jdp_connect_out jdp_connect_out_t;
typedef struct jdp_action_out jdp_action_out_t;
typedef struct jdp_scan_out jdp_scan_out_t;
typedef struct jdp_health_out jdp_health_out_t;

/* ---- callbacks ----------------------------------------------------------------------------- */

typedef struct jdp_driver {
    size_t size;        /* sizeof(jdp_driver_t) */
    uint32_t abi_major; /* JDP_ABI_VERSION_MAJOR */
    uint32_t abi_minor; /* JDP_ABI_VERSION_MINOR */

    /* Descriptor (ref/06 §2.3) as deterministic CBOR (spec/cddl/jdp.cddl `descriptor`). Required. */
    jdp_bytes_t descriptor;

    /* Pure: no I/O, no clock. Same request → same plan. Required. */
    jdp_status_t (*plan)(void *user, const jdp_plan_request_t *req, jdp_plan_builder_t *out);

    /* Creates the instance state. No I/O. Required. Store state with jdp_inst_set_user(). */
    jdp_status_t (*open)(void *user, jdp_inst_t *inst, const jdp_plan_request_t *req,
                         const jdp_plan_view_t *plan, const jdp_link_t *link);

    /* Required. One jdp_connect_info() per device of the instance. */
    jdp_status_t (*connect)(jdp_inst_t *inst, uint64_t deadline, jdp_connect_out_t *out);

    /* Poll and on-demand batches. Required. Return JDP_FAIL_NONE on success. Push batches never
     * reach this callback: push drivers call jdp_push(). */
    jdp_batch_failure_t (*read)(jdp_inst_t *inst, const jdp_batch_t *batch, uint64_t deadline,
                                jdp_sink_t *sink);

    /* Optional (capability "write"). One jdp_ack() per write, in order. */
    void (*write)(jdp_inst_t *inst, const jdp_write_t *writes, size_t n, uint64_t deadline,
                  jdp_acks_t *acks);

    /* Optional (capability "raw-access"). */
    jdp_batch_failure_t (*read_addr)(jdp_inst_t *inst, uint16_t device, jdp_bytes_t addr,
                                     jdp_raw_kind_t expect, uint16_t width, jdp_str_t type_name,
                                     uint64_t deadline, jdp_sink_t *one_value);
    jdp_batch_failure_t (*write_addr)(jdp_inst_t *inst, uint16_t device, jdp_bytes_t addr,
                                      const jdp_raw_value_t *value, jdp_str_t type_name,
                                      uint64_t deadline);

    /* Optional (capability "native-actions"). */
    void (*invoke)(jdp_inst_t *inst, uint64_t call, uint32_t action, jdp_bytes_t args,
                   uint64_t deadline, jdp_action_out_t *out);

    /* Optional (capability "scan"). Read-only. Called on a temporary thread without an instance. */
    jdp_status_t (*scan)(void *user, jdp_str_t uri, jdp_bytes_t options, const jdp_link_t *link,
                         uint64_t deadline, jdp_scan_out_t *out);

    /* Optional. Cheap, no I/O. */
    void (*health)(jdp_inst_t *inst, jdp_health_out_t *out);

    /* Required. Must not fail. */
    void (*disconnect)(jdp_inst_t *inst, uint64_t deadline);

    /* Optional. Frees the instance state. */
    void (*close)(jdp_inst_t *inst);
} jdp_driver_t;

/* ---- entry point --------------------------------------------------------------------------- */

/* Reads JDP_FD or JDP_SOCKET, JDP_TOKEN_FILE and JDP_DRIVER from the environment (ref/06 §5.2),
 * performs the handshake and serves until `bye`. Returns the process exit code:
 * 0 clean bye, 2 usage/environment, 3 version or authentication refused, 4 protocol error. */
int jdp_main(int argc, char **argv, const jdp_driver_t *driver, void *user);

/* ---- instance ------------------------------------------------------------------------------ */

void jdp_inst_set_user(jdp_inst_t *inst, void *state);
void *jdp_inst_user(const jdp_inst_t *inst);
uint16_t jdp_inst_id(const jdp_inst_t *inst);

/* Thread-safe. The effective link is the worst of this and the core's view (ref/06 §2.12). */
void jdp_report_link(jdp_inst_t *inst, jdp_link_state_t state, const char *detail);

/* Thread-safe. Samples of a push batch. JDP_E_FULL: the ring was full, the sample was counted as
 * dropped. JDP_E_INVALID: point not in the batch or raw kind different from `expect`. */
jdp_status_t jdp_push(jdp_inst_t *inst, uint32_t batch, const jdp_sample_t *samples, size_t n);

/* Monotonic clock of the node (CLOCK_MONOTONIC), ns. */
uint64_t jdp_now_mono_ns(void);

/* ---- plan builder -------------------------------------------------------------------------- */

jdp_status_t jdp_plan_add_batch(jdp_plan_builder_t *b, uint16_t device, jdp_batch_kind_t kind,
                                const uint32_t *points, size_t n_points, uint64_t period_ns,
                                uint64_t cost_ns, const uint8_t *private_data, size_t private_len);
void jdp_plan_set_write_cost(jdp_plan_builder_t *b, uint64_t ns);
jdp_status_t jdp_plan_set_private(jdp_plan_builder_t *b, const uint8_t *data, size_t len);
/* `code` is a J-code ("J0810"); `point` < 0 when not about a point. Any error fails the plan. */
void jdp_plan_error(jdp_plan_builder_t *b, const char *code, int64_t point, const char *message);
void jdp_plan_warning(jdp_plan_builder_t *b, const char *code, int64_t point, const char *message);

/* ---- outputs of callbacks ------------------------------------------------------------------ */

/* read: one call per point of the batch. JDP_E_INVALID for a point outside the batch. */
jdp_status_t jdp_sink_push(jdp_sink_t *sink, const jdp_sample_t *sample);
/* read/read_addr: human detail attached to a batch failure (truncated to 40 bytes). */
void jdp_sink_fail_detail(jdp_sink_t *sink, const char *detail);

jdp_status_t jdp_ack(jdp_acks_t *acks, size_t index, jdp_write_status_t status, bool has_ts_hw,
                     uint64_t ts_hw, const char *detail);

jdp_status_t jdp_connect_info(jdp_connect_out_t *out, uint16_t device, const jdp_device_info_t *info,
                              const uint8_t *discovered_cbor, size_t discovered_len);
void jdp_connect_fail(jdp_connect_out_t *out, const char *code, const char *message);

jdp_status_t jdp_action_result(jdp_action_out_t *out, const uint8_t *cbor, size_t len);
void jdp_action_fail(jdp_action_out_t *out, jdp_action_failure_t kind, const char *code,
                     const char *message);
/* Thread-safe; only while the invoke callback of `call` runs. */
void jdp_progress(jdp_inst_t *inst, uint64_t call, uint16_t permille, const char *message);
bool jdp_cancelled(jdp_inst_t *inst, uint64_t call);

jdp_status_t jdp_scan_hit(jdp_scan_out_t *out, const char *address, const uint8_t *config_cbor,
                          size_t config_len, const jdp_device_info_t *info, const char *profile);

void jdp_health_set(jdp_health_out_t *out, jdp_link_state_t link, const char *detail);
jdp_status_t jdp_health_counter(jdp_health_out_t *out, const char *name, uint64_t value);

/* ---- logging ------------------------------------------------------------------------------- */

/* Thread-safe, never blocks; above the rate limit of `welcome` messages are dropped and counted. */
void jdp_log(jdp_log_level_t level, const char *fmt, ...)
#if defined(__GNUC__) || defined(__clang__)
    __attribute__((format(printf, 2, 3)))
#endif
    ;

/* ---- minimal CBOR reader for config, addr, quirks and args --------------------------------- */

/* Look up a text key in a CBOR map; `path` may be dotted ("fallback.timeout") for nested maps.
 * JDP_E_INVALID if the key is missing or has another type. */
jdp_status_t jdp_cbor_get_int(jdp_bytes_t map, const char *path, int64_t *out);
jdp_status_t jdp_cbor_get_uint(jdp_bytes_t map, const char *path, uint64_t *out);
jdp_status_t jdp_cbor_get_float(jdp_bytes_t map, const char *path, double *out);
jdp_status_t jdp_cbor_get_bool(jdp_bytes_t map, const char *path, bool *out);
jdp_status_t jdp_cbor_get_str(jdp_bytes_t map, const char *path, jdp_str_t *out);
jdp_status_t jdp_cbor_get_bytes(jdp_bytes_t map, const char *path, jdp_bytes_t *out);
/* Duration keys arrive as uint ns; quantity keys as float in the unit of the schema. */
bool jdp_cbor_has(jdp_bytes_t map, const char *path);

#ifdef __cplusplus
}
#endif

#endif /* JDP_H */
