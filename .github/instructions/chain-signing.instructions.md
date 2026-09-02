---
applyTo: "rust/apps/**,rust/rust_c/src/**,src/ui/gui_chain/**,src/ui/gui_analyze/**"
---

# Chain parsing, signing, and the C/Rust boundary

Apply these checks when a pull request adds a chain, adds a transaction or message type,
adds a UR type, or changes parsing, address derivation, or signing.

## Signature algorithm and nonce

- Confirm the algorithm matches the chain and the key type, and that signing goes through the
  shared helpers rather than a new inline implementation:
  - secp256k1 ECDSA (Bitcoin legacy/segwit, EVM chains, Tron, Cosmos secp accounts): use the
    `rust/keystore` `secp256k1` helpers built on `rust-secp256k1`. The nonce is deterministic
    (RFC 6979) and the signature is low-s. Flag any code that supplies its own `k`, calls a
    lower-level signing primitive with a caller-chosen nonce, or skips `normalize_s`.
  - Taproot: BIP340 Schnorr on the tweaked key via `rust-bitcoin`'s PSBT signing. Flag a
    signature made with the untweaked key, or a script-path signature where key-path was
    requested.
  - ed25519: SLIP-0010 derivation for Solana, Sui, Aptos, Near, TON, Stellar; BIP32-Ed25519
    (Icarus) for Cardano. Do not mix the two. The signed bytes must be exactly what the chain
    specifies (for example an intent prefix plus hash, or a hash of the transaction body).
  - Randomised schemes (Zcash Orchard RedPallas randomizers, Monero CLSAG and range proofs,
    RSA-PSS salt for Arweave): every signature draws fresh randomness from `OsRng`. Flag a
    randomizer, ring scalar, or salt derived from the message, reused across inputs, or taken
    from a seeded PRNG.
- A recoverable signature must carry the correct recovery id; check chains that encode it in
  `v` with a chain-id offset.
- The sighash or signing digest must be built exactly per specification, including domain
  separation tags, chain id or network id, and replay-protection fields. Any of those that the
  user could care about (network, chain id) must be shown on the confirmation screen.
- Restrict sighash types. Flag acceptance of `SIGHASH_NONE`, `SIGHASH_SINGLE`, or
  `ANYONECANPAY` unless the type is displayed and explicitly approved.

## Request binding

- The master fingerprint and derivation path carried in the UR must be checked against the
  device's own fingerprint before signing (see the comparison in
  `rust/rust_c/src/ethereum/mod.rs`). A mismatch is an error, not a fallback.
- Validate derivation paths: bounded depth, hardened where the chain requires it, and within
  the purpose and coin type expected for that chain unless a documented exception exists.
- A public key or address returned to the host must be derived on the device from the seed,
  never echoed back from request bytes.

## What you see is what you sign

- Every field the confirmation screen shows (recipient, amount, fee, memo, contract, method,
  network) must be produced from the same parsed structure that is signed. Flag a second
  parse of the raw bytes for display, and any display value computed from a field that is
  not covered by the signature.
- Amounts and fees use checked arithmetic and the chain's decimal precision. Flag `as u64`
  truncation, float conversion for money, and silent unit assumptions.
- If a component cannot be decoded (unknown contract call, unknown instruction, opaque
  payload), the whole request either fails or is explicitly labelled as blind signing with the
  raw data shown. Silently omitting a field is a finding.
- Multi-output or batch requests must show every output and every change address, and change
  detection must verify the derivation path, not just compare address strings.

## Parsing untrusted input

- The request bytes are attacker-controlled. Follow the repository-wide Rust rules: no
  `unwrap`/`expect`/direct indexing on input, checked arithmetic, bounded sizes, `Result`
  everywhere. Prefer the existing decoders (`ur-registry`, `rust-bitcoin` PSBT, prost,
  `minicbor`, `serde_json`) over hand-written parsers.
- A new UR type must be registered in `rust/rust_c/src/common/ur.rs` and in the chain
  callback table in `src/ui/gui_chain/gui_chain.c`, with a bound on total payload size and on
  the number of multi-part fragments.
- Reject trailing data after a complete structure, and reject duplicates where the format
  forbids them.

## FFI conventions (`rust/rust_c`)

- Exports are `#[no_mangle] pub extern "C" fn` taking `PtrBytes` plus a length (or
  `PtrString`) and returning a heap-allocated response (`SimpleResponse<T>`, `UREncodeResult`,
  `TransactionCheckResult`, chain-specific structs). Use `extract_array!` / `extract_array_mut!`
  and `recover_c_char` for inputs; check for null and for a plausible length first.
- Every returned pointer type has a `free_*` export. Confirm the C caller frees it on every
  path, including the error path, and that the C side never frees a Rust pointer with `free()`
  or a Rust struct field individually.
- `seed.zeroize()` before every return in an export that receives a seed.
- Gate a new export with the same cargo feature as its crate so cbindgen and the C `#ifdef`
  stay in sync; check `rust/rust_c/cbindgen.toml` and `rust/rust_c/Cargo.toml` `[features]`.

## Variant gating and tests

- A new chain must be absent from the `btc_only` and `cypherpunk` builds unless explicitly
  intended: cargo feature in `rust/rust_c/Cargo.toml`, cbindgen define, and `#ifndef BTC_ONLY`
  / `#ifdef CYPHERPUNK` on the C side, all consistent.
- Tests must include vectors from the chain's official SDK or reference implementation, verify
  the produced signature against the public key inside the test, and cover at least one
  wrong-fingerprint and one malformed-input case. Add the crate to a CI workflow under
  `.github/workflows/` if it is new.
