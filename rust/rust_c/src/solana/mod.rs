use crate::common::errors::RustCError;
use crate::common::structs::{SimpleResponse, TransactionCheckResult, TransactionParseResult};
use crate::common::types::{PtrBytes, PtrString, PtrT, PtrUR};
use crate::common::ur::{UREncodeResult, FRAGMENT_MAX_LENGTH_DEFAULT};
use crate::common::utils::{convert_c_char, recover_c_char};
use crate::{extract_array, extract_ptr_with_type};
use alloc::string::ToString;
use app_solana::errors::SolanaError;
use app_solana::parse_message;
use cty::c_char;
use structs::{DisplaySolanaMessage, DisplaySolanaTx};
use ur_registry::solana::sol_sign_request::{SignType, SolSignRequest};
use ur_registry::solana::sol_signature::SolSignature;
use ur_registry::traits::RegistryItem;

pub mod structs;

// Transaction parse failures must never fall back to arbitrary-message signing.
// Reuse the full display parser so known-instruction trailing bytes are rejected
// in both the initial check and the final signer, not just by the UI.
fn validated_payload(
    request: &SolSignRequest,
) -> Result<app_solana::SolanaPayloadType, SolanaError> {
    let data = request.get_sign_data();
    if request.get_sign_type() == SignType::Transaction {
        app_solana::parse(&data)?;
        return Ok(app_solana::SolanaPayloadType::Transaction);
    }
    match app_solana::classify_payload(&data) {
        app_solana::SolanaPayloadType::Message => Ok(app_solana::SolanaPayloadType::Message),
        _ => Err(SolanaError::InvalidData(
            "transaction payload cannot be signed as a message".to_string(),
        )),
    }
}

#[cfg(test)]
mod fixture_tests {
    use super::*;

    #[test]
    fn squads_no_argument_trailing_bytes_rejected_at_final_signing() {
        let seed = hex::decode("5eb00bbddcf069084889a8ab9155568165f5c453ccb85e70811aaed6f6da5fc19a5ac40b389cd370d086206dec8aa6c43daea6690f20ad3d8d48b2d2ce9e38e4").unwrap();
        let path = "m/44'/501'/0'";
        let signer = app_solana::get_public_key(&seed, &path.to_string()).unwrap();
        let program =
            bitcoin::base58::decode("SQDS4ep65T869zMMBKyuUq6aD6EgTu8psMjkvj52pCf").unwrap();
        for discriminator in ["0b225cf89a1b336a", "c208a15799a419ab"] {
            for suffix in [&[][..], &[0][..], &[1, 2, 3][..]] {
                let mut data = hex::decode(discriminator).unwrap();
                data.extend(suffix);
                let mut raw = alloc::vec![1, 0, 1, 2];
                raw.extend(&signer);
                raw.extend(&program);
                raw.extend([7; 32]);
                raw.extend([1, 1, 0, data.len() as u8]);
                raw.extend(data);
                // Valid framing ensures rejection is caused by instruction
                // semantics, not by a truncated transaction or wrong signer.
                assert!(app_solana::message::Message::read_exact(&mut raw.clone()).is_ok());
                let mut request = SolSignRequest::new(
                    None,
                    raw.clone(),
                    ur_registry::crypto_key_path::CryptoKeyPath::from_path(
                        path.to_string(),
                        Some([0x73, 0xc5, 0xda, 0x0a]),
                    )
                    .unwrap(),
                    None,
                    None,
                    SignType::Transaction,
                );
                assert_eq!(app_solana::parse(&raw).is_ok(), suffix.is_empty());
                unsafe {
                    let result = build_sign_result(&mut request as *mut _ as PtrUR, &seed);
                    if suffix.is_empty() {
                        assert!(result.is_ok());
                    } else {
                        assert!(matches!(result, Err(SolanaError::ProgramError(_))));
                    }
                }
            }
        }
    }

