/*
 * main.c -- EXAMPLE of library mode (ref/08 section 2.6), the loop of doc 19
 * section 4.2 without FreeRTOS. Syntax-checked against joker_node.h:
 *   gcc -fsyntax-only -std=c11 -Wall -Wextra -pedantic -I.. main.c
 */
#include "joker_project.h"   /* generated; includes joker_node.h */

/* ---- board support (the integrator's own code) ---- */
extern uint64_t board_timer_us(void);
extern float    read_level_m(void);
extern bool     read_pump_feedback(void);
extern void     gpio_set_level(int pin, bool level);
extern void     board_reset(void);
extern void     watchdog_feed(void);
extern void     wait_until_us(uint64_t t);
extern int32_t  uart_rx(void *ctx, uint8_t *buf, uint32_t cap);
extern int32_t  uart_tx(void *ctx, const uint8_t *buf, uint32_t len);
extern jk_status_t flash_info(void *user, jk_area_t a, jk_area_info_t *i);
extern jk_status_t flash_read(void *user, jk_area_t a, uint32_t off, void *buf, uint32_t len);
extern jk_status_t flash_write(void *user, jk_area_t a, uint32_t off, const void *d, uint32_t len);
extern jk_status_t flash_erase(void *user, jk_area_t a, uint32_t off, uint32_t len);
#define PUMP_PIN 5

/* ---- symbols required by the ABI ---- */
uint64_t jk_now_us(void) { return board_timer_us(); }

JK_NORETURN void jk_fatal(jk_status_t code, uint32_t detail)
{
    (void)code;
    (void)detail;
    for (;;) {
        board_reset();          /* outputs fall to the hardware reset state */
    }
}

static jk_node_t node;          /* static state, no malloc */

static const jk_platform_t platform = {
    .user        = NULL,
    .area_info   = flash_info,
    .area_read   = flash_read,
    .area_write  = flash_write,
    .area_erase  = flash_erase,
    .area_map    = NULL,
    .link_key    = NULL,
    .notify      = NULL,
    .trust_keys  = NULL,
    .trust_count = 0,
};

/* timer ISR: supervision and watchdog */
void timer_isr(void)
{
    jk_supervision_t s;
    if (jk_supervise(&node, jk_now_us(), &s) == JK_OK) {
        if (s.forced_groups & (1u << JK_GROUP_MAIN)) {
            jk_value_t safe;
            if (jk_safe_get(&node, JK_SIG_PUMP_RUN, &safe) == JK_OK) {
                gpio_set_level(PUMP_PIN, safe.as.b);
            }
        }
        if (s.feed_watchdog) {
            watchdog_feed();
        }
    }
}

int main(void)
{
    uint64_t now = jk_now_us();
    jk_status_t st = jk_init(&node, &JK_GENERATION, &platform, now);
    if (st != JK_OK) {
        jk_fatal(st, 0);
    }

    const uint64_t period = jk_group_period_us(&node, JK_GROUP_MAIN);
    uint64_t next = now;
    for (;;) {
        now = jk_now_us();
        (void)jk_set_input(&node, JK_SIG_TANK_LEVEL, read_level_m(), JK_GOOD, now);
        (void)jk_set_input(&node, JK_SIG_PUMP_RUNNING, read_pump_feedback(), JK_GOOD, now);

        (void)jk_tick(&node, JK_GROUP_MAIN, now);   /* latch -> apps -> arbiter -> envelope -> commit */

        bool pump = false;
        jk_quality_t q;
        if (jk_get_output(&node, JK_SIG_PUMP_RUN, &pump, &q) == JK_OK) {
            gpio_set_level(PUMP_PIN, pump);           /* always apply: the value is admissible */
        }

        (void)jk_link_poll(&node, uart_rx, uart_tx);  /* link + one service step */

        jk_event_t ev;
        uint8_t payload[64];
        while (jk_event_next(&node, &ev, payload, sizeof payload) == JK_OK) {
            /* e.g. show alarm transitions on a local display */
        }

        next += period;
        wait_until_us(next);
    }
}
