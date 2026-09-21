use alloc::{format, string::ToString};
use seed_signer_message::{MessageEncoding, SeedSignerMessage};

pub mod seed_signer_message;

use super::{
    errors::RustCError,
    types::{Ptr, PtrString},
    ur::URParseResult,
    utils::check_recover_c_char_lossy,
};

#[repr(C)]
pub enum QRProtocol {
    QRCodeTypeText,
    QRCodeTypeUR,
}

#[no_mangle]
pub unsafe extern "C" fn infer_qrcode_type(qrcode: PtrString) -> QRProtocol {
    let (_, value) = check_recover_c_char_lossy(qrcode);
    if value.to_uppercase().starts_with("UR:") {
        QRProtocol::QRCodeTypeUR
    } else {
        QRProtocol::QRCodeTypeText
    }
}

#[no_mangle]
pub unsafe extern "C" fn parse_qrcode_text(qr: PtrString) -> Ptr<URParseResult> {
    let (is_ok, value) = check_recover_c_char_lossy(qr);
    if !is_ok {
        return URParseResult::from(RustCError::InvalidMessage).c_ptr();
    }
    #[cfg(feature = "cypherpunk")]
    if value.starts_with("u1") || value.starts_with("utest1") || value.starts_with("uregtest1") {
        // Bound retained QR text before address decoding. No URI
        // stripping: a payment request is not an address verification request.
        if value.len() > 1024 {
            return URParseResult::from(RustCError::InvalidMessage).c_ptr();
        }
        if let Ok(address) = cstr_core::CString::new(value.as_bytes()) {
            return URParseResult::single(
                super::ur::ViewType::ZcashAddressCheck,
                super::ur::QRCodeType::ZcashAddressText,
                address,
            )
            .c_ptr();
        }
        return URParseResult::from(RustCError::InvalidMessage).c_ptr();
    }
    if value.to_lowercase().starts_with("signmessage") {
        if let Some((headers, message)) = value.split_once(':') {
            if message.is_empty() {
                return URParseResult::from(RustCError::InvalidMessage).c_ptr();
            }
            let mut pieces = headers.split_ascii_whitespace();
            let _ = pieces.next(); //drop "signmessage"
            let path = pieces.next();
            let encode = pieces.next();
            if let (Some(encode), Some(path)) = (encode, path) {
                match encode {
                    "ascii" => {
                        let data = SeedSignerMessage::new(
                            path.replace('h', "'").to_string(),
                            message.to_string(),
                            MessageEncoding::ASCII,
                        );
                        return URParseResult::single(
                            super::ur::ViewType::BtcMsg,
                            super::ur::QRCodeType::SeedSignerMessage,
                            data,
                        )
                        .c_ptr();
                    }
                    _ => return URParseResult::from(RustCError::InvalidMessage).c_ptr(),
                }
            }
        }
        return URParseResult::from(RustCError::InvalidMessage).c_ptr();
    }
    URParseResult::from(RustCError::InvalidMessage).c_ptr()
}