    #[test]
    fn v1_security_checks_apply_at_final_signing() {
        let seed = hex::decode("5eb00bbddcf069084889a8ab9155568165f5c453ccb85e70811aaed6f6da5fc19a5ac40b389cd370d086206dec8aa6c43daea6690f20ad3d8d48b2d2ce9e38e4").unwrap();
        let dataset: serde_json::Value =
            serde_json::from_str(include_str!("../../tests/fixtures/solana-security.json"))
                .unwrap();
        let mut count = 0;
        for fixture in dataset["fixtures"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|f| f["id"].as_str().unwrap().starts_with("v1--"))
        {
            count += 1;
            let mut decoder = ur::Decoder::default();
            for frame in fixture["ur"].as_array().unwrap() {
                decoder
                    .receive(&frame.as_str().unwrap().to_lowercase())
                    .unwrap();
            }
            let mut request =
                SolSignRequest::try_from(decoder.message().unwrap().unwrap()).unwrap();
            let expected = fixture["expected_result"] == "success";
            unsafe {
                assert_eq!(
                    build_sign_result(&mut request as *mut _ as PtrUR, &seed).is_ok(),
                    expected,
                    "{}",
                    fixture["id"]
                );
            }
            request.set_sign_type(SignType::Message);
            assert!(validated_payload(&request).is_err(), "{}", fixture["id"]);
            // Exercise both exported UI entry points with the wrong type hint.
            unsafe {
                use crate::common::free::Free;
                let ptr = &mut request as *mut _ as PtrUR;
                let signer = app_solana::message::Message::read_exact(&mut request.get_sign_data())
                    .ok()
                    .and_then(|message| {
                        message
                            .accounts
                            .first()
                            .map(|account| hex::encode(&account.value))
                    })
                    .unwrap_or_else(|| "00".repeat(32));
                let pubkey = cstr_core::CString::new(signer).unwrap();
                if expected {
                    request.set_sign_type(SignType::Transaction);
                    for result in [
                        solana_parse_tx(ptr),
                        solana_parse_tx_with_pubkey(ptr, pubkey.as_ptr() as PtrString),
                    ] {
                        assert!(!(*(result as *const *mut DisplaySolanaTx)).is_null());
                        let result = alloc::boxed::Box::from_raw(result);
                        result.free();
                    }
                    request.set_sign_type(SignType::Message);
                }
                for result in [
                    solana_parse_tx(ptr),
                    solana_parse_tx_with_pubkey(ptr, pubkey.as_ptr() as PtrString),
                ] {
                    // TransactionParseResult is repr(C), with data first.
                    assert!((*(result as *const *mut DisplaySolanaTx)).is_null());
                    let result = alloc::boxed::Box::from_raw(result);
                    result.free();
                }
            }
            unsafe {
                assert!(build_sign_result(&mut request as *mut _ as PtrUR, &seed).is_err());
            }
            if fixture["id"] == "v1--transfer" {
                request.set_sign_type(SignType::Transaction);
                let mut data = request.get_sign_data();
                // This fixture has three addresses, all five config words,
                // and one System transfer. Increase its declared data size
                // and append a hidden byte: framing stays valid.
                assert_eq!(&data[158..162], &[2, 2, 12, 0]);
                data[160] = 13;
                data.push(0);
                assert!(app_solana::message::Message::read_exact(&mut data.clone()).is_ok());
                request.set_sign_data(data);
                assert!(validated_payload(&request).is_err());
                unsafe {
                    assert!(build_sign_result(&mut request as *mut _ as PtrUR, &seed).is_err());
                }
            }
        }
        assert_eq!(count, 6);
    }

