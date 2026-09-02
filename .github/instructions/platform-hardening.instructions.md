---
applyTo: "src/driver/drv_mpu.*,src/driver/drv_sensor.*,src/driver/drv_tamper.*,src/driver/drv_bpk.*,src/hardware_interface/**,src/presetting.*,src/boot_update.*,src/config/**,src/main.c,src/webusb_protocol/services/service_file_trans.*,CMakeLists.txt,firmware.cmake,build.py"
---

# Platform hardening: MPU, OTP, tamper, firmware update, build flags

## Protection toggles

- Any code that disables or relaxes a protection (MPU regions, the MemManage fault handler,
  OTP write protection, tamper or glitch sensors, the watchdog) must restore it on every
  return path, including error returns. Prefer a single cleanup label. Flag a new toggle
  that has no matching restore, or a restore that only runs on the success path.
- ARMv7-M MPU: the highest-numbered overlapping region wins. A new restrictive region must
  use a higher number than the full-SRAM background region or it is dead. Flag a change that
  reorders regions or widens the background region's permissions.
- SRAM stays execute-never and function pointers stay out of writable memory wherever
  possible. Flag a change that marks a RAM region executable or adds a writable function
  pointer table.

## Tamper and sensor configuration

- Do not disable tamper, mesh, voltage-glitch, or crystal monitors. A change to `SensorInit`,
  `TamperStartup`, or the sensor interrupt handler must state which detector it affects and
  why.
- The tamper response must complete on coin-cell power and must not depend on a call that
  can be made to fail from the outside. Flag a new dependency in the erase path.

## OTP

- OTP writes are irreversible and belong only in factory or presetting flows. Require an
  "already programmed" check before writing, never write from a USB, QR, or UI-reachable
  command, and never write a value that came from the host without device-side validation.

## Firmware update and anti-rollback

- Verify the signature over a SHA-256 digest of the full image before any byte of the image
  is used, decompressed, or flashed. No MD5 or SHA-1 anywhere in the trust chain, and no
  "verify after decompress".
- Enforce a monotonic version: reject an image whose version is below the current firmware
  or below the OTP rollback floor.
- The bootloader burns a new OTP rollback-floor record only when `SOFTWARE_VERSION_MAJOR` or
  `SOFTWARE_VERSION_MINOR` in `src/config/version.h` rises; a `SOFTWARE_VERSION_BUILD`-only
  bump leaves the device downgradable to the previous release. A release that carries a
  security fix must therefore raise major or minor. Flag a security fix paired with a
  build-only bump.
- The update file's declared sizes, offsets, and chunk indices are attacker-controlled;
  bound them against the flash layout in `src/config/flash_address.h` and the receive buffer.
- Bootloader update payloads ride inside the signed application image. Flag a change that
  reads bootloader length or content from anywhere else.

## Boot and pre-RTOS window

- Code that runs before the scheduler starts (the early part of `main.c`) must not
  expose USB or any other host-reachable interface longer than necessary, and must initialise
  the MPU and tamper checks before the LCD, flash, and PSRAM.

## Build flags

- Keep `HARDENING_FLAGS` in `firmware.cmake` (`-Wformat -Werror=format-security
  -Werror=format`). Do not add `-Wno-format*`, `-fno-stack-protector`, or similar for project
  sources; a per-source exemption is acceptable only for vendored code under `external/`.
- A change to `feature_toggle.yaml`, `flash_address.h`, or linker sections must explain the
  memory-layout consequence (what moves next to what).
