#![no_std]

extern crate alloc;
use crate::common::errors::RustCError;
use crate::common::structs::{SimpleResponse, TransactionCheckResult, TransactionParseResult};
use crate::common::ur::{UREncodeResult, FRAGMENT_MAX_LENGTH_DEFAULT};
use alloc::boxed::Box;
use alloc::string::ToString;
use app_monero::address::Address;
use app_monero::key::{PrivateKey, PublicKey};
use app_monero::structs::{AddressType, Network};
use app_monero::utils::constants::{OUTPUT_EXPORT_MAGIC, PRVKEY_LEH, UNSIGNED_TX_PREFIX};
// use crate::free::Free;
use crate::common::free::Free;
use crate::common::types::{PtrBytes, PtrString, PtrT, PtrUR};
use core::ffi::c_void;
use crate::common::utils::{convert_c_char, recover_c_char};
use crate::extract_array;
use crate::extract_ptr_with_type;
use cty::c_char;
use rand_core::OsRng;
use structs::{DisplayMoneroOutput, DisplayMoneroUnsignedTx};
use ur_registry::monero::xmr_keyimage::XmrKeyImage;
use ur_registry::monero::xmr_output::XmrOutput;
use ur_registry::monero::xmr_txsigned::XmrTxSigned;
use ur_registry::monero::xmr_txunsigned::XmrTxUnsigned;
use ur_registry::traits::{RegistryItem, To};

mod structs;

#[no_mangle]
pub unsafe extern "C" fn monero_get_address(
    pub_spend_key: PtrString,
    private_view_key: PtrString,
    major: u32,
    minor: u32,
    is_subaddress: bool,
) -> *mut SimpleResponse<c_char> {
    let spend_key = recover_c_char(pub_spend_key);
    let pvk = match hex::decode(recover_c_char(private_view_key))
        .ok()
        .and_then(|pvk| PrivateKey::try_from_slice(&pvk).ok())
    {
        Some(pvk) => pvk,
        None => return SimpleResponse::from(RustCError::InvalidData("invalid data".to_string())).simple_c_ptr(),
    };
    let public_spend = match PublicKey::from_str(spend_key.as_str()) {
        Ok(key) => key,
        _ => return SimpleResponse::from(RustCError::InvalidData("invalid data".to_string())).simple_c_ptr(),
    };

    match app_monero::address::generate_address(
        &public_spend,
        &pvk,
        major,
        minor,
        is_subaddress,
    ) {
        Ok(address) => SimpleResponse::success(convert_c_char(address.to_string()) as *mut c_char)
            .simple_c_ptr(),
        _ => {
            SimpleResponse::from(RustCError::InvalidData("invalid data".to_string())).simple_c_ptr()
        }
    }
}

unsafe fn safe_parse_key(decrypt_key: PtrBytes) -> Result<[u8; PRVKEY_LEH], RustCError> {
    let decrypt_key = extract_array!(decrypt_key, u8, PRVKEY_LEH);

    if decrypt_key.len() != PRVKEY_LEH {
        return Err(RustCError::InvalidMasterFingerprint);
    }

    Ok(decrypt_key.try_into().unwrap())
}

#[no_mangle]
pub unsafe extern "C" fn monero_output_request_check(
    ptr: PtrUR,
    decrypt_key: PtrBytes,
    pvk: PtrString,
) -> PtrT<TransactionCheckResult> {
    let request = extract_ptr_with_type!(ptr, XmrOutput);
    let decrypt_key = match safe_parse_key(decrypt_key) {
        Ok(decrypt_key) => decrypt_key,
        _ => return TransactionCheckResult::from(RustCError::InvalidMasterFingerprint).c_ptr(),
    };
    let pvk = match hex::decode(recover_c_char(pvk)) {
        Ok(pvk) if pvk.len() == PRVKEY_LEH => pvk,
        _ => return TransactionCheckResult::from(RustCError::InvalidMasterFingerprint).c_ptr(),
    };

    let payload = request.get_payload();

    match app_monero::utils::decrypt_data_with_decrypt_key(
        decrypt_key,
        pvk.try_into().unwrap(),
        payload,
        OUTPUT_EXPORT_MAGIC,
    ) {
        Ok(_) => TransactionCheckResult::new().c_ptr(),
        _ => TransactionCheckResult::from(RustCError::InvalidMasterFingerprint).c_ptr(),
    }
}

