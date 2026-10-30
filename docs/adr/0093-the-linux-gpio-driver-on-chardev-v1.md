# ADR 0093 — The Linux `gpio` driver on chardev v1

**Status:** accepted (v1) · **Date:** 2026-10

## Context

Document 05 §10 lists the Linux `gpio` driver as "chardev v2", and ref/06 §3.4 describes `Link::Gpio` as "line requests (chardev v2 / embedded-hal)". The driver that shipped with AUR-401 uses the **v1** ABI of the GPIO character device (`GPIO_GET_LINEHANDLE_IOCTL`, `GPIOHANDLE_GET_LINE_VALUES_IOCTL`, `GPIOHANDLE_SET_LINE_VALUES_IOCTL`). Nothing in the specification backs that choice. The review of main found the divergence on 2026-10-28.

The two ABIs differ in what the 1.0 runtime needs:

- v1 is in every kernel since 4.8. Its bias flags (`pull-up`, `pull-down`, `disabled`) need 5.5. Kernels from 5.10 still carry it unless they are built without `CONFIG_GPIO_CDEV_V1`. The Raspberry Pi 3 B+ of the development bench (document 23 §1) runs a 4.19 kernel.
- v2 needs 5.10. It adds per-line reconfiguration, hardware debounce, and edge events with a choice of clock. Those are the only things that need it.

The manifest offers `edges`, `timestamp` and `debounce` on a `gpio` signal (ref/01 appendix A, ref/03 §2.2.5). The 1.0 driver implements none of the three:

- `edges` and `timestamp` reach the driver's address schema and fail the manifest check with `J0140` ("unknown key"). That does not say what ADR 0064 asks for a surface the runtime does not implement yet.
- `debounce` is one of the common signal adjustments (ref/01 §5.8.1). It went into the address the driver receives, and the driver refused it when it planned. A `gpio` input with `debounce` therefore passed `jk check` and failed to start. ref/01 §5.8.1 says a `bool` read by poll, "a GPIO without `edges`" included, is debounced by the acquisition stage.

## Decision

1. **The Linux `gpio` driver of 1.0 uses chardev v1.** Each line the manifest grants is requested once when the driver opens, with its direction, its initial output level (its `safe`), its bias and its drive. Every read is a poll. A kernel without v1, or one older than 5.5 asked for a bias other than `as-is`, refuses the request, and the device does not open (a device fault, ref/06 §2.12). It is not a check error, because the kernel is only known on the node.
2. **v2 arrives with `edges`.** The minor that implements interrupt-driven reads (`edges`, `timestamp`, kernel `debounce`) moves the driver to v2 for the lines that declare them. Until then, `edges` and `timestamp` on a `gpio` signal are **reserved**: `jk check` refuses them with `J0807`, not `J0140`. The descriptor of the 1.0 catalog lists them as reserved address keys of its family. In a profile of the `gpio` family (an expander), the address keys are the driver's to validate (ref/03 §2.2.5), and the driver refuses all three with `J0853` when it plans.
3. **`debounce` on a manifest `gpio` signal is the acquisition stage's.** It is the common adjustment of ref/01 §5.8.1, applied to a `bool` read by poll (document 16 §3.4). It never reaches the driver's address. The same holds for every adjustment key of ref/01 §5.8.1 on a signal declared without a profile: the address the driver receives carries only its own family's keys.
4. Document 05 §10, ref/06 §3.4, ref/01 §5.8.1 and appendix A, and ref/03 §2.2.5 say chardev v1 and point here. On an MCU, `gpio` is the board's `embedded-hal` pin (ref/01 appendix A) and this ADR does not apply.

## Consequences

- The bench Pi 3 and every kernel from 4.8 run the `gpio` driver. Bias other than `as-is` needs 5.5.
- A `gpio` input with `debounce` loads and is debounced by polling, as ref/01 §5.8.1 already said. Before, it planned with an address its driver refused.
- `edges` and `timestamp` give the code the reserved surfaces of ADR 0064 give, and are accepted unchanged in the minor that implements them.
- Moving to v2 later changes no manifest. The keys are the same and the v1 lines keep their meaning.
