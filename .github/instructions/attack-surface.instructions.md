---
applyTo: "src/webusb_protocol/**,external/mh1903_lib/SCPU_USB_Lib/**,external/SCPU_USB_*/**,src/tasks/**,src/managers/fingerprint_process.*,src/managers/sdcard_manager.*,src/user_sqlite3.c,src/user_fatfs.c,src/device_settings.c,src/ui/gui_analyze/gui_resolve_ur.*,rust/sandbox_parser/**"
---

# Attacker-controlled input paths

Bytes in these files arrive from a USB host, a QR code, an SD card, the fingerprint module,
or a stored file that an attacker may have written. Assume every field is hostile, trace
each length and offset back to its source, and check it against the real destination buffer.

## USB transport and class handlers

- Any `wLength`, `len`, `count`, `xfer_count`, `packet_index`, or `total_packets` that comes
  from the host must be clamped to the destination buffer size before `memcpy`, a DMA setup,
  or an endpoint transfer in either direction. A fixed-size control buffer (for example a
  7-byte line-coding struct) accepts exactly its size and nothing larger.
- Unimplemented or over-length class requests must STALL the endpoint, never transfer.
- The EP0 and non-EP0 OUT paths, `USBD_CtlSendData`, `USBD_CtlPrepareRx`, and the endpoint
  start-transfer code are shared by every class (CDC, MSC). A fix in one class handler does
  not protect the others; say so when reviewing a partial fix.
- Watch for state skew: writing `rx_count` bytes while accounting for
  `MIN(rx_count, maxpacket)`, or advancing an index on one path and not the other.
- Interrupt context: no allocation, no secure-element or flash access, no logging, and no
  lock-screen timer reset from a USB interrupt handler. USB activity must not keep the device
  unlocked.

## EAPDU and protocol parsing

- Validate every TLV, length prefix, and packet index before using it. Match the file's
  existing fail-closed idiom (`CHECK_LENGTH`, `ASSERT`, `return NULL`): stop, send nothing,
  and do not continue with partial data.
- Multi-packet reassembly must bound `total_packets`, require `packet_index < total_packets`,
  reject duplicate or out-of-order fragments, reset on error, and time out so a half-received
  request cannot wedge the protocol task.
- `request_id` and per-request state must be cleared when the request ends or fails so a
  later request cannot inherit it.

```c
// Wrong: host-controlled length reaches memcpy unchecked
memcpy(g_cmdBuff, pdata, req->wLength);

// Right: clamp to the real buffer, reject anything larger
if (req->wLength > sizeof(g_cmdBuff)) { USBD_CtlError(pdev, req); return; }
memcpy_s(g_cmdBuff, sizeof(g_cmdBuff), pdata, req->wLength);
```

## Authentication and command surface

- Commands that read device information, export public keys or addresses, start a firmware
  update, reboot, erase, format, or write flash must check that the device is unlocked and,
  where the flow requires it, that the user approved on screen. Flag a new command reachable
  while locked, and flag any growth of the locked-state command set.
- A "get public key" style response must be generated on the device, never echoed from the
  request.
- Reboot or erase triggered by a storage write (for example after a mass-storage file copy)
  must not be unconditional.

## Adjacent memory and control flow

- For every global or static buffer written from input, check what follows it in `.bss` /
  `.data`: function pointers, FreeRTOS static timers and their callbacks, task control blocks,
  USB class callback tables. An overflow into any of those is code execution.
- Do not rely on the MPU, scheduler state, or "the parser task cannot call that" as a security
  boundary against code that already runs on the MCU. Stop the bug at the write.

## Cross-task state

- The USB or data-parser task and the UI task share approval state for export, sign, and
  update flows. A shared struct must be mutex-protected or single-owner, and an approval must
  re-check that the request it approves is the one still in flight. Flag a free-then-reallocate
  of a request struct while a confirmation screen can still reference it.

## MPU sandbox (`src/tasks/mpu_sandbox_*`, `rust/sandbox_parser`)

- Code that runs inside the sandbox is unprivileged and must be pure: no `malloc`, no logging,
  no USB, secure-element, flash, or OTP access, no reads or writes outside the sandbox input,
  output, and stack regions, and no function pointers into privileged code.
- The privileged side validates every sandbox result after return (the `IsEapduResultValid`
  pattern) and must not trust lengths, indices, or status values from the sandbox output
  without re-checking them against its own copy of the input.
- The sandbox stack and buffers must keep fitting their MPU regions; keep the
  `_Static_assert` checks and update them when sizes change. Flag a syscall or raise-privilege
  path that can be invoked from inside the sandbox with attacker-chosen arguments.

## Fingerprint UART, SD card, JSON, and sqlite

- Fingerprint frames: bound the frame length to the receive buffer before copying, verify the
  checksum before acting, and reject a length field that exceeds the buffer instead of
  truncating.
- SD card and file input: sanitise file names, bound file sizes and path lengths, and treat
  file content as hostile even when the file "should" have been written by this device.
- cJSON: check the node type (`cJSON_IsString`, `cJSON_IsNumber`, ...) before reading
  `valuestring` or `valueint`, and bound string lengths before copying into fixed buffers.
- sqlite: use prepared statements with bound parameters; never build SQL by string
  concatenation from input.
