//! The UR coin ID is authoritative for all PSBT Extend approval/signing stages.
use crate::{
    errors::{BitcoinError, Result},
    network::Network,
};
use alloc::string::ToString;
use bitcoin::{
    bip32::{ChildNumber, DerivationPath},
    psbt::Psbt,
    NetworkKind,
};

fn invalid() -> BitcoinError {
    BitcoinError::InvalidPsbt("PSBT metadata conflicts with the PSBT Extend coin ID".to_string())
}

pub(crate) fn validate(psbt: &Psbt, coin_id: Option<i128>) -> Result<Option<Network>> {
    let (coin, network) = match coin_id {
        None | Some(0) => return Ok(None),
        Some(2) => (2, Network::Litecoin),
        Some(3) => (3, Network::Dogecoin),
        Some(5) => (5, Network::Dash),
        Some(145) => (145, Network::BitcoinCash),
        _ => return Err(invalid()),
    };
    // Even unrecognized chain markers must not override the approved network.
    if psbt.unknown.keys().any(|key| key.key == b"chain") {
        return Err(invalid());
    }
    let check_path = |path: &DerivationPath, account: bool| -> Result<()> {
        let parts = path.as_ref();
        if parts.len() != if account { 3 } else { 5 } {
            return Err(invalid());
        }
        let purpose_ok = match parts[0] {
            ChildNumber::Hardened { index } => {
                if coin == 2 {
                    matches!(index, 49 | 84)
                } else {
                    index == 44
                }
            }
            _ => false,
        };
        if !purpose_ok
            || parts[1] != (ChildNumber::Hardened { index: coin })
            || !parts[2].is_hardened()
            || parts[3..].iter().any(|part| part.is_hardened())
        {
            return Err(invalid());
        }
        Ok(())
    };
    // Mainnet xpub serialization is shared by these chains; validate the origin
    // coin instead of incorrectly treating every mainnet xpub as Bitcoin.
    for (xpub, (_, path)) in &psbt.xpub {
        if xpub.network != NetworkKind::Main {
            return Err(invalid());
        }
        check_path(path, true)?;
    }
    for input in &psbt.inputs {
        for (_, path) in input.bip32_derivation.values() {
            check_path(path, false)?;
        }
        if !input.tap_key_origins.is_empty()
            || input.tap_internal_key.is_some()
            || input.tap_merkle_root.is_some()
            || !input.tap_scripts.is_empty()
            || !input.tap_script_sigs.is_empty()
            || input.tap_key_sig.is_some()
        {
            return Err(invalid());
        }
    }
    for output in &psbt.outputs {
        for (_, path) in output.bip32_derivation.values() {
            check_path(path, false)?;
        }
        if !output.tap_key_origins.is_empty()
            || output.tap_internal_key.is_some()
            || output.tap_tree.is_some()
        {
            return Err(invalid());
        }
    }
    if coin == 145 {
        super::bch::validate_outputs(psbt)?;
    }
    Ok(Some(network))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{addresses::address::Address, parsed_tx::ParseContext};
    use alloc::{collections::BTreeMap, vec::Vec};
    use bitcoin::{
        bip32::{Fingerprint, Xpub},
        ScriptBuf,
    };
    use core::str::FromStr;
    fn seed() -> Vec<u8> {
        hex::decode("5eb00bbddcf069084889a8ab9155568165f5c453ccb85e70811aaed6f6da5fc19a5ac40b389cd370d086206dec8aa6c43daea6690f20ad3d8d48b2d2ce9e38e4").unwrap()
    }
    fn fp() -> Fingerprint {
        Fingerprint::from_str("73c5da0a").unwrap()
    }
    fn account(coin: i128) -> (DerivationPath, Xpub) {
        let purpose = if coin == 2 { 84 } else { 44 };
        let path = DerivationPath::from_str(&format!("m/{purpose}'/{coin}'/0'")).unwrap();
        let xpub = keystore::algorithms::secp256k1::get_extended_public_key_by_seed(
            &seed(),
            &path.to_string(),
        )
        .unwrap();
        (path, xpub)
    }
    fn context(coin: i128) -> ParseContext {
        let (path, xpub) = account(coin);
        ParseContext::new(fp(), BTreeMap::from([(path, xpub)]), None, None)
    }
    fn fixture(coin: i128) -> Psbt {
        let data = match coin {
            2 => "70736274ff01007101000000011bfed2d08a2b45c790930953230bdc406eca13e579b5049eb751086b3ed369420000000000ffffffff02005a62020000000016001477f2208e272bef80e98ce956e05d5ed8f596f5a3f05f930300000000160014267f2c10ac39e483c00c982efdd0112bcc3e62830000000000010054010000000130297791cbb43ddbe1b17406f92f79b653a585f061213749fb069644b7f7ffbb00000000020101ffffffff0100e1f5050000000016001496cd3178f3de0f307bcee1aeaa4d5987862dd30a0000000001011f00e1f5050000000016001496cd3178f3de0f307bcee1aeaa4d5987862dd30a01030401000000220602e49c9b9b5d0f127235dc26a0c252814c52fb333d651a946773f59d72c2da99041873c5da0a540000800200008000000080000000000000000000002202029857513f0fe1dc125f219ffef22098e14c653c4bcb0a2aaeff47b3a252569f1a1873c5da0a540000800200008000000080010000000000000000",
            3 => "70736274ff0100770100000001faf994b04953ae7fbd735800e48605de15ded8126b9308556ad98962327ab3430000000000ffffffff02005a6202000000001976a9143bf96544e911ca728f5fabf5a4f3b65deab684c488acc0448403000000001976a91419115174dcc75cce4802aaec82e63b491274c2e288ac000000000001005701000000016f9512863e3f25696cc723f570d3c4077785f1944a895895a70750689453e48b00000000020101ffffffff0100e1f505000000001976a9144a483568665dcdfa68dd58a1f62893448a64333988ac0000000001030401000000220602cc6b0dc33aabcf3a23643e5e2919a80c50fb3dd2129ce409bbc5f0d4643d05e01873c5da0a2c0000800300008000000080000000000000000000002202020745c065a8b7cb03a843a57339cf9164d675a2e5010c6fbd61c09239bbb5f4df1873c5da0a2c0000800300008000000080010000000000000000",
            5 => "70736274ff010077010000000102754c72a9069560e14bf2faa47998f4442149e798cff5f8c953e7d92ab219d20000000000ffffffff02005a6202000000001976a9140a38ec3f3cc4c169c295ddb037c31c63bf2fa39188acf05f9303000000001976a914265a7d4ef63d643548a599c8fa53a0d9ef3fdca488ac0000000000010057010000000105d94a72185cf340264af0461c176c985bf0d55fef0026c94447f723ec03f90600000000020101ffffffff0100e1f505000000001976a9148a4f58c222cd5544c527bc66925652baa70b5e8088ac00000000010304010000002206026fa9a6f213b6ba86447965f6b4821264aaadd7521f049f00db9c43a770ea74051873c5da0a2c000080050000800000008000000000000000000000220202bd7b0ae9a8a0e67f82891d92545433e1b37074ab0bf5867b977f8c70b00f28671873c5da0a2c0000800500008000000080010000000000000000",
            145 => "70736274ff0100770100000001df92cf31d578229fa876e7bf2f5696798f4d18d28e0066010e67feb6f5fb00b90000000000ffffffff02005a6202000000001976a9144f04b6e897c1a3dfe2b74bfc40cbc4d9c64daf3088ac18839303000000001976a914cfdcdfa3e0483ddd3c7b6ab6ee13c5087c26d5f188ac0000000000010057010000000153ef901d37f8bfee6f9453ff7b8dc3e0672ee715498dae97db9dca73261f9a3300000000020101ffffffff0100e1f505000000001976a914086a977c7dad32a56996af2ce90a727238d4899888ac0000000001030441000000220602bbe7dbcdf8b2261530a867df7180b17a90b482f74f2736b8a30d3f756e42e2171873c5da0a2c000080910000800000008000000000000000000000220202f178b9410d11e47e10bef8a8e4bc53e48d9ae15864a024a54f87a802dad5f5141873c5da0a2c0000809100008000000080010000000000000000",
            _ => unreachable!(),
        };
        Psbt::deserialize(&hex::decode(data).unwrap()).unwrap()
    }
    fn rejected_at_all_stages(psbt: &Psbt, coin: i128) {
        assert!(crate::parse_psbt_extend(psbt.serialize(), context(coin), Some(coin)).is_err());
        assert!(crate::check_psbt_extend(psbt.serialize(), context(coin), Some(coin)).is_err());
        assert!(crate::sign_psbt_extend(psbt.serialize(), &seed(), fp(), Some(coin)).is_err());
    }

    #[test]
    fn global_xpub_cannot_override_any_extend_network() {
        for (coin, name) in [
            (2, "Litecoin"),
            (3, "Dogecoin"),
            (5, "Dash"),
            (145, "Bitcoin Cash"),
        ] {
            let mut p = fixture(coin);
            let (path, xpub) = account(coin);
            p.xpub.insert(xpub, (fp(), path));
            let parsed =
                crate::parse_psbt_extend(p.serialize(), context(coin), Some(coin)).unwrap();
            assert_eq!(parsed.overview.network, name);
            let original =
                crate::parse_psbt_extend(fixture(coin).serialize(), context(coin), Some(coin))
                    .unwrap();
            assert_eq!(parsed.overview.from, original.overview.from);
            assert_eq!(parsed.overview.to, original.overview.to);
            crate::check_psbt_extend(p.serialize(), context(coin), Some(coin)).unwrap();
            let signed = crate::sign_psbt_extend(p.serialize(), &seed(), fp(), Some(coin)).unwrap();
            let signed_display =
                crate::parse_psbt_extend(signed, context(coin), Some(coin)).unwrap();
            assert_eq!(signed_display.overview.network, name);
            assert_eq!(signed_display.overview.to, parsed.overview.to);
        }
    }

    #[test]
    fn conflicting_metadata_is_rejected_before_approval_and_signing() {
        for coin in [2, 3, 5, 145] {
            let original = fixture(coin);
            let (path, mut xpub) = account(coin);
            let mut p = original.clone();
            p.xpub.insert(
                xpub,
                (fp(), DerivationPath::from_str("m/44'/0'/0'").unwrap()),
            );
            rejected_at_all_stages(&p, coin);
            let mut p = original.clone();
            xpub.network = NetworkKind::Test;
            p.xpub.insert(xpub, (fp(), path));
            rejected_at_all_stages(&p, coin);
            let mut p = original.clone();
            p.inputs[0].bip32_derivation.values_mut().next().unwrap().1 =
                DerivationPath::from_str("m/44'/0'/0'/0/0").unwrap();
            rejected_at_all_stages(&p, coin);
            let mut p = original.clone();
            p.outputs[1].bip32_derivation.values_mut().next().unwrap().1 =
                DerivationPath::from_str("m/44'/0'/0'/1/0").unwrap();
            rejected_at_all_stages(&p, coin);
            let mut p = original.clone();
            p.inputs[0].bip32_derivation.values_mut().next().unwrap().1 = DerivationPath::default();
            rejected_at_all_stages(&p, coin);
            for marker in [b"fb".to_vec(), b"tfb".to_vec(), b"unknown".to_vec()] {
                let mut p = original.clone();
                p.unknown.insert(
                    bitcoin::psbt::raw::Key {
                        type_value: 0xfc,
                        key: b"chain".to_vec(),
                    },
                    marker,
                );
                rejected_at_all_stages(&p, coin);
            }
        }
    }

    #[test]
    fn bch_p2sh_recipient_is_exact_cashaddr_and_signable() {
        // CashAddr specification's 160-bit P2SH example.
        let script = ScriptBuf::from_bytes(
            hex::decode("a91476a04053bda0a88bda5177b86a15c3b29f55987387").unwrap(),
        );
        let expected = "ppm2qsznhks23z7629mms6s4cwef74vcwvn0h829pq";
        let address = Address::from_script(&script, Network::BitcoinCash)
            .unwrap()
            .to_string();
        assert_eq!(address, expected);
        assert_eq!(Address::from_str(expected).unwrap().script_pubkey(), script);
        let mut p = fixture(145);
        p.unsigned_tx.output[0].script_pubkey = script;
        let display = crate::parse_psbt_extend(p.serialize(), context(145), Some(145)).unwrap();
        assert_eq!(display.detail.to[0].address, expected);
        assert!(display
            .overview
            .to
            .iter()
            .any(|output| output.address == expected));
        crate::check_psbt_extend(p.serialize(), context(145), Some(145)).unwrap();
        let signed = crate::sign_psbt_extend(p.serialize(), &seed(), fp(), Some(145)).unwrap();
        let signed_display = crate::parse_psbt_extend(signed, context(145), Some(145)).unwrap();
        assert_eq!(signed_display.detail.to[0].address, expected);
        let mut different_script = p.unsigned_tx.output[0].script_pubkey.clone().into_bytes();
        different_script[2] ^= 1;
        let different_script = ScriptBuf::from_bytes(different_script);
        let different_address = Address::from_script(&different_script, Network::BitcoinCash)
            .unwrap()
            .to_string();
        assert_ne!(different_address, expected);
        assert_eq!(
            Address::from_str(&different_address)
                .unwrap()
                .script_pubkey(),
            different_script
        );
    }

    #[test]
    fn bch_unrenderable_outputs_rejected_at_all_stages() {
        for script in [
            ScriptBuf::new(),
            ScriptBuf::from_bytes(vec![0x6a]),
            ScriptBuf::from_bytes(
                hex::decode("001476a04053bda0a88bda5177b86a15c3b29f559873").unwrap(),
            ),
        ] {
            let mut p = fixture(145);
            p.unsigned_tx.output[0].script_pubkey = script;
            rejected_at_all_stages(&p, 145);
        }
    }
}
