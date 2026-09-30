#[no_mangle]
#[cfg_attr(target_os = "none", link_section = ".sandbox_parser_entry")]
pub unsafe extern "C" fn mpu_sandbox_validate_ur(input: *const u8, input_len: usize) -> u32 {
    if input.is_null() {
        return sandbox_parser::ValidationStatus::InvalidInput as u32;
    }

    let input = core::slice::from_raw_parts(input, input_len);
    sandbox_parser::validate_ur(input) as u32
}

#[no_mangle]
#[cfg_attr(target_os = "none", link_section = ".sandbox_parser_entry")]
pub unsafe extern "C" fn mpu_sandbox_validate_cbor(input: *const u8, input_len: usize) -> u32 {
    if input.is_null() {
        return sandbox_parser::ValidationStatus::InvalidInput as u32;
    }

    let input = core::slice::from_raw_parts(input, input_len);
    sandbox_parser::validate_cbor(input) as u32
}

#[no_mangle]
#[cfg_attr(target_os = "none", link_section = ".sandbox_parser_entry")]
/// Push one mailbox payload into the streaming CBOR validator.
///
/// # Safety
/// For arguments that pass the checks below, `storage` must point to a live,
/// aligned, writable allocation containing an initialized `State`: zeroed before
/// the first chunk, then preserved between chunks of the same message. The caller
/// must provide exclusive access to that state for the duration of this call.
/// `input` must point to at least `input_len` initialized, readable bytes in one
/// allocation, remain unchanged during this call, and not overlap the state.
pub unsafe extern "C" fn mpu_sandbox_cbor_stream_push(
    storage: *mut u8,
    capacity: usize,
    input: *const u8,
    input_len: usize,
    last: bool,
) -> u32 {
    use sandbox_parser::cbor_stream::State;
    // Match MPU_SANDBOX_INPUT_SIZE - MPU_SANDBOX_UR_CHUNK_HEADER_SIZE in C.
    const MAX_STREAM_INPUT_LEN: usize = 232 - 4;
    if storage.is_null()
        || input.is_null()
        || input_len > MAX_STREAM_INPUT_LEN
        || capacity < core::mem::size_of::<State>()
        || (storage as usize) % core::mem::align_of::<State>() != 0
    {
        return sandbox_parser::ValidationStatus::InvalidInput as u32;
    }
    // SAFETY: Size/alignment are checked above; the caller guarantees valid,
    // initialized storage and exclusive access as documented in the contract.
    let state = unsafe { &mut *storage.cast::<State>() };
    // SAFETY: Length is bounded above; the caller guarantees readable initialized
    // input that remains unchanged and does not overlap the mutable state.
    let input = unsafe { core::slice::from_raw_parts(input, input_len) };
    state.push(input);
    if last {
        state.finish()
    } else {
        0
    }
}

#[no_mangle]
#[cfg_attr(target_os = "none", link_section = ".sandbox_parser_entry")]
pub unsafe extern "C" fn mpu_sandbox_validate_eip712_json(input: *mut u8, input_len: usize) -> u32 {
    if input.is_null() {
        return sandbox_parser::ValidationStatus::InvalidInput as u32;
    }

    let input = core::slice::from_raw_parts_mut(input, input_len);
    sandbox_parser::validate_eip712_json(input) as u32
}