#[no_mangle]
pub unsafe extern "C" fn monero_unsigned_request_check(
    ptr: PtrUR,
    decrypt_key: PtrBytes,
    pvk: PtrString,
    pub_spend_key: PtrString,
) -> PtrT<TransactionCheckResult> {
    clear_approved_unsigned_tx();
    let request = extract_ptr_with_type!(ptr, XmrTxUnsigned);
    let decrypt_key = match safe_parse_key(decrypt_key) {
        Ok(key) => key,
        Err(_) => return TransactionCheckResult::from(RustCError::InvalidMasterFingerprint).c_ptr(),
    };
    if pvk.is_null() || pub_spend_key.is_null() {
        return TransactionCheckResult::from(RustCError::InvalidData("missing wallet keys".to_string())).c_ptr();
    }
    let pvk: [u8; 32] = match hex::decode(recover_c_char(pvk)).ok().and_then(|v| v.try_into().ok()) {
        Some(key) => key,
        None => return TransactionCheckResult::from(RustCError::InvalidMasterFingerprint).c_ptr(),
    };
    let public_spend: [u8; 32] = match hex::decode(recover_c_char(pub_spend_key)).ok().and_then(|v| v.try_into().ok()) {
        Some(key) => key,
        None => return TransactionCheckResult::from(RustCError::InvalidXPub).c_ptr(),
    };
    match app_monero::transfer::check_unsigned(request.get_payload(), decrypt_key, pvk, public_spend) {
        Ok(_) => TransactionCheckResult::new().c_ptr(),
        Err(app_monero::errors::MoneroError::MismatchedMfp) =>
            TransactionCheckResult::from(RustCError::InvalidMasterFingerprint).c_ptr(),
        Err(e) => TransactionCheckResult::from(RustCError::InvalidData(e.to_string())).c_ptr(),
    }
}

#[no_mangle]
pub unsafe extern "C" fn get_monero_pvk_by_seed(
    seed: PtrBytes,
    seed_len: u32,
) -> *mut SimpleResponse<c_char> {
    let seed = extract_array!(seed, u8, seed_len as usize);

    match app_monero::key::generate_keypair(seed, 0) {
        Ok(keypair) => {
            let pvk = keypair.view.to_bytes();
            SimpleResponse::success(convert_c_char(hex::encode(pvk)) as *mut c_char).simple_c_ptr()
        }
        _ => {
            SimpleResponse::from(RustCError::InvalidData("invalid seed".to_string())).simple_c_ptr()
        }
    }
}

#[no_mangle]
pub unsafe extern "C" fn monero_parse_output(
    ptr: PtrUR,
    decrypt_key: PtrBytes,
    pvk: PtrString,
) -> PtrT<TransactionParseResult<DisplayMoneroOutput>> {
    let request = extract_ptr_with_type!(ptr, XmrOutput);
    let decrypt_key = match safe_parse_key(decrypt_key) {
        Ok(decrypt_key) => decrypt_key,
        _ => return TransactionParseResult::from(RustCError::InvalidMasterFingerprint).c_ptr(),
    };
    let pvk = hex::decode(recover_c_char(pvk)).unwrap();

    match app_monero::outputs::parse_display_info(
        &request.get_payload(),
        decrypt_key,
        pvk.try_into().unwrap(),
    ) {
        Ok(display) => TransactionParseResult::success(Box::into_raw(Box::new(
            DisplayMoneroOutput::from(display),
        )))
        .c_ptr(),
        Err(_) => TransactionParseResult::from(RustCError::InvalidData("invalid data".to_string()))
            .c_ptr(),
    }
}

static mut APPROVED_UNSIGNED_TX: Option<[u8; 32]> = None;

fn remember_approved_unsigned_tx(payload: &[u8]) {
    unsafe {
        APPROVED_UNSIGNED_TX = Some(app_monero::transfer::payload_digest(payload));
    }
}

fn approved_unsigned_tx_matches(payload: &[u8]) -> bool {
    unsafe {
        APPROVED_UNSIGNED_TX == Some(app_monero::transfer::payload_digest(payload))
    }
}

fn clear_approved_unsigned_tx() {
    unsafe {
        APPROVED_UNSIGNED_TX = None;
    }
}

#[no_mangle]
pub unsafe extern "C" fn monero_parse_unsigned_tx(
    ptr: PtrUR,
    decrypt_key: PtrBytes,
    pvk: PtrString,
    pub_spend_key: PtrString,
) -> PtrT<TransactionParseResult<DisplayMoneroUnsignedTx>> {
    let request = extract_ptr_with_type!(ptr, XmrTxUnsigned);
    let decrypt_key = match safe_parse_key(decrypt_key) {
        Ok(decrypt_key) => decrypt_key,
        _ => return TransactionParseResult::from(RustCError::InvalidMasterFingerprint).c_ptr(),
    };
    // The private view key and the account's public spend key are needed to prove
    // that a "change" output really pays this wallet; never unwrap on host data.
    let pvk = match hex::decode(recover_c_char(pvk)) {
        Ok(pvk) if pvk.len() == PRVKEY_LEH => pvk,
        _ => return TransactionParseResult::from(RustCError::InvalidMasterFingerprint).c_ptr(),
    };
    let public_spend = match hex::decode(recover_c_char(pub_spend_key)) {
        Ok(key) if key.len() == PRVKEY_LEH => key,
        _ => return TransactionParseResult::from(RustCError::InvalidXPub).c_ptr(),
    };

    let payload = request.get_payload();

    match app_monero::transfer::parse_unsigned(
        payload.clone(),
        decrypt_key,
        pvk.try_into().unwrap(),
        public_spend.try_into().unwrap(),
    ) {
        Ok(display) => {
            // Only a parsed (and therefore displayable) payload may be signed.
            remember_approved_unsigned_tx(&payload);
            TransactionParseResult::success(Box::into_raw(Box::new(
                DisplayMoneroUnsignedTx::from(display),
            )))
            .c_ptr()
        }
        Err(_) => {
            clear_approved_unsigned_tx();
            TransactionParseResult::from(RustCError::InvalidData("invalid data".to_string()))
                .c_ptr()
        }
    }
}

