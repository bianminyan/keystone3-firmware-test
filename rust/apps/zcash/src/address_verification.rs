//! Verifies every receiver of an external Unified Address against one account.
use zcash_vendor::{
    zcash_keys::{
        address::Address,
        keys::{ReceiverRequirement, UnifiedAddressRequest, UnifiedFullViewingKey},
    },
    zcash_protocol::consensus,
    zip32::DiversifierIndex,
};

#[derive(Debug, PartialEq, Eq)]
pub enum VerificationResult {
    Owned,
    NotOwned,
    InvalidAddress,
    UnsupportedReceiver,
    KeyUnavailable,
}

pub fn verify<P: consensus::Parameters>(params: &P, ufvk: &str, text: &str) -> VerificationResult {
    use VerificationResult::*;
    // Network-aware decoding also checks the encoding and receiver structure.
    let ua = match Address::decode(params, text) {
        Some(Address::Unified(ua)) => ua,
        Some(_) => return UnsupportedReceiver,
        None => return InvalidAddress,
    };
    // Do not certify a UA containing components we cannot verify, including Sapling.
    if !ua.unknown().is_empty() || ua.has_sapling() {
        return UnsupportedReceiver;
    }
    let receiver = match ua.orchard() {
        Some(receiver) => receiver,
        None => return UnsupportedReceiver,
    };
    let ufvk = match UnifiedFullViewingKey::decode(params, ufvk) {
        Ok(ufvk) => ufvk,
        Err(_) => return KeyUnavailable,
    };
    // Derive external incoming viewing keys once, then reuse them for both checks.
    let incoming = ufvk.to_unified_incoming_viewing_key();
    let orchard = match incoming.orchard().as_ref() {
        Some(orchard) => orchard,
        None => return KeyUnavailable,
    };
    let index = match orchard.diversifier_index(receiver) {
        Some(index) => DiversifierIndex::from(*index.as_bytes()),
        None => return NotOwned,
    };
    // Require exactly the supported receivers present in the scanned UA. In
    // particular, Orchard-only addresses may use indices outside the transparent
    // child-index range. Never search forward to a different diversifier.
    let request = UnifiedAddressRequest::custom(
        ReceiverRequirement::Require,
        ReceiverRequirement::Omit,
        if ua.has_transparent() {
            ReceiverRequirement::Require
        } else {
            ReceiverRequirement::Omit
        },
    )
    .expect("Orchard is required");
    match incoming.address(index, request) {
        Ok(expected)
            if expected.orchard() == ua.orchard() && expected.transparent() == ua.transparent() =>
        {
            Owned
        }
        _ => NotOwned,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use alloc::{vec, vec::Vec};
    use keystore::algorithms::zcash::derive_ufvk;
    use zcash_vendor::zip32::Scope;
    use zcash_vendor::{
        zcash_address::unified::{self, Container, Encoding, Receiver},
        zcash_protocol::consensus::{MainNetwork, NetworkType, TestNetwork},
    };

    fn key(seed: u8) -> UnifiedFullViewingKey {
        UnifiedFullViewingKey::decode(
            &MainNetwork,
            &derive_ufvk(&MainNetwork, &[seed; 32], "m/32'/133'/0'").unwrap(),
        )
        .unwrap()
    }

    fn check(key: &UnifiedFullViewingKey, receivers: Vec<Receiver>) -> VerificationResult {
        let text = unified::Address::try_from_items(receivers)
            .unwrap()
            .encode(&NetworkType::Main);
        verify(&MainNetwork, &key.encode(&MainNetwork), &text)
    }

    #[test]
    fn default_and_rotated_addresses_are_owned() {
        let key = key(1);
        for i in [0u32, 1, 42, 1729704000] {
            let address = key
                .address(i.into(), UnifiedAddressRequest::AllAvailableKeys)
                .unwrap();
            assert_eq!(
                verify(
                    &MainNetwork,
                    &key.encode(&MainNetwork),
                    &address.encode(&MainNetwork)
                ),
                VerificationResult::Owned
            );
        }
    }

    #[test]
    fn orchard_only_supports_full_diversifier_range() {
        let key = key(1);
        let index = DiversifierIndex::from([0xff; 11]);
        let address = key.address(index, UnifiedAddressRequest::ORCHARD).unwrap();
        assert_eq!(
            verify(
                &MainNetwork,
                &key.encode(&MainNetwork),
                &address.encode(&MainNetwork)
            ),
            VerificationResult::Owned
        );
    }

    #[test]
    fn other_wallet_and_internal_address_are_not_owned() {
        let key = key(1);
        let other = key
            .address(0u32.into(), UnifiedAddressRequest::AllAvailableKeys)
            .unwrap();
        assert_eq!(
            verify(
                &MainNetwork,
                &self::key(2).encode(&MainNetwork),
                &other.encode(&MainNetwork)
            ),
            VerificationResult::NotOwned
        );
        let internal = key
            .orchard()
            .unwrap()
            .to_ivk(Scope::Internal)
            .address_at(0u32);
        assert_eq!(
            check(
                &key,
                vec![Receiver::Orchard(internal.to_raw_address_bytes())]
            ),
            VerificationResult::NotOwned
        );
    }

    #[test]
    fn mixed_receivers_and_wrong_transparent_index_are_rejected() {
        let key = key(1);
        let ua = key
            .address(0u32.into(), UnifiedAddressRequest::AllAvailableKeys)
            .unwrap();
        let orchard = Receiver::Orchard(ua.orchard().unwrap().to_raw_address_bytes());
        for other in [
            self::key(2)
                .address(0u32.into(), UnifiedAddressRequest::AllAvailableKeys)
                .unwrap(),
            key.address(1u32.into(), UnifiedAddressRequest::AllAvailableKeys)
                .unwrap(),
        ] {
            let (_, raw) = unified::Address::decode(&other.encode(&MainNetwork)).unwrap();
            let transparent = raw
                .items_as_parsed()
                .iter()
                .find(|r| matches!(r, Receiver::P2pkh(_)))
                .unwrap()
                .clone();
            assert_eq!(
                check(&key, vec![orchard.clone(), transparent]),
                VerificationResult::NotOwned
            );
        }
        assert_eq!(
            check(&key, vec![orchard, Receiver::P2sh([1; 20])]),
            VerificationResult::NotOwned
        );
    }

    #[test]
    fn unverified_receivers_never_report_success() {
        let key = key(1);
        let ua = key
            .address(0u32.into(), UnifiedAddressRequest::ORCHARD)
            .unwrap();
        let orchard = Receiver::Orchard(ua.orchard().unwrap().to_raw_address_bytes());
        assert_eq!(
            check(
                &key,
                vec![
                    orchard.clone(),
                    Receiver::Unknown {
                        typecode: 65535,
                        data: vec![1; 20]
                    }
                ]
            ),
            VerificationResult::UnsupportedReceiver
        );
        // Sapling is opaque when that feature is disabled, and must stay unverified.
        assert_eq!(
            check(&key, vec![orchard, Receiver::Sapling([0; 43])]),
            VerificationResult::UnsupportedReceiver
        );
        assert_ne!(
            check(&key, vec![Receiver::Sapling([0; 43])]),
            VerificationResult::Owned
        );
    }

    #[test]
    fn invalid_encoding_network_and_viewing_key_are_rejected() {
        let key = key(1);
        let ua = key
            .address(0u32.into(), UnifiedAddressRequest::ORCHARD)
            .unwrap();
        let ufvk = key.encode(&MainNetwork);
        for text in ["", "u1invalid", "zcash:u1invalid"] {
            assert_eq!(
                verify(&MainNetwork, &ufvk, text),
                VerificationResult::InvalidAddress
            );
        }
        assert_eq!(
            verify(&MainNetwork, &ufvk, &ua.encode(&TestNetwork)),
            VerificationResult::InvalidAddress
        );
        assert_eq!(
            verify(&MainNetwork, "", &ua.encode(&MainNetwork)),
            VerificationResult::KeyUnavailable
        );
        let mut corrupt = ua.encode(&MainNetwork).into_bytes();
        let last = corrupt.len() - 1;
        corrupt[last] = if corrupt[last] == b'q' { b'p' } else { b'q' };
        assert_eq!(
            verify(&MainNetwork, &ufvk, core::str::from_utf8(&corrupt).unwrap()),
            VerificationResult::InvalidAddress
        );
    }
}
