# Keystone3 firmware: pull request review instructions

Keystone3 is a hardware wallet. This repository is the device firmware: C (FreeRTOS + LVGL,
`src/`) and Rust (`rust/`) running on the MH1903 Cortex-M4 MCU, with two secure elements
(ATECC608B over I2C, DS28S60 over SPI) and on-chip OTP. Per-chain transaction parsing and
signing live in `rust/apps/<chain>`, seed-to-key derivation in `rust/keystore`, and the C/Rust
boundary in `rust/rust_c` (cbindgen generates `librust_c.h`).

Path-specific files under `.github/instructions/` add deeper checks for keys and secrets,
chain signing, attacker-controlled input paths, platform hardening, and UI flows. This file
holds the rules that apply to every pull request.

## Review language

- Write every review comment, finding, suggestion, and summary in Simplified Chinese
  (简体中文). Keep code identifiers, file paths, function names, macro names, flag names,
  and quoted code in their original form; do not translate them.
- Technical terms with an established English form (TRNG, nonce, PSBT, sighash, MPU, OTP,
  SE, UR, FFI, zeroize) may stay in English inside the Chinese sentence.

## Blocking findings

- Start the inline comment with the exact tag `[BLOCKER]` (the first characters of the
  comment body, before the Chinese text) when the finding is in one of these consequence
  tiers: seed or private-key disclosure, code execution on the MCU, or signing something
  other than what the user approved. A CI gate (`.github/workflows/copilot-review-gate.yml`)
  blocks merging while any `[BLOCKER]` thread is unresolved.
- These patterns always land in a blocking tier, so tag them:
  - Randomness that is not the device TRNG path (`TrngGet` / `random_buffer` /
    `GenerateTRNGRandomness` in C, `OsRng` / `getrandom` in Rust) feeding mnemonic or seed
    entropy, SLIP39 shares or identifiers, key generation, signature nonces or randomizers,
    or the per-device salt. Includes libc `rand()`, an LCG, a constant or time or
    message-derived seed, and a weak-symbol fallback that can shadow the TRNG
    implementation at link time.
  - A signature nonce, randomizer, or ring scalar that is reused, caller-supplied, or
    derived only from public data.
  - A seed, private key, mnemonic, or PIN written to a log, an error string, a USB or QR
    response, or persistent storage in plaintext.
  - An attacker-controlled length, offset, or index reaching a memory write without a
    bound against the real destination buffer.
  - A confirmation screen value that is not derived from the bytes being signed, or a
    request that skips the master-fingerprint or derivation-path check.
- Put the tag on the inline comment anchored to the offending line, one tag per finding.
  Do not put it only in the review summary; the gate reads inline threads.
- Use the tag only when the attacker step is concrete in this diff. Do not use it for lower
  tiers (secret residency, PIN or lock bypass, denial of service, information leak) or for
  style, test, or documentation issues; describe those without the tag.

## Threat model and how to rank findings

- Attacker model: a malicious USB host or cable, a malicious QR code, SD card, or fingerprint
  module, or brief physical access. There is no network stack.
- The seed is the crown jewel. Any code execution on the MCU is treated as total compromise,
  because the MCU gates access to the secure-element shares. Any attacker-influenced memory
  write is therefore a potential seed leak.
- Rank findings by consequence: seed or private-key disclosure > code execution > signing
  something other than what the user approved > secret residency in RAM > bypass of PIN or
  lock state > denial of service > information leak.
- The project deliberately fails closed. On malformed or unexpected input the convention is
  `ASSERT(...)` (reboots the device) or an early `return NULL` / `return Err(...)` with no
  response. Do not recommend replacing such a hard failure with a "graceful" continue, and do
  not report a reboot on bad input as a high-severity denial of service. Do report code that
  keeps executing after it has detected bad data.
- Report the attacker step, not just the pattern: which input reaches the line, and what the
  attacker gets. When a fix closes one entry point of a shared path (for example one USB class
  handler but not the shared endpoint transfer code), say so.

## Rules for every pull request

### Memory safety (C)

- Every length, offset, count, or index that originates outside the firmware (USB packet, QR
  payload, SD file, secure-element response, fingerprint UART frame, JSON, sqlite, CBOR) must be
  checked against the real destination buffer size before it reaches `memcpy`, `memcpy_s`, a
  DMA or endpoint transfer, or an array index. Checking against a protocol maximum is not
  enough when the buffer is smaller.
- Use the bounded safec forms with the true destination size: `memcpy_s`, `strcpy_s` or
  `strncpy_s`, `snprintf_s`, `memset_s`. Flag `strcpy`, `strcat`, `sprintf`, `vsprintf`, and
  bare `memcpy` into fixed-size buffers.
- Look at what lives next to a global or static buffer that is written: function pointers,
  FreeRTOS timer or task control blocks, callback tables, vtables. An overflow into those is
  code execution, not a crash.
- Length arithmetic: flag `a + b <= limit` where the sum can wrap, narrowing to `uint8_t` or
  `uint16_t`, signed/unsigned comparisons, and shifts such as `x << 24` on an 8-bit value
  (undefined behaviour).