#[no_mangle]
pub unsafe extern "C" fn monero_generate_keyimage(
    ptr: PtrUR,
    seed: PtrBytes,
    seed_len: u32,
    major: u32,
) -> PtrT<UREncodeResult> {
    let request = extract_ptr_with_type!(ptr, XmrOutput);
    let seed = extract_array!(seed, u8, seed_len as usize);
    let keypair = match app_monero::key::generate_keypair(seed, major) {
        Ok(keypair) => keypair,
        _ => return UREncodeResult::from(RustCError::InvalidData("invalid seed".to_string())).c_ptr(),
    };
    match app_monero::key_images::generate_export_ur_data(keypair, request.get_payload(), OsRng) {
        Ok(data) => {
            let data = XmrKeyImage::new(data);

            UREncodeResult::encode(
                data.try_into().unwrap(),
                XmrKeyImage::get_registry_type().get_type(),
                FRAGMENT_MAX_LENGTH_DEFAULT,
            )
            .c_ptr()
        }
        Err(_) => UREncodeResult::from(RustCError::InvalidData("invalid data".to_string())).c_ptr(),
    }
}

#[no_mangle]
pub unsafe extern "C" fn monero_generate_signature(
    ptr: PtrUR,
    seed: PtrBytes,
    seed_len: u32,
    major: u32,
) -> PtrT<UREncodeResult> {
    let request = extract_ptr_with_type!(ptr, XmrTxUnsigned);
    let seed = extract_array!(seed, u8, seed_len as usize);
    let payload = request.get_payload();
    if !approved_unsigned_tx_matches(&payload) {
        return UREncodeResult::from(RustCError::InvalidData(
            "unapproved transaction".to_string(),
        ))
        .c_ptr();
    }

    let keypair = match app_monero::key::generate_keypair(seed, major) {
        Ok(keypair) => keypair,
        _ => return UREncodeResult::from(RustCError::InvalidData("invalid seed".to_string())).c_ptr(),
    };
    let result = app_monero::transfer::sign_tx(keypair, payload);
    clear_approved_unsigned_tx();

    match result {
        Ok(data) => {
            let data = XmrTxSigned::new(data);

            UREncodeResult::encode(
                data.try_into().unwrap(),
                XmrTxSigned::get_registry_type().get_type(),
                FRAGMENT_MAX_LENGTH_DEFAULT,
            )
            .c_ptr()
        }
        Err(_) => UREncodeResult::from(RustCError::InvalidData("invalid data".to_string())).c_ptr(),
    }
}

#[no_mangle]
pub unsafe extern "C" fn monero_generate_decrypt_key(pvk: PtrString) -> *mut SimpleResponse<u8> {
    let pvk = match hex::decode(recover_c_char(pvk)) {
        Ok(pvk) if pvk.len() == PRVKEY_LEH => pvk,
        _ => return SimpleResponse::from(RustCError::InvalidMasterFingerprint).simple_c_ptr(),
    };

    let key = app_monero::utils::generate_decrypt_key(pvk.try_into().unwrap());

    SimpleResponse::success(Box::into_raw(Box::new(key)) as *mut u8).simple_c_ptr()
}

#[no_mangle]
pub unsafe extern "C" fn monero_free_decrypt_key_response(response: *mut c_void) {
    if response.is_null() {
        return;
    }

    let response = response as *mut SimpleResponse<u8>;
    let data = (*response).data;
    if !data.is_null() {
        // Same layout the allocation used: `Box<[u8; PRVKEY_LEH]>`.
        let _ = Box::from_raw(data as *mut [u8; PRVKEY_LEH]);
    }
    // The array has already been freed with its original layout. Let the
    // common response destructor release the private error message and wrapper.
    (*response).data = core::ptr::null_mut();
    crate::common::free::free_simple_response_u8(response);
}

#[no_mangle]
pub unsafe extern "C" fn get_extended_monero_pubkeys_by_seed(
    seed: PtrBytes,
    seed_len: u32,
    path: PtrString,
) -> *mut SimpleResponse<c_char> {
    let path = recover_c_char(path);
    let major = path
        .split('/')
        .nth(3)
        .unwrap()
        .replace("'", "")
        .parse::<u32>()
        .unwrap();
    let seed = extract_array!(seed, u8, seed_len as usize);
    let keypair = app_monero::key::generate_keypair(seed, major).unwrap();
    let public_spend_key = keypair.spend.get_public_key();
    let result = hex::encode(public_spend_key.as_bytes());

    SimpleResponse::success(convert_c_char(result.to_string())).simple_c_ptr()
}
