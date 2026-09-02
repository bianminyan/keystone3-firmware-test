---
applyTo: "rust/keystore/**,rust/rust_c/src/common/**,rust/rust_c/src/wallet/**,src/managers/**,src/crypto/**,src/driver/drv_atecc608b.*,src/driver/drv_ds28s60.*,src/driver/drv_otp.*,src/driver/drv_trng.*,src/hardware_interface/se_interface.*,src/tasks/fetch_sensitive_data_task.*"
---

# Keys, secrets, KDF, and secure-element code

These files create, store, derive from, or verify the wallet seed and the user's PIN or
password. Treat every change here as security-critical, even a refactor.

## Entropy and randomness

- Mnemonic entropy, SLIP39 shares and identifiers, per-device salts, AES IVs, secure-element
  nonces and challenges, and ECDH ephemeral keys must come from `random_buffer` / `TrngGet`
  in C or `OsRng` / `getrandom` in Rust. Reject anything else, including `rand()`, a hash of
  the time, a constant seed, or bytes copied from a previous call.
- A nonce or challenge is fresh for every secure-element command. Flag a nonce that is
  cached in a static, reused across two commands, or shortened below what the command
  expects.
- The dice-roll entropy path must hash the user-supplied rolls; it must never use them as raw
  entropy or silently mix them with a weaker source.
- Code under `#ifdef COMPILE_SIMULATOR` may use weak randomness or stubbed secure elements.
  Check that the `#else` device path never inherits a simulator shortcut, and that no
  simulator-only branch becomes reachable in a production build.

```c
// Wrong: libc PRNG on device
for (i = 0; i < len; i++) buf[i] = rand() & 0xFF;

// Right: hardware TRNG mixed with both secure elements
random_buffer(buf, len);
```

```rust
// Wrong: deterministic "randomness" from public data
let mut rng = ChaCha20Rng::from_seed(sha256(tx_bytes));

// Right: the device TRNG through the registered getrandom backend
let sig = signing_key.sign_with_rng(&mut OsRng, &hash);
```

## PIN and password handling

- Every password-to-key or password-to-verifier step must use the per-device salt stored in
  OTP and an iterated or memory-hard construction. Flag a new path that verifies a credential
  with a single hash or HMAC, or that reuses a fast hash for a new purpose.
- Verification results must be compared in constant time. The attempt counter must live in the
  secure element and be updated before the comparison result is acted upon, never only in MCU
  RAM and never after a successful branch has already returned.
- Do not add new rate-limit, lockout, or counter logic that the MCU alone enforces; the
  secure element must enforce it.

## Seed and private-key handling

- Minimise callers that receive a raw seed (`GetAccountSeed`). A new caller must run on the
  sensitive-data task via `AsyncExecute`, declare the seed buffer outside any
  `do { ... } while (0)` so a single `CLEAR_ARRAY(seed)` after the loop covers every `break`,
  and wipe on the error path too.
- In Rust FFI exports, call `seed.zeroize()` (or wrap in `Zeroizing`) before every return,
  including early error returns; follow the pattern in `rust/rust_c/src/ton/mod.rs`.
- Derived private keys, extended private keys, and intermediate KDF buffers get the same
  treatment as the seed. Flag a `SecretKey` or `Xpriv` that is cloned into a struct that is
  not zeroized on drop.
- Reject trivially weak seeds and keys (all zero, all `0xFF`) before deriving from them.

```c
// Wrong: seed lives on the UI task stack and is never wiped on the error path
do {
    uint8_t seed[64];
    GetAccountSeed(idx, seed, password);
    ret = do_sign(seed);
    CHECK_CHAIN_BREAK(ret);
} while (0);
return ret;

// Right: one wipe covers every exit
uint8_t seed[64] = {0};
do {
    GetAccountSeed(idx, seed, password);
    ret = do_sign(seed);
    CHECK_CHAIN_BREAK(ret);
} while (0);
CLEAR_ARRAY(seed);
return ret;
```

## Secret cache (`src/crypto/secret_cache.c`)

- The cache (password, new password, entropy, mnemonic, SLIP39 shares, passphrase) is cleared
  by `ClearSecretCache()` at flow teardown (the view's DeInit) and on lock. Do not add a clear
  in the middle of a flow on a recoverable error; the import and create flows rewind to an
  earlier screen and still need the cached PIN.
- Do not add a getter that returns a copy of a secret the caller does not wipe, and do not add
  a new secret to the cache without adding it to `ClearSecretCache()`.

## Secure-element and OTP access

- All secure-element page reads go through the authenticated `SE_HmacEncryptRead` wrapper in
  `src/hardware_interface/se_interface.c` and equivalent authenticated ATECC608B helpers. Flag a new
  driver-level read that bypasses the challenge-response authentication, or new logic that
  treats bytes read from the secure element (status pages, counters, flags) as trusted without
  a MAC.
- The return status of every secure-element read, write, and authentication call must be
  checked and a failure must fail closed. A helper that always returns success hides errors;
  flag it.
- The account status state machine is written as a bracket (`CREATING` then `CREATED` around
  the secret write). Do not reorder these writes or add a path that leaves a partially written
  account looking valid.
- OTP writes are one-shot and irreversible. They belong only in factory or presetting flows and
  must be preceded by a `CheckEntropy` (or equivalent) "already written" test. Never write OTP
  from a USB, QR, or UI-reachable command.
- Both secure-element backends (`se_backend_gen1.c`, `se_backend_gen2.c`) are selected by the
  chip's locked configuration. A change to one backend must not alter the sealed-blob format
  of devices already in the field. A format change needs a documented migration and must keep
  the "forget password" and "single-wallet re-import" flows working.
- Page and slot numbers are a shared namespace across both drivers and `se_manager.h`. Flag a
  new page or slot that collides with an existing one or falls outside the authenticated
  range.

## Cryptographic primitives

- Do not hand-roll a primitive. Use `rust/keystore` algorithms or `src/crypto/utils` (HMAC,
  HKDF, PBKDF2). Flag a new hash, MAC, KDF, or cipher implementation written inline.
- A change to a KDF input, salt string, or iteration count changes what existing devices
  derive. Require a test vector from a real device or a documented migration.