- Format strings must be literals. `printf(buf)` and `vsprintf` are banned and the build sets
  `-Werror=format-security`; do not accept new `-Wno-format*` exemptions for project code.
- Heap: `SRAM_MALLOC` pairs with `SRAM_FREE`, `EXT_MALLOC` with `EXT_FREE`. Check that every
  early-return path frees, that nothing dereferences a pointer after freeing it, and that the
  return value of every allocation, secure-element, flash, and OTP call is checked.
- Stack: FreeRTOS task stacks are small. Flag new local arrays or structs above roughly 1 KB,
  variable-length arrays, `alloca`, and recursion on input-controlled depth.
- Every `switch` case ends in `break`, `return`, or an explicit `/* fallthrough */` comment.

### Memory and input safety (Rust)

- Crates are `no_std` + `alloc` and a panic reboots the device at best. On any data that can
  come from outside (UR/CBOR, protobuf, JSON, PSBT, USB payload, a C pointer plus length),
  flag `unwrap()`, `expect()`, direct indexing `x[i]` or `x[a..b]`, truncating `as` casts, and
  unchecked arithmetic. Prefer `get()`, `checked_*` / `saturating_*`, `try_into()`, and
  returning `Err`.
- Every `unsafe` block needs a comment stating why its invariant holds. At the FFI boundary:
  null-check every pointer, treat a C-provided length as untrusted before `from_raw_parts` or
  `extract_array!`, and make sure every pointer handed to C has a matching `free_*` export
  (see `rust/rust_c/src/common/free.rs`) that the C caller actually invokes on every path.
- Bound everything derived from input: nesting depth, item counts, string lengths, allocation
  sizes, number of multi-part QR fragments.

### Secrets

- Seeds, private keys, mnemonics, passphrases, PINs, and secure-element keys are transient.
  Wipe them on every exit path, including error paths: Rust `zeroize()` or `Zeroizing<T>`;
  C `memset_s`, `CLEAR_ARRAY`, or `CLEAR_OBJECT`. Bare `memset` may be optimised away and does
  not count.
- Never put a secret in a new global or static, in a log (`printf`, `PrintArray`, log
  helpers), in an error string, or in a panic message.
- Comparisons involving secrets or authentication tags (password hashes, HMACs, secure-element
  MACs, signatures) must be constant-time. Flag `memcmp`, `strcmp`, and byte-slice `==` on
  such values.
- Randomness: the only accepted entropy sources are the C helpers `TrngGet`,
  `random_buffer`, and `GenerateTRNGRandomness` (hardware TRNG mixed with both secure
  elements' RNGs), and in Rust `getrandom` or `rand_core::OsRng`, which route to the same
  helper through the custom `getrandom` backend in `rust/rust_c/src/trng.rs`. Flag libc
  `rand()`, `ChaCha20Rng::from_seed` or any `SeedableRng` seeded from a constant, a timestamp,
  a counter, or public data, and any hard-coded "random" bytes. Deterministic derivation from
  the master seed is acceptable only when it is intentional and commented as such.

### Concurrency

- State shared between the USB or data-parser task, the fingerprint task, the sensitive-data
  task, and the UI task needs a mutex or a single owner. Flag a new global touched from more
  than one task without one.
- A confirmation screen must act on exactly the request the user saw: re-validate the request
  identity before acting and reject a second in-flight request.

### Build variants and gating

- Three products build from one tree: multi-coin (default), `btc_only` (`-DBTC_ONLY=true`),
  and `cypherpunk` (`-DCYPHERPUNK=true`). Rust mirrors this with the cargo features
  `multi-coins`, `btc-only`, and `cypherpunk`, and cbindgen maps features to C defines in
  `rust/rust_c/cbindgen.toml`. New chain or wallet code must be gated consistently on both
  sides so nothing leaks into `btc_only` or `cypherpunk`.
- CI runs only Rust crate tests and `rustfmt`; it does not build the firmware image. That is
  a known, accepted gap and is not a review finding. Do not comment that a change "has not
  been verified by a firmware build", do not ask the author to run the three variants or the
  simulator, and do not ask for a build-or-test statement in the pull request description.
  Whether and how a change was built is verified by the maintainers before merge, outside
  the review. Review what the diff does instead: if you can point to a concrete compile or
  link problem (an undefined symbol, a missing include, a mismatched prototype, a gate that
  leaves a call unresolved in one variant), report that specific problem.

### Tests and documentation

- Signing, parsing, address-derivation, and KDF changes need unit tests with vectors from the
  official specification or reference implementation, plus at least one negative case (wrong
  network, wrong master fingerprint, truncated or oversized input).
- Rust must pass `cargo fmt --check`; C follows the AStyle command in `README.md`.
- User-visible changes update `CHANGELOG.md` (and the BTC and ZH variants when applicable).
- Strings added to or changed in `src/ui/lv_i18n/data.csv` require the generated font files
  under `src/ui/gui_assets/font/` to be regenerated in the same pull request; otherwise
  non-Latin characters render as empty boxes.