    #[test]
    fn canonical_display_regressions_preserve_order_alt_and_message_length() {
        use crate::common::free::Free;
        use app_solana::parser::{overview::SolanaOverview, structs::OverviewAction};
        let dataset: serde_json::Value =
            serde_json::from_str(include_str!("../../tests/fixtures/solana-security.json"))
                .unwrap();
        for id in ["04", "05", "11", "14", "22"] {
            let fixture = dataset["fixtures"]
                .as_array()
                .unwrap()
                .iter()
                .find(|f| f["id"] == alloc::format!("transaction-security--{id}"))
                .unwrap();
            let mut decoder = ur::Decoder::default();
            for frame in fixture["ur"].as_array().unwrap() {
                decoder
                    .receive(&frame.as_str().unwrap().to_lowercase())
                    .unwrap();
            }
            let request = SolSignRequest::try_from(decoder.message().unwrap().unwrap()).unwrap();
            let data = request.get_sign_data();
            if id == "14" {
                assert_eq!(data.len(), 961);
                let message =
                    app_solana::parse_message(data, &alloc::string::String::new()).unwrap();
                assert_eq!(message.message_length, 961);
                assert_eq!(message.raw_message.len(), 1922);
                continue;
            }
            let parsed = app_solana::parse(&data).unwrap();
            let detail: serde_json::Value = serde_json::from_str(&parsed.detail).unwrap();
            assert!(parsed.detail.starts_with("{\n") || parsed.detail.starts_with("[\n"));
            assert_ne!(parsed.detail, serde_json::to_string(&detail).unwrap());
            if id == "22" {
                let instructions = detail["instructions"].as_array().unwrap();
                assert!(instructions.iter().any(|instruction| {
                    let bytes =
                        bitcoin::base58::decode(instruction["data"].as_str().unwrap()).unwrap();
                    bytes
                        .windows(b"SQUADS-DETAILS-END".len())
                        .any(|w| w == b"SQUADS-DETAILS-END")
                }));
                continue;
            }
            if id == "11" {
                assert_eq!(
                    detail["address_table_lookups"][0]["account_key"],
                    "7tj9biW3KRJ7EEWmVUGigHiouCTXhV2dzcyvwma7Cyu7"
                );
                assert!(detail["address_table_lookups"][0]["writable_indexes"]
                    .as_array()
                    .unwrap()
                    .contains(&serde_json::json!(7)));
                let SolanaOverview::Transfer(transfer) = &parsed.overview else {
                    panic!("ALT transfer should retain its recognized layout")
                };
                assert_eq!(transfer.value, "0.001 SOL");
                assert!(transfer.to_in_lookup_table);
                assert_eq!(
                    transfer.to,
                    "Table: 7tj9biW3KRJ7EEWmVUGigHiouCTXhV2dzcyvwma7Cyu7\nIndex: 7"
                );
            } else {
                let expected = if id == "04" {
                    alloc::vec![
                        (1, OverviewAction::Unknown(0)),
                        (2, OverviewAction::Primary)
                    ]
                } else {
                    alloc::vec![
                        (1, OverviewAction::Primary),
                        (2, OverviewAction::Unknown(0))
                    ]
                };
                assert_eq!(parsed.overview_order, expected);
                let SolanaOverview::Transfer(transfer) = &parsed.overview else {
                    panic!("retain transfer amounts")
                };
                assert!(!transfer.value.is_empty());
            }
            let display = structs::DisplaySolanaTx::from(parsed);
            unsafe {
                let overview = &*display.overview;
                if id == "11" {
                    assert!(overview.unknown_instructions.is_null());
                    assert!(overview.transfer_to_in_lookup_table);
                } else {
                    let actions = &*overview.overview_order;
                    assert_eq!(actions.size, 2);
                    assert_eq!((*actions.data).kind, if id == "04" { 2 } else { 0 });
                    assert_eq!((*overview.additional_unknown_programs).size, 1);
                }
                display.free();
            }
        }
    }

