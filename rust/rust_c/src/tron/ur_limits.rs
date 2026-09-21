use alloc::string::ToString;
use ur_registry::error::URError;

// Room for the 16 KiB sign_data plus CBOR metadata. These are device resource
// limits, not TRON consensus limits. Other UR types retain their own policies.
pub(crate) const MAX_UR_BYTES: usize = app_tron::MAX_SIGN_DATA_LEN + 1024;
const MAX_PARTS: usize = 128;
const MAX_WORK: usize = 192 * 1024;

fn error() -> URError {
    URError::UrDecodeError("TRON UR exceeds device limits or has invalid fragment metadata".into())
}

pub(crate) fn is_tron(value: &str) -> bool {
    let prefix = "ur:tron-sign-request/";
    value
        .get(..prefix.len())
        .is_some_and(|s| s.eq_ignore_ascii_case(prefix))
}

pub(crate) fn validate_text_len(value: &str) -> Result<(), URError> {
    // Bytewords uses two characters per byte; leave room for URI and checksum.
    if is_tron(value) && value.len() > MAX_UR_BYTES * 2 + 128 {
        return Err(error());
    }
    Ok(())
}

pub(crate) fn validate_single_len(len: usize) -> Result<(), URError> {
    if len > MAX_UR_BYTES {
        return Err(error());
    }
    Ok(())
}

#[derive(Default)]
pub(crate) struct FragmentBudget {
    work: usize,
}

impl FragmentBudget {
    pub(crate) fn receive(&mut self, value: &str) -> Result<(), URError> {
        if !is_tron(value) {
            return Err(error());
        }
        validate_text_len(value)?;
        let (kind, cbor) = ur::decode(value).map_err(|e| URError::UrDecodeError(e.to_string()))?;
        if kind != ur::ur::Kind::MultiPart {
            return Err(error());
        }
        let mut d = minicbor::Decoder::new(&cbor);
        let mut read = || -> Result<(usize, usize), minicbor::decode::Error> {
            if d.array()? != Some(5) {
                return Err(minicbor::decode::Error::message("invalid fountain part"));
            }
            let sequence = d.u32()?;
            let parts = d.u32()? as usize;
            let message_len = d.u32()? as usize;
            let _checksum = d.u32()?;
            let fragment_len = d.bytes()?.len();
            if sequence == 0
                || parts == 0
                || parts > MAX_PARTS
                || message_len == 0
                || message_len > MAX_UR_BYTES
                || fragment_len == 0
                || fragment_len > MAX_UR_BYTES
                || d.position() != cbor.len()
                || message_len > parts * fragment_len
                || message_len <= (parts - 1) * fragment_len
            {
                return Err(minicbor::decode::Error::message("invalid fountain limits"));
            }
            Ok((parts, fragment_len))
        };
        let (parts, bytes) = read().map_err(|_| error())?;
        // Bound accumulated fragments and index sets, including redundant parts.
        // Charge 8 bytes per index even on the 32-bit device. The decoder can
        // retain several copies; this is a work budget, not a measurement of peak heap use.
        self.work = self
            .work
            .checked_add(bytes + parts * 8 + 128)
            .ok_or_else(error)?;
        if self.work > MAX_WORK {
            return Err(error());
        }
        Ok(())
    }
}
