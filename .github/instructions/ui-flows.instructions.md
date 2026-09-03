---
applyTo: "src/ui/**"
---

# UI flows, confirmation screens, and secret handling in views

## Secrets in views

- Views never read a seed, mnemonic, or password on the UI task. Sensitive work goes through
  `AsyncExecute` on the sensitive-data task; a view only receives the result.
- Any local buffer that briefly holds a seed or password is wiped with `CLEAR_ARRAY` or
  `memset_s` on every exit path.
- The create and import wallet flows call `ClearSecretCache()` in the view's DeInit, and
  `LockScreen()` clears it on lock. Do not add a clear in the middle of a flow on a
  recoverable error: the duplicate-mnemonic retry rewinds to the mnemonic screen and still
  needs the cached PIN.
- Never place a secret in a label text, a log line, or a screenshot-visible widget longer
  than the screen that needs it; remove it when the screen is left.

## Confirmation screens

- What the user sees is what gets signed. Labels for recipient, amount, fee, network, memo,
  and contract data come from the parsed transaction struct returned by Rust, never from a
  separate parse of the raw bytes and never from host-supplied display hints.
- Show addresses in full or with a visible checksum; do not truncate silently. Show every
  output and change address; do not collapse them.
- A pull request that adds a transaction layout JSON in `src/ui/gui_analyze/gui_analyze.c`
  must add the matching `ui_simulator/assets/page_<chain>.json`, and the two must render the
  same fields.
- Approve and reject handlers must act only on the request that produced the screen; check the
  request identity before calling into the service layer, and disable the button after the
  first press.

## Lock screen and password errors

- A view that enters the lock screen through the "go home after password" purpose must
  handle both the pass and the fail signal. On fail it calls `GuiLockScreenPassCode(false)` and
  reports the error count, otherwise the lock screen stays stuck on "Verifying".
- The PIN pad shuffle uses `TrngGet`; do not replace it with a cheaper source.

## Variant gating

- Multi-coin, `btc_only`, and `cypherpunk` views live under `multi/`, `btc_only/`, and
  `multi/cypherpunk/` directories and behind `#ifdef BTC_ONLY` / `#ifdef CYPHERPUNK`. A new
  chain screen must not appear in the two restricted variants, and a changed shared widget must
  compile in all three. Report a specific `#ifdef`, symbol, or call that breaks one variant,
  anchored to the line. Do not ask for build proof or a verification statement; see "Out of
  scope: do not report" in `.github/copilot-instructions.md`.

## i18n and fonts

- Any string added to or changed in `src/ui/lv_i18n/data.csv` requires regenerating the fonts
  with `src/ui/lv_i18n/get_font_contain.py` in the same pull request; the generated files under
  `src/ui/gui_assets/font/<lang>/` must be part of the diff.
- The `font` size tag of a string must match the widget that renders it: title labels use 36,
  little-title and hint-box titles 28, text labels and buttons 24, illustrate and notice
  labels 20. A mismatch renders empty boxes even after regeneration.
- Long or CJK strings need an explicit width and `LV_LABEL_LONG_WRAP`; flag a label whose size
  was fixed from a short placeholder and later receives a long translation.

## LVGL resource hygiene

- Objects, timers, and animations created in a view's Init are deleted in its DeInit. Flag a
  timer or event callback that outlives its view, and a callback that dereferences a widget
  that may already be deleted.
- The UI task stack is tight: keep large arrays and JSON buffers off the stack, and free
  `SRAM_MALLOC`/`EXT_MALLOC` allocations on every path including when the user backs out.