    #[test]
    fn canonical_fixtures_check_and_sign() {
        let seed = hex::decode("5eb00bbddcf069084889a8ab9155568165f5c453ccb85e70811aaed6f6da5fc19a5ac40b389cd370d086206dec8aa6c43daea6690f20ad3d8d48b2d2ce9e38e4").unwrap();
        let mut fingerprint = [0x73, 0xc5, 0xda, 0x0a];
        let dataset: serde_json::Value =
            serde_json::from_str(include_str!("../../tests/fixtures/solana-security.json"))
                .unwrap();
        for fixture in dataset["fixtures"].as_array().unwrap() {
            if !["01", "02", "07", "08", "13", "14", "23", "24"]
                .iter()
                .any(|id| {
                    fixture["id"]
                        .as_str()
                        .unwrap()
                        .ends_with(&alloc::format!("--{id}"))
                })
            {
                continue;
            }
            let expected = fixture["expected_result"] == "success";
            let mut decoder = ur::Decoder::default();
            for frame in fixture["ur"].as_array().unwrap() {
                decoder
                    .receive(&frame.as_str().unwrap().to_lowercase())
                    .unwrap();
            }
            let mut request =
                SolSignRequest::try_from(decoder.message().unwrap().unwrap()).unwrap();
            unsafe {
                let ptr = &mut request as *mut _ as PtrUR;
                let checked = solana_check(ptr, fingerprint.as_mut_ptr(), 4);
                let accepted = *(checked as *const u32) == 0;
                crate::common::structs::free_TransactionCheckResult(checked);
                assert_eq!(accepted, expected, "check {}", fixture["id"]);
                assert_eq!(
                    build_sign_result(ptr, &seed).is_ok(),
                    expected,
                    "sign {}",
                    fixture["id"]
                );
                if expected {
                    // Keep the request valid and change only the device fingerprint,
                    // so rejection cannot be caused by payload or path validation.
                    let mut wrong_fingerprint = fingerprint;
                    wrong_fingerprint[0] ^= 1;
                    let checked = solana_check(ptr, wrong_fingerprint.as_mut_ptr(), 4);
                    let error_code = *(checked as *const u32);
                    crate::common::structs::free_TransactionCheckResult(checked);
                    assert_eq!(
                        error_code,
                        crate::common::errors::ErrorCodes::MasterFingerprintMismatch as u32,
                        "fingerprint mismatch {}",
                        fixture["id"]
                    );
                    let original_path = request.get_derivation_path();
                    for source in [Some(wrong_fingerprint), None] {
                        request.set_derivation_path(
                            ur_registry::crypto_key_path::CryptoKeyPath::new(
                                original_path.get_components(),
                                source,
                                original_path.get_depth(),
                            ),
                        );
                        assert!(matches!(build_sign_result(ptr, &seed),
                            Err(SolanaError::InvalidData(message)) if message == "master fingerprint mismatch"));
                    }
                    request.set_derivation_path(original_path);
                    assert!(build_sign_result(ptr, &seed).is_ok());
                    let checked = solana_check(ptr, core::ptr::null_mut(), 4);
                    assert_eq!(
                        *(checked as *const u32),
                        crate::common::errors::ErrorCodes::InvalidMasterFingerprint as u32
                    );
                    crate::common::structs::free_TransactionCheckResult(checked);
                    // Changing the host's type hint must not turn a transaction
                    // into a message signature (or plain text into a transaction).
                    request.set_sign_type(if request.get_sign_type() == SignType::Transaction {
                        SignType::Message
                    } else {
                        SignType::Transaction
                    });
                    assert!(
                        validated_payload(&request).is_err(),
                        "type mismatch {}",
                        fixture["id"]
                    );
                    assert!(
                        build_sign_result(ptr, &seed).is_err(),
                        "sign type mismatch {}",
                        fixture["id"]
                    );
                }
            }
        }
    }
}

