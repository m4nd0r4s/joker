/*
 * joker_node.h -- C ABI of the Joker Micro (library mode).
 *
 * Normative reference: docs/ref/08-joker-micro-abi.md (this header is the
 * machine-readable form of section 2 of that document; if they diverge, the
 * document wins and this header is a bug).
 *
 * This header is FIXED: it is the same for every project and changes only
 * with JK_ABI_VERSION_MINOR (additions) or JK_ABI_VERSION_MAJOR (breaks).
 * Everything that depends on a project (JK_SIG_..., JK_GROUP_..., the size
 * of jk_node_t, JK_GENERATION) lives in the GENERATED header
 * "joker_project.h", which includes this one. Application code includes
 * "joker_project.h".
 *
 * Rules (ref/08 section 2.4):
 *   - no allocation, no threads, no hidden global state;
 *   - reentrant per jk_node_t; each function documents its call context:
 *       [any]      any context, including ISRs, concurrently with anything;
 *       [group]    the context that ticks that group (one per group);
 *       [service]  the single service context of the node;
 *       [excl]     exclusive: no [group]/[service] call on the same node at the
 *                  same time; concurrent [any] calls return JK_E_STATE;
 *       [pure]     no node state; any task context (not ISR: stack);
 *   - all times are microseconds (ref/00 section 3): monotonic uint64_t since
 *     MCU boot, UTC int64_t since 1970-01-01T00:00:00Z;
 *   - all values are in the declared unit of the signal (ref/00 section 4.1);
 *   - JK_OK is 0, errors are negative, informative results are positive;
 *   - synchronisation is only critical-section + portable-atomic (rule 8): the
 *     integrator links a critical-section implementation (its RTOS or HAL), or
 *     builds the library with the cs-single-core feature.
 *
 * Checked with: gcc -fsyntax-only -std=c11 -Wall -Wextra -pedantic
 */
#ifndef JOKER_NODE_H
#define JOKER_NODE_H

#include <stdbool.h>
#include <stddef.h>
#include <stdint.h>