unsafe fn build_sign_result(ptr: PtrUR, seed: &[u8]) -> Result<SolSignature, SolanaError> {
    let sign_request = extract_ptr_with_type!(ptr, SolSignRequest);
    let sign_data = sign_request.get_sign_data();
    let is_complete_transaction = match validated_payload(sign_request)? {
        app_solana::SolanaPayloadType::Transaction => true,
        app_solana::SolanaPayloadType::Message => false,
        app_solana::SolanaPayloadType::MalformedTransaction => {
            return Err(SolanaError::InvalidData(
                "invalid or unsupported transaction data".to_string(),
            ));
        }
    };
    // Derive ownership from the seed actually used to sign, not host metadata.
    let fingerprint = keystore::algorithms::secp256k1::get_master_fingerprint_by_seed(seed)?;
    if sign_request.get_derivation_path().get_source_fingerprint() != Some(*fingerprint.as_bytes())
    {
        return Err(SolanaError::InvalidData(
            "master fingerprint mismatch".to_string(),
        ));
    }
    let path = sign_request
        .get_derivation_path()
        .get_path()
        .ok_or(SolanaError::InvalidData(
            "invalid derivation path".to_string(),
        ))?;
    let path = app_solana::validate_derivation_path(&path)?;
    if is_complete_transaction {
        let signer = app_solana::get_public_key(seed, &path)?;
        app_solana::validate_tx_signer(&mut sign_data.clone(), &signer)?;
    }
    let signature = app_solana::sign(sign_data, &path, seed)?;
    Ok(SolSignature::new(
        sign_request.get_request_id(),
        signature.to_vec(),
    ))
}

#[no_mangle]
pub unsafe extern "C" fn solana_get_address(pubkey: PtrString) -> *mut SimpleResponse<c_char> {
    let x_pub = recover_c_char(pubkey);
    let address = app_solana::get_address(&x_pub);
    match address {
        Ok(result) => SimpleResponse::success(convert_c_char(result) as *mut c_char).simple_c_ptr(),
        Err(e) => SimpleResponse::from(e).simple_c_ptr(),
    }
}

#[no_mangle]
pub unsafe extern "C" fn solana_check(
    ptr: PtrUR,
    master_fingerprint: PtrBytes,
    length: u32,
) -> PtrT<TransactionCheckResult> {
    if length != 4 || master_fingerprint.is_null() {
        return TransactionCheckResult::from(RustCError::InvalidMasterFingerprint).c_ptr();
    }
    let sol_sign_request = extract_ptr_with_type!(ptr, SolSignRequest);
    if let Err(error) = validated_payload(sol_sign_request) {
        return TransactionCheckResult::from(error).c_ptr();
    }
    let derivation_path: ur_registry::crypto_key_path::CryptoKeyPath =
        sol_sign_request.get_derivation_path();
    let path = match derivation_path.get_path() {
        Some(path) => path,
        None => return TransactionCheckResult::from(RustCError::InvalidHDPath).c_ptr(),
    };
    if app_solana::validate_derivation_path(&path).is_err() {
        return TransactionCheckResult::from(RustCError::InvalidHDPath).c_ptr();
    }
    let mfp = extract_array!(master_fingerprint, u8, 4);
    if let Ok(mfp) = (mfp.try_into() as Result<[u8; 4], _>) {
        if let Some(ur_mfp) = derivation_path.get_source_fingerprint() {
            return if mfp == ur_mfp {
                TransactionCheckResult::new().c_ptr()
            } else {
                TransactionCheckResult::from(RustCError::MasterFingerprintMismatch).c_ptr()
            };
        }
        return TransactionCheckResult::from(RustCError::MasterFingerprintMismatch).c_ptr();
    };
    TransactionCheckResult::from(RustCError::InvalidMasterFingerprint).c_ptr()
}

#[no_mangle]
pub unsafe extern "C" fn solana_parse_tx(
    ptr: PtrUR,
) -> PtrT<TransactionParseResult<DisplaySolanaTx>> {
    let solan_sign_reqeust = extract_ptr_with_type!(ptr, SolSignRequest);
    let tx_hex = solan_sign_reqeust.get_sign_data();
    match validated_payload(solan_sign_reqeust) {
        Ok(app_solana::SolanaPayloadType::Transaction) => {}
        Ok(_) => {
            return TransactionParseResult::from(SolanaError::InvalidData(
                "expected a transaction request".to_string(),
            ))
            .c_ptr()
        }
        Err(error) => return TransactionParseResult::from(error).c_ptr(),
    }
    match app_solana::parse(&tx_hex.to_vec()) {
        Ok(v) => TransactionParseResult::success(DisplaySolanaTx::from(v).c_ptr()).c_ptr(),
        Err(e) => TransactionParseResult::from(e).c_ptr(),
    }
}

#[no_mangle]
pub unsafe extern "C" fn solana_parse_tx_with_pubkey(
    ptr: PtrUR,
    pubkey: PtrString,
) -> PtrT<TransactionParseResult<DisplaySolanaTx>> {
    let solan_sign_reqeust = extract_ptr_with_type!(ptr, SolSignRequest);
    let tx_hex = solan_sign_reqeust.get_sign_data();
    match validated_payload(solan_sign_reqeust) {
        Ok(app_solana::SolanaPayloadType::Transaction) => {}
        Ok(_) => {
            return TransactionParseResult::from(SolanaError::InvalidData(
                "expected a transaction request".to_string(),
            ))
            .c_ptr()
        }
        Err(error) => return TransactionParseResult::from(error).c_ptr(),
    }
    let pubkey = recover_c_char(pubkey);
    let signer: [u8; 32] = match hex::decode(pubkey)
        .ok()
        .and_then(|value| value.try_into().ok())
    {
        Some(value) => value,
        None => {
            return TransactionParseResult::from(SolanaError::InvalidData(
                "invalid Solana signer public key".to_string(),
            ))
            .c_ptr()
        }
    };
    match app_solana::parse_for_signer(&tx_hex.to_vec(), &signer) {
        Ok(v) => TransactionParseResult::success(DisplaySolanaTx::from(v).c_ptr()).c_ptr(),
        Err(e) => TransactionParseResult::from(e).c_ptr(),
    }
}

#[no_mangle]
// this function is used to sign the tx and message
pub unsafe extern "C" fn solana_sign_tx(
    ptr: PtrUR,
    seed: PtrBytes,
    seed_len: u32,
) -> PtrT<UREncodeResult> {
    let seed = extract_array!(seed, u8, seed_len as usize);
    build_sign_result(ptr, seed)
        .map(|v| v.try_into())
        .map_or_else(
            |e| UREncodeResult::from(e).c_ptr(),
            |v| {
                v.map_or_else(
                    |e| UREncodeResult::from(e).c_ptr(),
                    |data| {
                        UREncodeResult::encode(
                            data,
                            SolSignature::get_registry_type().get_type(),
                            FRAGMENT_MAX_LENGTH_DEFAULT,
                        )
                        .c_ptr()
                    },
                )
            },
        )
}

#[no_mangle]
pub unsafe extern "C" fn solana_parse_message(
    ptr: PtrUR,
    pubkey: PtrString,
) -> PtrT<TransactionParseResult<DisplaySolanaMessage>> {
    let sol_sign_request = extract_ptr_with_type!(ptr, SolSignRequest);
    let pubkey = recover_c_char(pubkey);
    match validated_payload(sol_sign_request) {
        Err(error) => return TransactionParseResult::from(error).c_ptr(),
        Ok(app_solana::SolanaPayloadType::Message) => {}
        Ok(app_solana::SolanaPayloadType::Transaction) => {
            return TransactionParseResult::from(RustCError::UnsupportedTransaction(
                "Transaction".to_string(),
            ))
            .c_ptr();
        }
        Ok(app_solana::SolanaPayloadType::MalformedTransaction) => {
            return TransactionParseResult::from(SolanaError::InvalidData(
                "invalid or unsupported transaction data".to_string(),
            ))
            .c_ptr();
        }
    }
    match parse_message(sol_sign_request.get_sign_data(), &pubkey.to_string()) {
        Ok(t) => TransactionParseResult::success(DisplaySolanaMessage::from(t).c_ptr()).c_ptr(),
        Err(e) => TransactionParseResult::from(e).c_ptr(),
    }
}

#[no_mangle]
pub unsafe extern "C" fn sol_get_path(ptr: PtrUR) -> PtrString {
    let sol_sign_request = extract_ptr_with_type!(ptr, SolSignRequest);
    let derivation_path = sol_sign_request.get_derivation_path();
    if let Some(path) = derivation_path.get_path() {
        return convert_c_char(path);
    }
    convert_c_char("".to_string())
}