#ifdef __cplusplus
#define JK_NORETURN [[noreturn]]
extern "C" {
#else
#define JK_NORETURN _Noreturn
#endif

/* ------------------------------------------------------------------------- */
/* 1. Version                                                                */
/* ------------------------------------------------------------------------- */

#define JK_ABI_VERSION_MAJOR 1
#define JK_ABI_VERSION_MINOR 0
/** Packed version: major in bits 16..31, minor in bits 0..15. */
#define JK_ABI_VERSION \
    ((uint32_t)(((uint32_t)JK_ABI_VERSION_MAJOR << 16) | (uint32_t)JK_ABI_VERSION_MINOR))
#define JK_ABI_MAJOR_OF(v) ((uint16_t)(((uint32_t)(v)) >> 16))
#define JK_ABI_MINOR_OF(v) ((uint16_t)(((uint32_t)(v)) & 0xFFFFu))

/* ------------------------------------------------------------------------- */
/* 2. Status codes (ref/08 section 2.7). Each error maps to a J-E25xx code.   */
/* ------------------------------------------------------------------------- */

typedef int32_t jk_status_t;

/* informative (>= 0) */
#define JK_OK                  ((jk_status_t)0)
#define JK_EMPTY               ((jk_status_t)1)   /* nothing (more) to return */
#define JK_PENDING             ((jk_status_t)2)   /* not decided yet */
#define JK_RELINQUISHED        ((jk_status_t)3)   /* output relinquished: no value to apply */

/* errors (< 0) */
#define JK_E_ARG               ((jk_status_t)-1)  /* J-E2501 */
#define JK_E_STATE             ((jk_status_t)-2)  /* J-E2502 */
#define JK_E_ABI               ((jk_status_t)-3)  /* J-E2503 */
#define JK_E_ALIGN             ((jk_status_t)-4)  /* J-E2504 */
#define JK_E_UNSUPPORTED       ((jk_status_t)-5)  /* J-E2505 */
#define JK_E_REENTRANT         ((jk_status_t)-6)  /* J-E2506 */
#define JK_E_HALTED            ((jk_status_t)-7)  /* J-E2507 */
#define JK_E_GEN_FORMAT        ((jk_status_t)-10) /* J-E2510 */
#define JK_E_GEN_VERSION       ((jk_status_t)-11) /* J-E2511 */
#define JK_E_GEN_CAPACITY      ((jk_status_t)-12) /* J-E2512 */
#define JK_E_GEN_SIGNATURE     ((jk_status_t)-13) /* J-E2513 */
#define JK_E_GEN_TRUST         ((jk_status_t)-14) /* J-E2514 */
#define JK_E_GEN_ROLLBACK      ((jk_status_t)-15) /* J-E2515 */
#define JK_E_GEN_FIRMWARE      ((jk_status_t)-16) /* J-E2516 */
#define JK_E_GEN_NODE          ((jk_status_t)-17) /* J-E2517 */
#define JK_E_GEN_INTEGRITY     ((jk_status_t)-18) /* J-E2518 */
#define JK_E_SIGNAL            ((jk_status_t)-20) /* J-E2520 */
#define JK_E_DIRECTION         ((jk_status_t)-21) /* J-E2521 */
#define JK_E_TYPE              ((jk_status_t)-22) /* J-E2522 */
#define JK_E_GROUP             ((jk_status_t)-23) /* J-E2523 */
#define JK_E_TIME              ((jk_status_t)-24) /* J-E2524 */
#define JK_E_FULL              ((jk_status_t)-25) /* J-E2525 */
#define JK_E_NOT_FOUND         ((jk_status_t)-26) /* J-E2526 */
#define JK_E_ARGS              ((jk_status_t)-27) /* J-E2527 */
#define JK_E_VALUE             ((jk_status_t)-28) /* J-E2528 */
#define JK_E_QUALITY           ((jk_status_t)-29) /* J-E2529 */
#define JK_E_PARAM             ((jk_status_t)-30) /* J-E2530 */
#define JK_E_TTL               ((jk_status_t)-31) /* J-E2531 */
#define JK_E_LEVEL             ((jk_status_t)-32) /* J-E2532 */
#define JK_E_BUSY              ((jk_status_t)-33) /* J-E2533 */
#define JK_E_PERSIST           ((jk_status_t)-40) /* J-E2540 */
#define JK_E_PERSIST_CORRUPT   ((jk_status_t)-41) /* J-E2541 */
#define JK_E_PERSIST_FULL      ((jk_status_t)-42) /* J-E2542 */
#define JK_E_LINK              ((jk_status_t)-50) /* J-E2550 */
#define JK_E_LINK_AUTH         ((jk_status_t)-51) /* J-E2551 */
#define JK_E_LINK_VERSION      ((jk_status_t)-52) /* J-E2552 */
#define JK_E_LINK_DOWN         ((jk_status_t)-53) /* J-E2553 */
#define JK_E_OTA               ((jk_status_t)-60) /* J-E2560 */
#define JK_E_OTA_VERIFY        ((jk_status_t)-61) /* J-E2561 */
#define JK_E_INTERNAL          ((jk_status_t)-99) /* J-E2599 */

#define JK_IS_ERROR(st) ((st) < 0)

/* ------------------------------------------------------------------------- */
/* 3. Identifiers. All are indices of the EXTERNAL INTERFACE of the build     */
/*    (stable for the life of the firmware), not generation SignalIds.        */
/* ------------------------------------------------------------------------- */

typedef uint32_t jk_sig_t;      /* JK_SIG_<PATH>                      */
typedef uint32_t jk_group_t;    /* JK_GROUP_<NAME>                    */
typedef uint32_t jk_writer_t;   /* JK_WRITER_<NAME>                   */
typedef uint32_t jk_inst_t;     /* JK_INST_<NAME>                     */
typedef uint16_t jk_member_t;   /* JK_C_<PKG>_<CLASS>_<MAJOR>_<MEMBER> */
typedef uint32_t jk_alarm_t;    /* JK_ALARM_<NAME>                    */
typedef uint32_t jk_param_t;    /* JK_PARAM_<APP>_<NAME>              */
typedef uint32_t jk_ticket_t;   /* action invocation ticket           */
typedef uint8_t  jk_link_id_t;  /* JK_LINK_<NAME>                     */

#define JK_NONE ((uint32_t)0xFFFFFFFFu)  /* "no identifier" sentinel */

/* ------------------------------------------------------------------------- */
/* 4. Shared enumerations (ref/00 sections 4.3, 4.4, 6)                       */
/* ------------------------------------------------------------------------- */

/* signal types (ref/00 section 6) */
typedef uint8_t jk_type_t;
#define JK_TYPE_BOOL   ((jk_type_t)0)
#define JK_TYPE_I64    ((jk_type_t)1)
#define JK_TYPE_U64    ((jk_type_t)2)
#define JK_TYPE_F64    ((jk_type_t)3)
#define JK_TYPE_F32    ((jk_type_t)4)
#define JK_TYPE_ENUM   ((jk_type_t)5)
#define JK_TYPE_BITS   ((jk_type_t)6)
#define JK_TYPE_STRING ((jk_type_t)7)
#define JK_TYPE_BYTES  ((jk_type_t)8)
/* 9..15 reserved; 16 = block (v1.1), 17 = stream (v1.1) */

/* quality: code in bits 0..7, reason in bits 8..15, flags in bits 16..31 */
typedef uint32_t jk_quality_t;

#define JK_QC_GOOD      0u
#define JK_QC_UNCERTAIN 1u
#define JK_QC_BAD       2u

#define JK_QR_NONE                 0u
#define JK_QR_OVERRIDDEN           1u
#define JK_QR_LOCAL_OVERRIDE       2u
#define JK_QR_STALE                3u
#define JK_QR_NOT_CONFIRMED        4u
#define JK_QR_SUBSTITUTED          5u
#define JK_QR_OUT_OF_DISTRIBUTION  6u
#define JK_QR_SENSOR_CAL           7u
#define JK_QR_CLOCK_UNSYNCED       8u
#define JK_QR_INITIAL              9u
#define JK_QR_FROZEN              10u
#define JK_QR_PARTIAL_WRITE       11u
#define JK_QR_COMM_FAILURE        12u
#define JK_QR_DEVICE_FAULT        13u
#define JK_QR_OUT_OF_RANGE        14u
#define JK_QR_CONFIG_ERROR        15u
#define JK_QR_NOT_CONNECTED       16u
#define JK_QR_NAN                 17u
#define JK_QR_TRIPPED             18u
#define JK_QR_DEADLINE            19u
/* 20..239 reserved; 240..255 private to the integrator (seen as 0 outside) */
#define JK_QR_PRIVATE_FIRST      240u

/* quality flags (bits 16..31); all others reserved, must be 0 on input */
#define JK_QF_TS_SRC_MONO  (1u << 16)  /* ts_src is local monotonic, not UTC */

#define JK_Q(code, reason) \
    ((jk_quality_t)(((uint32_t)(code) & 0xFFu) | (((uint32_t)(reason) & 0xFFu) << 8)))
#define JK_GOOD            JK_Q(JK_QC_GOOD, JK_QR_NONE)
#define JK_UNCERTAIN(r)    JK_Q(JK_QC_UNCERTAIN, (r))
#define JK_BAD(r)          JK_Q(JK_QC_BAD, (r))
#define JK_Q_CODE(q)       ((uint8_t)(((uint32_t)(q)) & 0xFFu))
#define JK_Q_REASON(q)     ((uint8_t)((((uint32_t)(q)) >> 8) & 0xFFu))
#define JK_Q_FLAGS(q)      ((uint32_t)(q) & 0xFFFF0000u)
#define JK_Q_IS_GOOD(q)    (JK_Q_CODE(q) == JK_QC_GOOD)

/* write outcomes (ref/00 section 4.4) */
typedef uint8_t jk_outcome_t;
#define JK_OUTCOME_NONE          ((jk_outcome_t)0)
#define JK_OUTCOME_ACCEPTED      ((jk_outcome_t)1)
#define JK_OUTCOME_CLAMPED_RANGE ((jk_outcome_t)2)
#define JK_OUTCOME_CLAMPED_RATE  ((jk_outcome_t)3)
#define JK_OUTCOME_SUPPRESSED    ((jk_outcome_t)4)
#define JK_OUTCOME_OVERRIDDEN    ((jk_outcome_t)5)
#define JK_OUTCOME_DEFERRED      ((jk_outcome_t)6)
#define JK_OUTCOME_SHADOWED      ((jk_outcome_t)7)
#define JK_OUTCOME_REJECTED      ((jk_outcome_t)8)
#define JK_OUTCOME_TRIPPED       ((jk_outcome_t)9)

typedef uint8_t jk_reject_t;
#define JK_REJECT_NONE           ((jk_reject_t)0)
#define JK_REJECT_WRONG_TYPE     ((jk_reject_t)1)
#define JK_REJECT_INTERLOCK      ((jk_reject_t)2)
#define JK_REJECT_QUALITY        ((jk_reject_t)3)
#define JK_REJECT_RANGE          ((jk_reject_t)4)
#define JK_REJECT_ATOMIC_GROUP   ((jk_reject_t)5)
#define JK_REJECT_NOT_OWNER      ((jk_reject_t)6)
#define JK_REJECT_CLOCK_UNSYNCED ((jk_reject_t)7)
#define JK_REJECT_BUDGET         ((jk_reject_t)8)

typedef uint8_t jk_clock_q_t;
#define JK_CLOCK_SYNCED   ((jk_clock_q_t)0)
#define JK_CLOCK_HOLDOVER ((jk_clock_q_t)1)
#define JK_CLOCK_UNSYNCED ((jk_clock_q_t)2)

typedef uint8_t jk_alarm_state_t;
#define JK_ALARM_NORMAL         ((jk_alarm_state_t)0)
#define JK_ALARM_ACTIVE_UNACKED ((jk_alarm_state_t)1)
#define JK_ALARM_ACTIVE_ACKED   ((jk_alarm_state_t)2)
#define JK_ALARM_RTN_UNACKED    ((jk_alarm_state_t)3)
#define JK_ALARM_UNKNOWN        ((jk_alarm_state_t)4)

typedef uint8_t jk_severity_t;
#define JK_SEVERITY_LOW      ((jk_severity_t)0)
#define JK_SEVERITY_MEDIUM   ((jk_severity_t)1)
#define JK_SEVERITY_HIGH     ((jk_severity_t)2)
#define JK_SEVERITY_CRITICAL ((jk_severity_t)3)

typedef uint8_t jk_role_t;
#define JK_ROLE_VIEWER   ((jk_role_t)0)
#define JK_ROLE_OPERATOR ((jk_role_t)1)
#define JK_ROLE_ADMIN    ((jk_role_t)2)

/* arbiter levels are absolute, 1 (highest) .. 16 (lowest); 0 = no level */
typedef uint8_t jk_level_t;
#define JK_LEVEL_NONE ((jk_level_t)0)

/* time sentinels */
#define JK_TS_SRC_NONE ((int64_t)INT64_MIN)    /* no ts_src   */
#define JK_TS_HW_NONE  ((uint64_t)UINT64_MAX)  /* no ts_hw    */

/* ------------------------------------------------------------------------- */
/* 5. Value, sample, output                                                  */
/* ------------------------------------------------------------------------- */

/** Tagged value. f32 values travel promoted in as.f (exact). string/bytes are
 *  borrowed (ptr, len <= 64) and only valid for the duration of the call. */
typedef struct jk_value {
    jk_type_t type;
    uint8_t   jk_reserved0;
    uint16_t  jk_reserved1;
    uint32_t  jk_reserved2;
    union {
        bool     b;
        int64_t  i;      /* i64, enum (signed variant value) */
        uint64_t u;      /* u64, bits, enum (unsigned variant value) */
        double   f;      /* f64, f32 */
        struct {
            const uint8_t *ptr;
            uint32_t       len;
        } buf;           /* string (UTF-8, no NUL needed), bytes */
    } as;
} jk_value_t;

/** Input sample with all timestamps (jk_set_input_ex). */
typedef struct jk_sample_in {
    jk_value_t   value;
    jk_quality_t quality;
    uint32_t     jk_reserved;
    uint64_t     ts_acq_us;   /* monotonic receive instant (required)   */
    uint64_t     ts_hw_us;    /* monotonic hardware capture, or JK_TS_HW_NONE */
    int64_t      ts_src_us;   /* UTC from the remote device, or JK_TS_SRC_NONE */
} jk_sample_in_t;

/** Sample as read (jk_read). For string/bytes, value.as.buf points to the
 *  caller buffer given to jk_read_buf; with jk_read it has len 0. */
typedef struct jk_sample {
    jk_value_t   value;
    jk_quality_t quality;     /* staleness (max_age) already applied */
    uint32_t     jk_reserved;
    uint64_t     seq;
    uint64_t     ts_acq_us;
    uint64_t     ts_hw_us;
    int64_t      ts_src_us;
} jk_sample_t;

/* jk_output_t.flags */
#define JK_OUTF_CHANGED      (1u << 0)  /* applied value/state changed since the last jk_take_changed */
#define JK_OUTF_SAFE         (1u << 1)  /* no active level: value is the safe value (A6) */
#define JK_OUTF_RELINQUISH   (1u << 2)  /* safe = "relinquish" is in force: give control back */
#define JK_OUTF_TRIPPED      (1u << 3)  /* output in trip (E7) */
#define JK_OUTF_FORCED       (1u << 4)  /* force_safe active (supervisor or jk_force_safe) */
#define JK_OUTF_UNCONFIRMED  (1u << 5)  /* verify/readback failed: Uncertain(not_confirmed) */
#define JK_OUTF_PULSE        (1u << 6)  /* pulse in progress */
#define JK_OUTF_MANUAL       (1u << 7)  /* the active level is a manual writer */

/** Committed state of an output: what the integrator must apply. */
typedef struct jk_output {
    jk_value_t   value;       /* last applied value; never meaningless unless RELINQUISH */
    jk_quality_t quality;
    jk_outcome_t outcome;     /* outcome of the effective write in the last tick */
    jk_reject_t  reject;
    jk_level_t   level;       /* active level, JK_LEVEL_NONE if none */
    uint8_t      jk_reserved0;
    uint32_t     flags;       /* JK_OUTF_* */
    uint32_t     jk_reserved1;
    uint64_t     seq;         /* commit sequence of the owner group */
} jk_output_t;

/** Outcome of the last proposal of one writer on one output. */
typedef struct jk_outcome_info {
    jk_outcome_t outcome;
    jk_reject_t  reject;
    jk_level_t   level;       /* for OVERRIDDEN: the active (winning) level */
    uint8_t      jk_reserved0;
    uint32_t     jk_reserved1;
    jk_value_t   applied;     /* applied value (CLAMPED_*: the clamped value) */
    uint64_t     tick_seq;    /* tick of the owner group that decided it */
} jk_outcome_info_t;

/* ------------------------------------------------------------------------- */
/* 6. Node, generation, platform                                             */
/* ------------------------------------------------------------------------- */

/** The node. Incomplete here; "joker_project.h" completes it with
 *  JK_PROJECT_NODE_WORDS 8-byte words. Allocate statically. */
typedef struct jk_node jk_node_t;

/** A generation ready to use. Fields are private (filled by
 *  jk_generation_open or by the build for JK_GENERATION). */
typedef struct jk_generation {
    const void *jk_image;
    uint32_t    jk_len;
    uint32_t    jk_flags;
    uint64_t    jk_reserved[2];
} jk_generation_t;

typedef struct jk_generation_info {
    uint64_t gen_seq;
    uint8_t  source_hash[32];     /* the "generation hash" (ref/00 section 5) */
    uint8_t  registry_hash[32];   /* doc 16 section 7 */
    uint8_t  image_hash[32];      /* SHA-256 of the image bytes */
    uint32_t image_len;
    uint32_t format;              /* blob format version */
    uint32_t min_runtime;         /* packed major<<16 | minor */
    uint32_t flags;               /* JK_GENF_* */
} jk_generation_info_t;

#define JK_GENF_SIGNED    (1u << 0)
#define JK_GENF_DEV       (1u << 1)
#define JK_GENF_EMBEDDED  (1u << 2)
#define JK_GENF_VERIFIED  (1u << 3)
#define JK_GENF_NODE_BOUND (1u << 4)

/* flash areas handled through the platform callbacks */
typedef uint8_t jk_area_t;
#define JK_AREA_PERSIST     ((jk_area_t)0)  /* trips, kv, alarms, params, generation state */
#define JK_AREA_GEN_A       ((jk_area_t)1)  /* generation slot A */
#define JK_AREA_GEN_B       ((jk_area_t)2)  /* generation slot B */
#define JK_AREA_FW_STAGING  ((jk_area_t)3)  /* firmware image for the bootloader */
#define JK_AREA_IDENTITY    ((jk_area_t)4)  /* node name, link keys, trust bundle, KEM key */
#define JK_AREA_COUNT       5u

typedef struct jk_area_info {
    uint32_t size;          /* bytes; 0 = area not present */
    uint32_t erase_size;    /* erase granularity (power of two) */
    uint32_t write_size;    /* program granularity (1, 4, 8, 16, 256...) */
    uint8_t  erased_value;  /* 0xFF for NOR flash */
    uint8_t  jk_reserved[3];
} jk_area_info_t;

/* jk_platform_t.notify events (service context) */
typedef uint8_t jk_notify_t;
#define JK_NOTIFY_GEN_READY     ((jk_notify_t)1) /* a verified generation is waiting for jk_generation_swap */
#define JK_NOTIFY_GEN_ROLLBACK  ((jk_notify_t)2) /* confirm window expired: rolled back */
#define JK_NOTIFY_GEN_CONFIRMED ((jk_notify_t)3)
#define JK_NOTIFY_FW_READY      ((jk_notify_t)4) /* verified firmware in JK_AREA_FW_STAGING */
#define JK_NOTIFY_TIME          ((jk_notify_t)5) /* clock quality changed; arg = jk_clock_q_t */
#define JK_NOTIFY_LINK          ((jk_notify_t)6) /* link state changed; arg = link id */
#define JK_NOTIFY_TRACE_FROZEN  ((jk_notify_t)7) /* trace ring frozen (trip or request) */
#define JK_NOTIFY_RESTART       ((jk_notify_t)8) /* the peer asked for a node restart */

/** Platform callbacks. All optional (NULL). Called only from [excl]
 *  functions, the service context and jk_generation_open; may block. The
 *  structure must stay valid while the node exists. */
typedef struct jk_platform {
    void *user;

    jk_status_t (*area_info)(void *user, jk_area_t area, jk_area_info_t *info);
    jk_status_t (*area_read)(void *user, jk_area_t area, uint32_t offset,
                             void *buf, uint32_t len);
    jk_status_t (*area_write)(void *user, jk_area_t area, uint32_t offset,
                              const void *data, uint32_t len);
    jk_status_t (*area_erase)(void *user, jk_area_t area, uint32_t offset, uint32_t len);
    /** Memory-mapped (XIP) address of an area, or NULL. */
    const void *(*area_map)(void *user, jk_area_t area);

    /** Link key (32 B) from a secure element; NULL: keys from JK_AREA_IDENTITY. */
    jk_status_t (*link_key)(void *user, jk_link_id_t link, uint8_t key_out[32]);

    void (*notify)(void *user, jk_notify_t what, uint32_t arg);

    /** Trust anchors: Ed25519 root public keys that sign trust bundles. Without
     *  a bundle in JK_AREA_IDENTITY (record 18) they act for every role
     *  (config for generation blobs, firmware for images). ref/08 2.13.5. */
    const uint8_t (*trust_keys)[32];
    uint32_t trust_count;

    /** X25519 private key for secrets (autonomous MCUs) from a secure element;
     *  NULL: the key comes from JK_AREA_IDENTITY (record 19). */
    jk_status_t (*kem_key)(void *user, uint8_t key_out[32]);
} jk_platform_t;

/* ------------------------------------------------------------------------- */
/* 7. Symbols the INTEGRATOR must define                                     */
/* ------------------------------------------------------------------------- */

/** Monotonic microseconds since MCU boot. Non-decreasing, callable from any
 *  context (including ISR), wait-free. Used by the library only in
 *  jk_link_poll (receive timestamps, link timers) and at the end of jk_tick
 *  (statistics and overrun policy). Never in the decisions of a tick. */
uint64_t jk_now_us(void);

/** Called on a broken internal invariant or a panic of statically linked
 *  app code. Must not return (reset the MCU). Outputs must fall to their
 *  hardware reset state, which the board declares safe (J0786). */
JK_NORETURN void jk_fatal(jk_status_t code, uint32_t detail);

/* ------------------------------------------------------------------------- */
/* 8. Utilities [pure]                                                       */
/* ------------------------------------------------------------------------- */

/** Version of the linked library (JK_ABI_VERSION packing). [any] */
uint32_t jk_abi_version(void);
/** Registry code of a status ("J-E2512"); "ok" for >= 0. [any] */
const char *jk_status_str(jk_status_t st);

/* ------------------------------------------------------------------------- */
/* 9. Generation                                                             */
/* ------------------------------------------------------------------------- */

/** Validate a blob (format, integrity, signature, trust, rollback, node
 *  binding, firmware compatibility, capacities, bytecode) and fill *gen.
 *  The blob must be 8-aligned and stay mapped and unchanged while any node
 *  uses it. May call platform->area_read(JK_AREA_IDENTITY). [pure] */
jk_status_t jk_generation_open(jk_generation_t *gen, const void *blob, uint32_t len,
                               const jk_platform_t *platform, uint64_t min_seq);
/** Pick and open the boot generation from JK_AREA_GEN_A/B and the
 *  generation state in JK_AREA_PERSIST (active area; the other one if the
 *  active was still confirming or does not validate). Needs area_map.
 *  JK_E_NOT_FOUND if none validates. [pure] */
jk_status_t jk_generation_boot(jk_generation_t *gen, const jk_platform_t *platform);
/** [pure] */
jk_status_t jk_generation_info(const jk_generation_t *gen, jk_generation_info_t *info);

/* ------------------------------------------------------------------------- */
/* 10. Lifecycle                                                             */
/* ------------------------------------------------------------------------- */

/** Initialise. Use the jk_init() macro, which checks the ABI version, the
 *  size of jk_node_t and the interface hash of "joker_project.h". [excl] */
jk_status_t jk_init_checked(jk_node_t *node, size_t node_size, uint32_t abi_version,
                            uint64_t iface_hash, const jk_generation_t *gen,
                            const jk_platform_t *platform, uint64_t now_us);

#define jk_init(node, gen, platform, now_us)                                   \
    jk_init_checked((node), sizeof(*(node)), JK_ABI_VERSION,                   \
                    JK_PROJECT_IFACE_HASH, (gen), (platform), (now_us))

/** Warm restart with the same generation: outputs to safe, leases dropped,
 *  persisted state (trips, alarms, params, kv) reloaded. [excl] */
jk_status_t jk_restart(jk_node_t *node, uint64_t now_us);

/** Orderly stop: outputs forced to safe, persistence flushed (blocking).
 *  Afterwards only jk_get_output*, jk_safe_get, jk_init are valid. [excl] */
jk_status_t jk_shutdown(jk_node_t *node);

/** Switch to another generation at a tick boundary, keeping applied values,
 *  leases and trips of identical outputs (doc 16 A8). next = NULL takes the
 *  generation received by OTA. confirm_ms = 0: no confirm window. [excl] */
jk_status_t jk_generation_swap(jk_node_t *node, const jk_generation_t *next,
                               uint32_t confirm_ms, uint64_t now_us);
/** Confirm the generation in its confirm window (ADR 0015). [service] */
jk_status_t jk_generation_confirm(jk_node_t *node);

/* ------------------------------------------------------------------------- */
/* 11. Tick and groups                                                       */
/* ------------------------------------------------------------------------- */

/** One tick of a group: latch, async proposals, expressions, app tasks,
 *  methods, arbiter, envelope, commit, trace, stats (doc 01 section 4).
 *  now_us is the latch instant; must not go back for the group. [group] */
jk_status_t jk_tick(jk_node_t *node, jk_group_t group, uint64_t now_us);

/** Period declared for the group in the current generation; 0 if unknown. [any] */
uint32_t jk_group_period_us(const jk_node_t *node, jk_group_t group);

typedef struct jk_group_stats {
    uint64_t ticks;
    uint32_t overruns;
    uint32_t skipped;
    uint32_t last_duration_us;
    uint32_t max_duration_us;
    uint32_t max_lateness_us;
    uint32_t latch_collisions;   /* reads that fell back to the previous sample */
    uint32_t inbox_dropped;      /* proposals/method records lost (queue full) */
    uint32_t jk_reserved;
} jk_group_stats_t;

/** [any] */
jk_status_t jk_group_stats(const jk_node_t *node, jk_group_t group, jk_group_stats_t *out);

/* ------------------------------------------------------------------------- */
/* 12. Inputs [any; one writer context per signal]                           */
/* ------------------------------------------------------------------------- */

jk_status_t jk_set_input_bool(jk_node_t *node, jk_sig_t sig, bool v, jk_quality_t q, uint64_t ts_acq_us);
jk_status_t jk_set_input_i64 (jk_node_t *node, jk_sig_t sig, int64_t v, jk_quality_t q, uint64_t ts_acq_us);
jk_status_t jk_set_input_u64 (jk_node_t *node, jk_sig_t sig, uint64_t v, jk_quality_t q, uint64_t ts_acq_us);
jk_status_t jk_set_input_f64 (jk_node_t *node, jk_sig_t sig, double v, jk_quality_t q, uint64_t ts_acq_us);
jk_status_t jk_set_input_f32 (jk_node_t *node, jk_sig_t sig, float v, jk_quality_t q, uint64_t ts_acq_us);
jk_status_t jk_set_input_buf (jk_node_t *node, jk_sig_t sig, const void *data, uint32_t len,
                              jk_quality_t q, uint64_t ts_acq_us);   /* string / bytes */
jk_status_t jk_set_input_ex  (jk_node_t *node, jk_sig_t sig, const jk_sample_in_t *s);

/** Type-generic setter (C11 _Generic): integer types -> i64/u64, float, double, bool.
 *  Not available in C++ (use the typed functions). */
#ifndef __cplusplus
#define jk_set_input(node, sig, v, q, ts)                  \
    _Generic((v),                                          \
        _Bool:              jk_set_input_bool,             \
        float:              jk_set_input_f32,              \
        double:             jk_set_input_f64,              \
        signed char:        jk_set_input_i64,              \
        short:              jk_set_input_i64,              \
        int:                jk_set_input_i64,              \
        long:               jk_set_input_i64,              \
        long long:          jk_set_input_i64,              \
        unsigned char:      jk_set_input_u64,              \
        unsigned short:     jk_set_input_u64,              \
        unsigned int:       jk_set_input_u64,              \
        unsigned long:      jk_set_input_u64,              \
        unsigned long long: jk_set_input_u64)((node), (sig), (v), (q), (ts))
#endif

/* ------------------------------------------------------------------------- */
/* 13. Reading signals [any; wait-free]                                      */
/* ------------------------------------------------------------------------- */

/** Last value of any interface signal, with staleness computed at now_us. */
jk_status_t jk_read(const jk_node_t *node, jk_sig_t sig, uint64_t now_us, jk_sample_t *out);
/** Same, copying string/bytes into buf (cap >= 64 always suffices). */
jk_status_t jk_read_buf(const jk_node_t *node, jk_sig_t sig, uint64_t now_us,
                        jk_sample_t *out, void *buf, uint32_t cap);

/* ------------------------------------------------------------------------- */
/* 14. Outputs [any; wait-free]                                              */
/* ------------------------------------------------------------------------- */

/* Return JK_OK (value written), JK_RELINQUISHED (value untouched) or an error. */
jk_status_t jk_get_output_bool(const jk_node_t *node, jk_sig_t sig, bool *v, jk_quality_t *q);
jk_status_t jk_get_output_i64 (const jk_node_t *node, jk_sig_t sig, int64_t *v, jk_quality_t *q);
jk_status_t jk_get_output_u64 (const jk_node_t *node, jk_sig_t sig, uint64_t *v, jk_quality_t *q);
jk_status_t jk_get_output_f64 (const jk_node_t *node, jk_sig_t sig, double *v, jk_quality_t *q);
jk_status_t jk_get_output_f32 (const jk_node_t *node, jk_sig_t sig, float *v, jk_quality_t *q);
jk_status_t jk_get_output_ex  (const jk_node_t *node, jk_sig_t sig, jk_output_t *out);

#ifndef __cplusplus
#define jk_get_output(node, sig, pv, pq)                   \
    _Generic((pv),                                         \
        _Bool *:    jk_get_output_bool,                    \
        int64_t *:  jk_get_output_i64,                     \
        uint64_t *: jk_get_output_u64,                     \
        double *:   jk_get_output_f64,                     \
        float *:    jk_get_output_f32)((node), (sig), (pv), (pq))
#endif

/** Next extern output owned by group whose committed state changed since the
 *  previous call; JK_EMPTY when none. One consumer per group. [any] */
jk_status_t jk_take_changed(jk_node_t *node, jk_group_t group, jk_sig_t *sig);

/** Outputs of a group (to drive them to safe after jk_supervise). [any] */
uint32_t jk_group_output_count(const jk_node_t *node, jk_group_t group);
jk_sig_t jk_group_output_at(const jk_node_t *node, jk_group_t group, uint32_t index);

/* ------------------------------------------------------------------------- */
/* 15. Writes from the firmware: extern writers and manual [service]         */
/* ------------------------------------------------------------------------- */

jk_status_t jk_propose(jk_node_t *node, jk_writer_t writer, jk_sig_t sig,
                       const jk_value_t *v, uint64_t now_us);
jk_status_t jk_propose_bool(jk_node_t *node, jk_writer_t w, jk_sig_t sig, bool v, uint64_t now_us);
jk_status_t jk_propose_i64 (jk_node_t *node, jk_writer_t w, jk_sig_t sig, int64_t v, uint64_t now_us);
jk_status_t jk_propose_u64 (jk_node_t *node, jk_writer_t w, jk_sig_t sig, uint64_t v, uint64_t now_us);
jk_status_t jk_propose_f64 (jk_node_t *node, jk_writer_t w, jk_sig_t sig, double v, uint64_t now_us);
jk_status_t jk_relinquish(jk_node_t *node, jk_writer_t writer, jk_sig_t sig, uint64_t now_us);
/** JK_OK, or JK_PENDING until the owner group has ticked. [any] */
jk_status_t jk_last_outcome(const jk_node_t *node, jk_writer_t writer, jk_sig_t sig,
                            jk_outcome_info_t *out);

/** Manual write from a local HMI (doc 16 A10: TTL mandatory, <= max_manual).
 *  level must be in the manual band (3..5) and allowed for the output. */
jk_status_t jk_manual_write(jk_node_t *node, jk_sig_t sig, const jk_value_t *v,
                            jk_level_t level, uint32_t ttl_ms, uint32_t user_id,
                            uint64_t now_us);
jk_status_t jk_manual_release(jk_node_t *node, jk_sig_t sig, jk_level_t level, uint64_t now_us);

/* ------------------------------------------------------------------------- */
/* 16. Classes: methods, actions, events [service]                           */
/* ------------------------------------------------------------------------- */

#define JK_METHOD_MAX_ARGS 8u

/** Queue a method record for the owner group(s) of its outs (doc 17 section 4.1). */
jk_status_t jk_call_method(jk_node_t *node, jk_writer_t writer, jk_inst_t inst,
                           jk_member_t method, const jk_value_t *args, uint8_t nargs,
                           uint64_t now_us);
/** Same, with arguments as a deterministic CBOR array (records, lists). */
jk_status_t jk_call_method_cbor(jk_node_t *node, jk_writer_t writer, jk_inst_t inst,
                                jk_member_t method, const uint8_t *cbor, uint32_t len,
                                uint64_t now_us);

typedef uint8_t jk_action_err_t;   /* ActionErrorKind subset (ref/04 4.7.1): J-E0301/0303/0304/0305/0308 */
#define JK_ACTION_OK        ((jk_action_err_t)0)
#define JK_ACTION_DENIED    ((jk_action_err_t)1)
#define JK_ACTION_TIMEOUT   ((jk_action_err_t)2)
#define JK_ACTION_DEVICE    ((jk_action_err_t)3)
#define JK_ACTION_INVALID   ((jk_action_err_t)4)
#define JK_ACTION_CANCELLED ((jk_action_err_t)5)

/** Invoke an action (caller side). key: idempotency key, 1..64 printable ASCII. */
jk_status_t jk_action_invoke(jk_node_t *node, jk_writer_t caller, jk_inst_t inst,
                             jk_member_t action, const uint8_t *args_cbor, uint32_t args_len,
                             const char *key, uint32_t key_len, uint64_t now_us,
                             jk_ticket_t *ticket);
/** JK_PENDING, or JK_OK with *err and the result (CBOR on success, UTF-8
 *  message otherwise) in buf. The ticket is released on JK_OK. */
jk_status_t jk_action_result(jk_node_t *node, jk_ticket_t ticket, jk_action_err_t *err,
                             uint8_t *buf, uint32_t cap, uint32_t *len);
jk_status_t jk_action_cancel(jk_node_t *node, jk_ticket_t ticket);

/** Action request for an action implemented by the firmware (serving side). */
typedef struct jk_action_req {
    uint32_t    id;           /* pass to jk_action_complete */
    jk_inst_t   instance;
    jk_member_t action;
    uint8_t     flags;        /* JK_ACTREQF_* */
    jk_role_t   role;         /* role of the caller */
    uint32_t    args_len;
    uint32_t    jk_reserved;
    uint64_t    deadline_us;  /* monotonic; after it the library answers TIMEOUT */
} jk_action_req_t;

#define JK_ACTREQF_LONG      (1u << 0)  /* @long: progress and cancel allowed */
#define JK_ACTREQF_CANCELLED (1u << 1)  /* set on re-poll of a cancelled long action */

jk_status_t jk_action_next(jk_node_t *node, jk_action_req_t *req, uint8_t *args, uint32_t cap);
jk_status_t jk_action_progress(jk_node_t *node, uint32_t id, uint8_t percent);
jk_status_t jk_action_is_cancelled(const jk_node_t *node, uint32_t id);  /* JK_OK = yes, JK_EMPTY = no */
jk_status_t jk_action_complete(jk_node_t *node, uint32_t id, jk_action_err_t err,
                               const uint8_t *result, uint32_t len);

/** Emit a class event of an instance implemented by the firmware (CBOR record). */
jk_status_t jk_event_emit(jk_node_t *node, jk_inst_t inst, jk_member_t event,
                          const uint8_t *cbor, uint32_t len, uint64_t now_us);

typedef uint8_t jk_event_kind_t;
#define JK_EV_CLASS  ((jk_event_kind_t)1)  /* event member of an instance */
#define JK_EV_ALARM  ((jk_event_kind_t)2)  /* alarm transition */
#define JK_EV_APP    ((jk_event_kind_t)3)  /* event published by an app (EventOutput) */

typedef struct jk_event {
    jk_event_kind_t  kind;
    uint8_t          flags;       /* JK_EVF_* */
    jk_member_t      member;      /* JK_EV_CLASS: event member */
    uint32_t         source;      /* instance, alarm or app-port interface index */
    uint64_t         seq;
    uint64_t         ts_us;       /* monotonic */
    uint32_t         len;         /* payload bytes written to buf (CBOR) */
    uint32_t         dropped;     /* events lost before this one */
    jk_alarm_state_t alarm_from;
    jk_alarm_state_t alarm_to;
    jk_severity_t    severity;
    uint8_t          jk_reserved[5];
} jk_event_t;

#define JK_EVF_UNCERTAIN (1u << 0)  /* alarm evaluated with Uncertain inputs */
#define JK_EVF_TRUNCATED (1u << 1)  /* payload larger than cap: not copied */

/** Next event of the firmware consumer queue; JK_EMPTY when none. */
jk_status_t jk_event_next(jk_node_t *node, jk_event_t *ev, uint8_t *buf, uint32_t cap);

/* ------------------------------------------------------------------------- */
/* 17. Alarms and trips                                                      */
/* ------------------------------------------------------------------------- */

typedef struct jk_alarm_info {
    jk_alarm_state_t state;
    jk_severity_t    severity;
    uint8_t          shelved;
    uint8_t          suppressed;
    uint32_t         jk_reserved;
    uint64_t         since_us;      /* monotonic instant of the last transition */
    uint64_t         shelved_until_us;
} jk_alarm_info_t;

jk_status_t jk_alarm_info(const jk_node_t *node, jk_alarm_t alarm, jk_alarm_info_t *out); /* [any] */
jk_status_t jk_alarm_ack(jk_node_t *node, jk_alarm_t alarm, uint32_t user_id, uint64_t now_us);
jk_status_t jk_alarm_shelve(jk_node_t *node, jk_alarm_t alarm, uint32_t ttl_ms, uint32_t user_id,
                            const char *reason, uint32_t reason_len, uint64_t now_us);
jk_status_t jk_alarm_unshelve(jk_node_t *node, jk_alarm_t alarm, uint32_t user_id, uint64_t now_us);
/** Reset a trip (doc 16 E7), as a local operator. [service] */
jk_status_t jk_trip_reset(jk_node_t *node, jk_sig_t sig, uint32_t user_id, uint64_t now_us);

/* ------------------------------------------------------------------------- */
/* 18. Parameters                                                            */
/* ------------------------------------------------------------------------- */

#define JK_PARAMF_PERSIST (1u << 0)   /* keep across restarts (JK_AREA_PERSIST) */

jk_status_t jk_param_get(const jk_node_t *node, jk_param_t param, jk_value_t *out);  /* [any] */
jk_status_t jk_param_set(jk_node_t *node, jk_param_t param, const jk_value_t *v,
                         uint32_t flags, uint64_t now_us);                           /* [service] */
jk_status_t jk_param_clear(jk_node_t *node, jk_param_t param, uint64_t now_us);     /* [service] */

/* ------------------------------------------------------------------------- */
/* 19. Time                                                                  */
/* ------------------------------------------------------------------------- */

/** Set the UTC correspondence (only if the generation's clock source is
 *  "extern"; with source "link" the TIME message does it). [service] */
jk_status_t jk_time_set_utc(jk_node_t *node, uint64_t mono_us, int64_t utc_us, jk_clock_q_t q);
/** UTC of a monotonic instant, with the clock quality. [any] */
jk_status_t jk_time_utc(const jk_node_t *node, uint64_t mono_us, int64_t *utc_us, jk_clock_q_t *q);

/* ------------------------------------------------------------------------- */
/* 20. Joker Link: transport integration only (messages: ref/07) [service]    */
/* ------------------------------------------------------------------------- */

/** Non-blocking receive. Stream links (UART, RS-485, USB CDC, TCP): any
 *  number of bytes; the library does the framing. Datagram links (UDP, CAN
 *  with ISO-TP done by the integrator): exactly one message per call.
 *  Returns bytes written to buf (0 = nothing now) or < 0 (transport down). */
typedef int32_t (*jk_link_rx_fn)(void *ctx, uint8_t *buf, uint32_t cap);
/** Non-blocking send. Stream: returns bytes accepted (may be partial).
 *  Datagram: len or 0 (try later). < 0: transport down. */
typedef int32_t (*jk_link_tx_fn)(void *ctx, const uint8_t *buf, uint32_t len);

/** Link 0 with ctx = platform->user; runs one jk_service step (now = jk_now_us()). */
jk_status_t jk_link_poll(jk_node_t *node, jk_link_rx_fn rx, jk_link_tx_fn tx);
/** Any link, explicit ctx; runs one jk_service step. */
jk_status_t jk_link_poll_at(jk_node_t *node, jk_link_id_t link, jk_link_rx_fn rx,
                            jk_link_tx_fn tx, void *ctx);

typedef uint8_t jk_link_state_t;
#define JK_LINK_DOWN             ((jk_link_state_t)0)
#define JK_LINK_HANDSHAKE        ((jk_link_state_t)1)
#define JK_LINK_UP               ((jk_link_state_t)2)
#define JK_LINK_AUTH_FAILED      ((jk_link_state_t)3)
#define JK_LINK_VERSION_MISMATCH ((jk_link_state_t)4)

/* session mode chosen by the peer in HELLO (ref/00 section 6 session_mode, ref/07 7.3) */
typedef uint8_t jk_session_mode_t;
#define JK_SESSION_FULL        ((jk_session_mode_t)0)
#define JK_SESSION_OBSERVE     ((jk_session_mode_t)1)
#define JK_SESSION_MAINTENANCE ((jk_session_mode_t)2)
#define JK_SESSION_REFUSED     ((jk_session_mode_t)3)

typedef struct jk_link_status {
    jk_link_state_t   state;
    uint8_t           version_major;   /* negotiated link_version */
    uint8_t           version_minor;
    uint8_t           authenticated;
    jk_session_mode_t session_mode;    /* valid in JK_LINK_UP */
    uint8_t           jk_reserved0[3];
    jk_status_t       last_error;
    uint32_t          rx_frames;
    uint32_t          tx_frames;
    uint32_t          crc_errors;      /* L1: bad CRC-32C (J-E2001) */
    uint32_t          auth_errors;     /* L2: bad Poly1305 tag (J-E2101) */
    uint32_t          replay_drops;    /* L2: replayed or outside the window (J-E2102) */
    uint32_t          rx_errors;       /* other framing and message errors (J-E2002..J-E2008) */
    uint32_t          retransmits;     /* reliable messages sent again */
    uint32_t          tx_dropped;
    uint64_t          last_rx_us;
} jk_link_status_t;

jk_status_t jk_link_status(const jk_node_t *node, jk_link_id_t link, jk_link_status_t *out); /* [any] */
jk_status_t jk_link_reset(jk_node_t *node, jk_link_id_t link);                               /* [service] */

/* ------------------------------------------------------------------------- */
/* 21. Service domain [service]                                              */
/* ------------------------------------------------------------------------- */

/** One step of the async domain: app async tasks, persistence, timers
 *  (leases of manual writes, confirm window, action timeouts), OTA steps. */
jk_status_t jk_service(jk_node_t *node, uint64_t now_us);
/** Monotonic instant by which jk_service should run again (UINT64_MAX: none). [any] */
uint64_t jk_service_next_us(const jk_node_t *node);
/** Write all dirty persistent records now (blocking). */
jk_status_t jk_persist_flush(jk_node_t *node);

/* ------------------------------------------------------------------------- */
/* 22. Safe state and watchdog [any]                                         */
/* ------------------------------------------------------------------------- */

typedef struct jk_supervision {
    uint32_t late_groups;     /* bit g: group g missed its tick by > 1 period */
    uint32_t forced_groups;   /* bit g: > 3 periods: force_safe set on its outputs */
    uint8_t  feed_watchdog;   /* 1: healthy, feed the hardware watchdog */
    uint8_t  halted;
    uint8_t  forced_all;      /* jk_force_safe active */
    uint8_t  service_late;    /* jk_service not run within service_deadline */
    uint32_t jk_reserved;
} jk_supervision_t;

/** Check groups against their deadlines and set force_safe; wait-free,
 *  O(groups); meant for a timer ISR or a high-priority task. */
jk_status_t jk_supervise(jk_node_t *node, uint64_t now_us, jk_supervision_t *out);
/** Force every output to safe (volatile; cleared with on = false). */
jk_status_t jk_force_safe(jk_node_t *node, bool on);
/** Safe value of an output (immutable generation data): JK_OK or JK_RELINQUISHED. */
jk_status_t jk_safe_get(const jk_node_t *node, jk_sig_t sig, jk_value_t *out);

/* ------------------------------------------------------------------------- */
/* 23. Trace (doc 16 section 7)                                              */
/* ------------------------------------------------------------------------- */

jk_status_t jk_trace_freeze(jk_node_t *node, bool freeze);                    /* [any] */
jk_status_t jk_trace_size(const jk_node_t *node, uint32_t *bytes);            /* [service] */
/** Read the frozen ring as a .jktrace byte stream (uncompressed). [service] */
jk_status_t jk_trace_read(const jk_node_t *node, uint32_t offset, uint8_t *buf,
                          uint32_t cap, uint32_t *len);

/* ------------------------------------------------------------------------- */
/* 24. Local OTA (integrator transport; the link has its own) [service]      */
/* ------------------------------------------------------------------------- */

typedef uint8_t jk_ota_kind_t;
#define JK_OTA_GENERATION ((jk_ota_kind_t)1)
#define JK_OTA_FIRMWARE   ((jk_ota_kind_t)2)

jk_status_t jk_ota_begin(jk_node_t *node, jk_ota_kind_t kind, uint32_t total_len);
jk_status_t jk_ota_write(jk_node_t *node, uint32_t offset, const uint8_t *data, uint32_t len);
/** JK_PENDING while verifying (call again), JK_OK when verified. */
jk_status_t jk_ota_finish(jk_node_t *node);
jk_status_t jk_ota_abort(jk_node_t *node);

/* ------------------------------------------------------------------------- */
/* 25. Diagnostics [any]                                                     */
/* ------------------------------------------------------------------------- */

typedef uint8_t jk_node_state_t;
#define JK_NODE_UNINIT   ((jk_node_state_t)0)
#define JK_NODE_RUNNING  ((jk_node_state_t)1)
#define JK_NODE_HALTED   ((jk_node_state_t)2)
#define JK_NODE_SHUTDOWN ((jk_node_state_t)3)

typedef struct jk_node_info {
    jk_node_state_t state;
    jk_clock_q_t    clock;
    uint8_t         confirming;     /* generation inside its confirm window */
    uint8_t         gen_pending;    /* a verified generation awaits a swap */
    uint32_t        node_bytes_used;
    uint64_t        gen_seq;
    uint64_t        iface_hash;
    uint64_t        confirm_deadline_us;
    uint8_t         source_hash[32];   /* generation hash (ref/10 2.6.2) */
    uint8_t         registry_hash[32];
    uint64_t        trust_version;     /* installed trust bundle version; 0 = anchors only */
    uint32_t        security_version;  /* confirmed firmware anti-rollback counter */
    uint32_t        jk_reserved;
} jk_node_info_t;

jk_status_t jk_node_info(const jk_node_t *node, jk_node_info_t *out);

#ifdef __cplusplus
}
#endif

#endif /* JOKER_NODE_H */
