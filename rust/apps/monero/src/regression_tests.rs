//! Monero transaction compatibility and validation tests.

use crate::transfer::{
    is_own_change_address, AccountPublicAddress, CtKey, Multisig_kLRki, OutputEntry,
    TxConstructionData, TxDestinationEntry, TxSourceEntry,
};
use alloc::string::{String, ToString};
use alloc::vec;
use alloc::vec::Vec;
use curve25519_dalek::edwards::EdwardsPoint;
use curve25519_dalek::scalar::Scalar;
use rand_core::{OsRng, RngCore};

struct Party {
    spend: Scalar,
    view: Scalar,
}

impl Party {
    fn new() -> Party {
        Party {
            spend: random_scalar(),
            view: random_scalar(),
        }
    }

    fn spend_point(&self) -> EdwardsPoint {
        EdwardsPoint::mul_base(&self.spend)
    }

    fn view_point(&self) -> EdwardsPoint {
        EdwardsPoint::mul_base(&self.view)
    }

    fn standard_dest(&self, amount: u64) -> TxDestinationEntry {
        dest(self.spend_point(), self.view_point(), false, amount)
    }

    fn subaddress_dest(&self, m: &Scalar, amount: u64) -> TxDestinationEntry {
        let d_prime = self.spend_point() + EdwardsPoint::mul_base(m);
        let b_prime = d_prime * self.view;
        dest(d_prime, b_prime, true, amount)
    }
}

fn random_scalar() -> Scalar {
    let mut bytes = [0u8; 64];
    OsRng.fill_bytes(&mut bytes);
    Scalar::from_bytes_mod_order_wide(&bytes)
}

fn dest(
    spend: EdwardsPoint,
    view: EdwardsPoint,
    is_subaddress: bool,
    amount: u64,
) -> TxDestinationEntry {
    TxDestinationEntry {
        original: String::new(),
        amount,
        addr: AccountPublicAddress {
            spend_public_key: spend.compress().to_bytes(),
            view_public_key: view.compress().to_bytes(),
        },
        is_subaddress,
        is_integrated: false,
    }
}

fn source() -> TxSourceEntry {
    TxSourceEntry {
        outputs: vec![OutputEntry {
            index: 0,
            key: CtKey {
                dest: [0u8; 32],
                mask: [0u8; 32],
            },
        }],
        real_output: 0,
        real_out_tx_key: [0u8; 32],
        real_out_additional_tx_keys: vec![],
        real_output_in_tx_index: 0,
        amount: 0,
        rct: true,
        mask: [0u8; 32],
        multisig_kLRki: Multisig_kLRki {
            k: [0u8; 32],
            L: [0u8; 32],
            R: [0u8; 32],
            ki: [0u8; 32],
        },
    }
}

fn tx_data(payments: Vec<TxDestinationEntry>, change: TxDestinationEntry) -> TxConstructionData {
    let mut splitted_dsts = payments;
    splitted_dsts.push(change.clone());
    TxConstructionData {
        sources: vec![source()],
        change_dts: change,
        splitted_dsts,
        selected_transfers: vec![0],
        extra: vec![],
        unlock_time: 0,
        use_rct: 1,
        rct_config: crate::transfer::RCTConfig {
            version: 2,
            range_proof_type: crate::transfer::RangeProofType::CLSAG,
            bp_version: crate::transfer::RctType::RCTTypeNull,
        },
        dests: vec![],
        subaddr_account: 0,
        subaddr_indices: vec![0],
    }
}

struct PublishedKeys {
    tx_key: Scalar,
    tx_key_pub: EdwardsPoint,
    additional_keys: Vec<Scalar>,
    additional_keys_pub: Vec<EdwardsPoint>,
}

fn published_keys(data: &TxConstructionData) -> PublishedKeys {
    let (tx_key, additional_keys, tx_key_pub, additional_keys_pub) = data.transaction_keys();
    PublishedKeys {
        tx_key: tx_key.scalar,
        tx_key_pub,
        additional_keys: additional_keys.into_iter().map(|key| key.scalar).collect(),
        additional_keys_pub,
    }
}

fn assert_every_recipient_can_derive(
    data: &TxConstructionData,
    parties: &[&Party],
) -> PublishedKeys {
    let published = published_keys(data);

    for (i, dst) in data.splitted_dsts.iter().enumerate() {
        if dst.addr == data.change_dts.addr {
            continue; // change is derived as `view * R` by the sender itself
        }

        let key_to_use = match published.additional_keys.get(i) {
            Some(key) if dst.is_subaddress => key,
            _ => &published.tx_key,
        };
        let sender_ecdh = *key_to_use * dst.addr_view_point();

        let published_key = match published.additional_keys_pub.get(i) {
            Some(key) if dst.is_subaddress => *key,
            _ => published.tx_key_pub,
        };
        let recipient_ecdh = published_key * parties[i].view;

        assert_eq!(
            sender_ecdh, recipient_ecdh,
            "output {i} cannot be derived by its recipient"
        );
    }

    published
}

#[test]
fn standard_payment_only_uses_basepoint_tx_key() {
    let payee = Party::new();
    let sender = Party::new();
    let data = tx_data(vec![payee.standard_dest(2)], sender.standard_dest(1));

    let published = published_keys(&data);
    assert!(published.additional_keys.is_empty());
    assert_eq!(
        published.tx_key_pub,
        EdwardsPoint::mul_base(&published.tx_key)
    );
    assert_every_recipient_can_derive(&data, &[&payee]);
}

#[test]
fn single_subaddress_payment_uses_subaddress_spend_key() {
    let payee = Party::new();
    let sender = Party::new();
    let m = random_scalar();
    let data = tx_data(vec![payee.subaddress_dest(&m, 2)], sender.standard_dest(1));

    let published = published_keys(&data);
    assert!(published.additional_keys.is_empty());
    assert_eq!(
        published.tx_key_pub,
        payee.subaddress_dest(&m, 0).addr_spend_point() * published.tx_key
    );
    assert_every_recipient_can_derive(&data, &[&payee]);
}

#[test]
fn mixed_standard_and_subaddress_payments_use_additional_tx_keys() {
    let standard_payee = Party::new();
    let subaddress_payee = Party::new();
    let sender = Party::new();
    let m = random_scalar();

    let data = tx_data(
        vec![
            standard_payee.standard_dest(2),
            subaddress_payee.subaddress_dest(&m, 3),
        ],
        sender.standard_dest(1),
    );

    let published = assert_every_recipient_can_derive(&data, &[&standard_payee, &subaddress_payee]);
    assert_eq!(published.additional_keys.len(), data.splitted_dsts.len());
    assert_eq!(
        published.tx_key_pub,
        EdwardsPoint::mul_base(&published.tx_key)
    );
}

#[test]
fn two_subaddress_payments_use_additional_tx_keys() {
    let payee_a = Party::new();
    let payee_b = Party::new();
    let sender = Party::new();
    let (m_a, m_b) = (random_scalar(), random_scalar());

    let data = tx_data(
        vec![
            payee_a.subaddress_dest(&m_a, 2),
            payee_b.subaddress_dest(&m_b, 3),
        ],
        sender.standard_dest(1),
    );

    let published = assert_every_recipient_can_derive(&data, &[&payee_a, &payee_b]);
    assert_eq!(published.additional_keys.len(), data.splitted_dsts.len());
}

#[test]
fn every_destination_shape_stays_derivable() {
    let sender = Party::new();
    let a = Party::new();
    let b = Party::new();
    let (m, n) = (random_scalar(), random_scalar());

    let sweep = Party::new();
    let mut sweep_data = tx_data(vec![sweep.standard_dest(5)], sender.standard_dest(0));
    sweep_data.splitted_dsts.pop();
    assert_every_recipient_can_derive(&sweep_data, &[&sweep]);

    let two_std = tx_data(
        vec![a.standard_dest(2), b.standard_dest(3)],
        sender.standard_dest(1),
    );
    assert_every_recipient_can_derive(&two_std, &[&a, &b]);

    let three = tx_data(
        vec![
            a.standard_dest(2),
            b.standard_dest(3),
            a.subaddress_dest(&m, 4),
        ],
        sender.standard_dest(1),
    );
    assert_every_recipient_can_derive(&three, &[&a, &b, &a]);

    let same_party = tx_data(
        vec![a.subaddress_dest(&m, 2), a.subaddress_dest(&n, 3)],
        sender.standard_dest(1),
    );
    assert_every_recipient_can_derive(&same_party, &[&a, &a]);
}

#[test]
fn change_address_must_be_derived_from_the_device_keys() {
    use crate::key::{KeyPair, PrivateKey};

    let wallet = Party::new();
    let keypair = KeyPair::new(PrivateKey::new(wallet.view), PrivateKey::new(wallet.spend));
    let public_spend = keypair.get_public_spend();
    let m = random_scalar();

    assert!(is_own_change_address(
        &public_spend,
        &keypair.view,
        0,
        &[0],
        &wallet.standard_dest(1),
    ));

    let minor = 1u32;
    let own_subaddress = TxDestinationEntry {
        original: String::new(),
        amount: 1,
        addr: AccountPublicAddress {
            spend_public_key: keypair.get_public_sub_spend(0, minor).as_bytes(),
            view_public_key: keypair.get_public_sub_view(0, minor).as_bytes(),
        },
        is_subaddress: true,
        is_integrated: false,
    };
    assert!(is_own_change_address(
        &public_spend,
        &keypair.view,
        0,
        &[minor],
        &own_subaddress,
    ));

    let other_wallet = Party::new();
    let mut candidate = other_wallet.standard_dest(1);
    candidate.addr.view_public_key = wallet.view_point().compress().to_bytes();
    assert!(!is_own_change_address(
        &public_spend,
        &keypair.view,
        0,
        &[0],
        &candidate,
    ));

    assert!(!is_own_change_address(
        &public_spend,
        &keypair.view,
        0,
        &[0, 3],
        &other_wallet.subaddress_dest(&m, 1),
    ));
}

#[test]
fn dummy_payment_id_uses_the_payment_destination() {
    use crate::key::{KeyPair, PrivateKey};
    use monero_wallet::SharedKeyDerivations;
    use zeroize::Zeroizing;

    let sender = Party::new();
    let keypair = KeyPair::new(PrivateKey::new(sender.view), PrivateKey::new(sender.spend));
    let payee = Party::new();

    let data = tx_data(vec![payee.standard_dest(2)], sender.standard_dest(1));
    let (tx_key, additional_keys, tx_key_pub, additional_keys_pub) = data.transaction_keys();
    let extra = data
        .extra(
            &keypair,
            &tx_key,
            &additional_keys,
            &tx_key_pub,
            &additional_keys_pub,
        )
        .unwrap();

    let payment_ecdh = tx_key.scalar * payee.view_point();
    let expected = SharedKeyDerivations::payment_id_xor(Zeroizing::new(payment_ecdh));

    let nonce = extra_field(&extra, 0x02).expect("dummy payment ID nonce missing");
    assert_eq!(nonce[0], 1, "expected a short (encrypted) payment ID nonce");
    assert_eq!(&nonce[1..9], &expected);

    let change_ecdh = keypair.view.scalar * tx_key_pub;
    let change_xor = SharedKeyDerivations::payment_id_xor(Zeroizing::new(change_ecdh));
    assert_ne!(&nonce[1..9], &change_xor);
}

#[test]
fn two_output_destination_validation() {
    use crate::key::{KeyPair, PrivateKey};

    let sender = Party::new();
    let payee = Party::new();

    let mut data = tx_data(vec![payee.standard_dest(2)], sender.standard_dest(1));
    data.splitted_dsts[1].amount = 7; // no longer equal to `change_dts`

    let published = published_keys(&data);
    let keypair = KeyPair::new(PrivateKey::new(sender.view), PrivateKey::new(sender.spend));
    let extra = data
        .extra(
            &keypair,
            &PrivateKey::new(published.tx_key),
            &vec![],
            &published.tx_key_pub,
            &published.additional_keys_pub,
        )
        .unwrap();
    assert!(
        extra_field(&extra, 0x01).is_some(),
        "missing transaction key"
    );
}

#[test]
fn selected_transfers_are_written_as_varints() {
    use crate::signed_transaction::write_selected_transfers;
    use crate::utils::io::Reader;

    let transfers = vec![0usize, 1, 127, 128, 300];
    let encoded = write_selected_transfers(&transfers);

    let mut reader = Reader::new(&encoded);
    let count = reader.read_varint().unwrap() as usize;
    assert_eq!(count, transfers.len());
    for expected in transfers {
        assert_eq!(
            reader.read_varint().unwrap() as usize,
            expected,
            "index {expected} did not round-trip"
        );
    }
    assert_eq!(
        reader.offset(),
        encoded.len(),
        "trailing bytes after selected_transfers"
    );
}

#[test]
fn main_address_input_compatibility() {
    use crate::key::{KeyPair, PrivateKey, PublicKey};
    use crate::utils::hash::hash_to_scalar;
    use crate::utils::varinteger::{encode, length};

    let keypair = KeyPair::new(
        PrivateKey::new(random_scalar()),
        PrivateKey::new(random_scalar()),
    );
    let tx_key = random_scalar();
    let main_tx_pub = EdwardsPoint::mul_base(&tx_key);
    let additional_tx_pub = EdwardsPoint::mul_base(&random_scalar());

    let index = 1u64;
    let derivation = (keypair.view.scalar * main_tx_pub).mul_by_cofactor();
    let derivation_bytes = derivation.compress().to_bytes();
    let mut index_buf = vec![0u8; length(index)];
    encode(index, &mut index_buf);
    let derivation_scalar =
        hash_to_scalar(&[derivation_bytes.as_slice(), index_buf.as_slice()].concat());
    let output_pub = (keypair.spend.get_public_key().point.decompress().unwrap()
        + EdwardsPoint::mul_base(&derivation_scalar))
    .compress()
    .to_bytes();

    let additional = PublicKey::from_bytes(&additional_tx_pub.compress().to_bytes()).unwrap();
    assert!(
        crate::key_images::try_to_generate_image(
            &keypair,
            &main_tx_pub.compress().to_bytes(),
            &output_pub,
            vec![additional],
            index,
            0,
            vec![0],
        )
        .is_ok(),
        "main-address output must remain signable with additional keys present"
    );
}

#[test]
fn subaddr_account_mismatch_fails_the_input_ownership_check() {
    use crate::key::{KeyPair, PrivateKey};
    use crate::utils::hash::hash_to_scalar;
    use crate::utils::varinteger::{encode, length};

    let keypair = KeyPair::new(
        PrivateKey::new(random_scalar()),
        PrivateKey::new(random_scalar()),
    );
    let tx_key = random_scalar();
    let tx_pubkey = EdwardsPoint::mul_base(&tx_key).compress().to_bytes();

    let (real_major, minor, index) = (0u32, 3u32, 2u64);
    let derivation = (keypair.view.scalar * EdwardsPoint::mul_base(&tx_key)).mul_by_cofactor();
    let derivation_bytes = derivation.compress().to_bytes();
    let mut index_buf = vec![0u8; length(index)];
    encode(index, &mut index_buf);
    let derivation_scalar =
        hash_to_scalar(&[derivation_bytes.as_slice(), index_buf.as_slice()].concat());
    let output_pub = (keypair
        .get_public_sub_spend(real_major, minor)
        .point
        .decompress()
        .unwrap()
        + EdwardsPoint::mul_base(&derivation_scalar))
    .compress()
    .to_bytes();

    assert!(crate::key_images::try_to_generate_image(
        &keypair,
        &tx_pubkey,
        &output_pub,
        vec![],
        index,
        real_major,
        vec![minor],
    )
    .is_ok());

    assert!(crate::key_images::try_to_generate_image(
        &keypair,
        &tx_pubkey,
        &output_pub,
        vec![],
        index,
        1,
        vec![minor],
    )
    .is_err());
}

#[test]
fn destination_type_is_checked_against_the_address_string() {
    use crate::address::Address;
    use crate::structs::{AddressType, Network};

    let payee = Party::new();
    let sender = Party::new();

    let standard = Address::new(
        Network::Mainnet,
        AddressType::Standard,
        crate::key::PublicKey::new(payee.spend_point().compress()),
        crate::key::PublicKey::new(payee.view_point().compress()),
    )
    .to_string();

    let mut honest = payee.standard_dest(2);
    honest.original = standard.clone();
    let data = tx_data(vec![honest], sender.standard_dest(1));
    assert!(data.validate_ok());

    let mut lying = payee.standard_dest(2);
    lying.original = standard;
    lying.is_subaddress = true;
    let data = tx_data(vec![lying], sender.standard_dest(1));
    assert!(data.validate_ok() == false);

    let mut mismatched = payee.standard_dest(2);
    mismatched.original = Address::new(
        Network::Mainnet,
        AddressType::Standard,
        crate::key::PublicKey::new(sender.spend_point().compress()),
        crate::key::PublicKey::new(sender.view_point().compress()),
    )
    .to_string();
    let data = tx_data(vec![mismatched], sender.standard_dest(1));
    assert!(data.validate_ok() == false);
}

#[test]
fn requested_payment_id_is_carried_into_the_signed_extra() {
    use crate::key::{KeyPair, PrivateKey};
    use monero_wallet::SharedKeyDerivations;
    use zeroize::Zeroizing;

    let sender = Party::new();
    let keypair = KeyPair::new(PrivateKey::new(sender.view), PrivateKey::new(sender.spend));
    let payee = Party::new();

    let payment_id = [0x11u8, 0x22, 0x33, 0x44, 0x55, 0x66, 0x77, 0x88];
    let mut nonce = vec![1u8];
    nonce.extend_from_slice(&payment_id);

    let mut data = tx_data(vec![payee.standard_dest(2)], sender.standard_dest(1));
    data.extra = vec![0x00, 0x00, 0x02, nonce.len() as u8];
    data.extra.extend_from_slice(&nonce);

    let (tx_key, additional_keys, tx_key_pub, additional_keys_pub) = data.transaction_keys();
    let extra = data.extra_of(
        &keypair,
        &tx_key,
        &additional_keys,
        &tx_key_pub,
        &additional_keys_pub,
    );

    let written = extra_field(&extra, 0x02).expect("payment ID nonce missing");
    let expected_xor =
        SharedKeyDerivations::payment_id_xor(Zeroizing::new(tx_key.scalar * payee.view_point()));
    let expected: Vec<u8> = payment_id
        .iter()
        .zip(expected_xor.iter())
        .map(|(a, b)| a ^ b)
        .collect();
    assert_eq!(written[0], 1);
    assert_eq!(&written[1..9], expected.as_slice());

    assert_eq!(*extra.last().unwrap(), 0x00);
}

#[test]
fn integrated_and_subaddress_flags_together_are_rejected() {
    use crate::address::integrated_address_string;
    use crate::key::{KeyPair, PrivateKey, PublicKey};
    use crate::transfer::validate_destinations;

    let wallet = Party::new();
    let keypair = KeyPair::new(PrivateKey::new(wallet.view), PrivateKey::new(wallet.spend));
    let payee = Party::new();
    let payment_id = [0x11u8; 8];

    let integrated = integrated_address_string(
        &PublicKey::new(payee.spend_point().compress()),
        &PublicKey::new(payee.view_point().compress()),
        &payment_id,
    )
    .unwrap();

    let mut conflicting = payee.standard_dest(2);
    conflicting.original = integrated.clone();
    conflicting.is_integrated = true;
    conflicting.is_subaddress = true;
    assert!(
        validate_destinations(core::slice::from_ref(&tx_data(
            vec![conflicting],
            wallet.standard_dest(1)
        )))
        .is_err(),
        "integrated + subaddress must be rejected"
    );

    let mut honest = payee.standard_dest(2);
    honest.original = integrated;
    honest.is_integrated = true;
    assert!(validate_destinations(core::slice::from_ref(&tx_data(
        vec![honest],
        wallet.standard_dest(1)
    )))
    .is_ok());

    let own_sub = TxDestinationEntry {
        original: String::new(),
        amount: 1,
        addr: AccountPublicAddress {
            spend_public_key: keypair.get_public_sub_spend(0, 1).as_bytes(),
            view_public_key: keypair.get_public_sub_view(0, 1).as_bytes(),
        },
        is_subaddress: true,
        is_integrated: false,
    };
    assert!(crate::transfer::is_own_change_address(
        &keypair.get_public_spend(),
        &keypair.view,
        0,
        &[1],
        &own_sub,
    ));
}

#[test]
fn signed_payment_id_must_match_the_displayed_integrated_address() {
    use crate::address::integrated_address_string;
    use crate::key::PublicKey;
    use crate::transfer::validate_payment_id_binding;

    let wallet = Party::new();
    let payee = Party::new();
    let displayed_id = [0x11u8; 8];
    let signed_id = [0x22u8; 8];

    let integrated = integrated_address_string(
        &PublicKey::new(payee.spend_point().compress()),
        &PublicKey::new(payee.view_point().compress()),
        &displayed_id,
    )
    .unwrap();

    let mut dest = payee.standard_dest(2);
    dest.original = integrated.clone();
    dest.is_integrated = true;

    let nonce_field = |payment_id: [u8; 8]| {
        let mut extra = vec![0x02u8, 9, 1];
        extra.extend_from_slice(&payment_id);
        extra
    };

    let with_extra = |payment_id: Option<[u8; 8]>| {
        let mut data = tx_data(vec![dest.clone()], wallet.standard_dest(1));
        data.extra = match payment_id {
            Some(payment_id) => nonce_field(payment_id),
            None => vec![],
        };
        data
    };

    assert!(
        validate_payment_id_binding(core::slice::from_ref(&with_extra(Some(displayed_id)))).is_ok()
    );

    assert!(
        validate_payment_id_binding(core::slice::from_ref(&with_extra(Some(signed_id)))).is_err()
    );

    assert!(validate_payment_id_binding(core::slice::from_ref(&with_extra(None))).is_err());

    let mut no_integrated = tx_data(vec![payee.standard_dest(2)], wallet.standard_dest(1));
    no_integrated.extra = nonce_field(signed_id);
    assert!(validate_payment_id_binding(core::slice::from_ref(&no_integrated)).is_err());

    let mut plain = tx_data(vec![payee.standard_dest(2)], wallet.standard_dest(1));
    plain.extra = vec![];
    assert!(validate_payment_id_binding(core::slice::from_ref(&plain)).is_ok());

    let mut dummy = tx_data(vec![payee.standard_dest(2)], wallet.standard_dest(1));
    dummy.extra = nonce_field([0u8; 8]);
    assert!(validate_payment_id_binding(core::slice::from_ref(&dummy)).is_ok());
    assert_eq!(
        dummy.requested_extra().unwrap().nonce.as_deref(),
        Some(&[1u8, 0, 0, 0, 0, 0, 0, 0, 0][..])
    );

    let mut integrated_but_dummy = tx_data(vec![dest.clone()], wallet.standard_dest(1));
    integrated_but_dummy.extra = nonce_field([0u8; 8]);
    assert!(validate_payment_id_binding(core::slice::from_ref(&integrated_but_dummy)).is_err());
}

#[test]
fn payment_id_declared_only_in_dests_is_rejected() {
    use crate::address::integrated_address_string;
    use crate::key::PublicKey;
    use crate::transfer::validate_payment_id_binding;

    let wallet = Party::new();
    let payee = Party::new();
    let payment_id = [0x11u8; 8];

    let integrated = integrated_address_string(
        &PublicKey::new(payee.spend_point().compress()),
        &PublicKey::new(payee.view_point().compress()),
        &payment_id,
    )
    .unwrap();

    let mut in_dests_only = tx_data(vec![payee.standard_dest(2)], wallet.standard_dest(1));
    in_dests_only.dests = vec![TxDestinationEntry {
        original: integrated,
        amount: 2,
        addr: payee.standard_dest(2).addr,
        is_subaddress: false,
        is_integrated: true,
    }];
    let mut nonce = vec![0x02u8, 9, 1];
    nonce.extend_from_slice(&payment_id);
    in_dests_only.extra = nonce;

    assert!(
        validate_payment_id_binding(core::slice::from_ref(&in_dests_only)).is_err(),
        "a payment ID that is not displayed must not be signed"
    );

    let mut on_output = tx_data(vec![payee.standard_dest(2)], wallet.standard_dest(1));
    on_output.splitted_dsts[0].original = integrated_address_string(
        &PublicKey::new(payee.spend_point().compress()),
        &PublicKey::new(payee.view_point().compress()),
        &payment_id,
    )
    .unwrap();
    on_output.splitted_dsts[0].is_integrated = true;
    let mut nonce = vec![0x02u8, 9, 1];
    nonce.extend_from_slice(&payment_id);
    on_output.extra = nonce;
    assert!(validate_payment_id_binding(core::slice::from_ref(&on_output)).is_ok());
}

#[test]
fn multiple_recipients_with_a_payment_id_are_rejected() {
    use crate::address::integrated_address_string;
    use crate::key::{KeyPair, PrivateKey, PublicKey};
    use crate::transfer::validate_payment_id_binding;

    let wallet = Party::new();
    let payee = Party::new();
    let other = Party::new();
    let payment_id = [0x33u8; 8];

    let integrated = integrated_address_string(
        &PublicKey::new(payee.spend_point().compress()),
        &PublicKey::new(payee.view_point().compress()),
        &payment_id,
    )
    .unwrap();

    let mut integrated_dest = payee.standard_dest(2);
    integrated_dest.original = integrated;
    integrated_dest.is_integrated = true;

    let mut data = tx_data(
        vec![integrated_dest, other.standard_dest(3)],
        wallet.standard_dest(1),
    );
    let mut nonce = vec![0x02u8, 9, 1];
    nonce.extend_from_slice(&payment_id);
    data.extra = nonce;

    assert!(
        validate_payment_id_binding(core::slice::from_ref(&data)).is_err(),
        "an encrypted payment ID needs exactly one destination"
    );

    let keypair = KeyPair::new(PrivateKey::new(wallet.view), PrivateKey::new(wallet.spend));
    let (tx_key, additional_keys, tx_key_pub, additional_keys_pub) = data.transaction_keys();
    assert!(data
        .extra(
            &keypair,
            &tx_key,
            &additional_keys,
            &tx_key_pub,
            &additional_keys_pub,
        )
        .is_err());
}

#[test]
fn all_zero_payment_id_in_an_integrated_address_is_accepted() {
    use crate::address::integrated_address_string;
    use crate::key::{KeyPair, PrivateKey, PublicKey};
    use crate::transfer::validate_payment_id_binding;

    let wallet = Party::new();
    let payee = Party::new();

    let zero_integrated = integrated_address_string(
        &PublicKey::new(payee.spend_point().compress()),
        &PublicKey::new(payee.view_point().compress()),
        &[0u8; 8],
    )
    .unwrap();

    let mut dest = payee.standard_dest(2);
    dest.original = zero_integrated;
    dest.is_integrated = true;

    let with_nonce = |data: &mut TxConstructionData| {
        let mut nonce = vec![0x02u8, 9, 1];
        nonce.extend_from_slice(&[0u8; 8]);
        data.extra = nonce;
    };

    let mut matching = tx_data(vec![dest.clone()], wallet.standard_dest(1));
    with_nonce(&mut matching);
    assert!(validate_payment_id_binding(core::slice::from_ref(&matching)).is_ok());

    let mut no_nonce = tx_data(vec![dest.clone()], wallet.standard_dest(1));
    no_nonce.extra = vec![];
    assert!(validate_payment_id_binding(core::slice::from_ref(&no_nonce)).is_ok());

    let mut plain = tx_data(vec![payee.standard_dest(2)], wallet.standard_dest(1));
    with_nonce(&mut plain);
    assert!(validate_payment_id_binding(core::slice::from_ref(&plain)).is_ok());

    let keypair = KeyPair::new(PrivateKey::new(wallet.view), PrivateKey::new(wallet.spend));
    let mut data = tx_data(vec![dest], wallet.standard_dest(1));
    with_nonce(&mut data);
    let (tx_key, additional_keys, tx_key_pub, additional_keys_pub) = data.transaction_keys();
    assert!(data
        .extra(
            &keypair,
            &tx_key,
            &additional_keys,
            &tx_key_pub,
            &additional_keys_pub,
        )
        .is_ok());
}

#[test]
fn padding_is_counted_from_fields_not_from_zero_bytes() {
    use crate::key::{KeyPair, PrivateKey};

    let sender = Party::new();
    let keypair = KeyPair::new(PrivateKey::new(sender.view), PrivateKey::new(sender.spend));
    let payee = Party::new();

    let build = |extra: Vec<u8>| {
        let mut data = tx_data(vec![payee.standard_dest(2)], sender.standard_dest(1));
        data.extra = extra;
        let (tx_key, additional_keys, tx_key_pub, additional_keys_pub) = data.transaction_keys();
        let extra = data
            .extra(
                &keypair,
                &tx_key,
                &additional_keys,
                &tx_key_pub,
                &additional_keys_pub,
            )
            .unwrap();
        (data, extra)
    };

    let mut zero_id = vec![0x02u8, 9, 1];
    zero_id.extend_from_slice(&[0u8; 8]);
    let (data, extra) = build(zero_id);
    assert_eq!(extra.len(), 44, "unexpected extra length");
    assert_eq!(data.requested_extra().unwrap().padding, 0);

    let mut padded = vec![0x02u8, 9, 1];
    padded.extend_from_slice(&[0u8; 8]);
    padded.extend_from_slice(&[0x00, 0x00, 0x00]);
    let (data, extra) = build(padded);
    assert_eq!(data.requested_extra().unwrap().padding, 3);
    assert_eq!(extra.len(), 44 + 3, "padding must survive signing");
    assert_eq!(&extra[extra.len() - 3..], &[0u8, 0, 0]);
}

#[test]
fn each_transaction_keeps_its_own_payment_id() {
    use crate::address::integrated_address_string;
    use crate::key::{KeyPair, PrivateKey, PublicKey};
    use crate::transfer::{validate_payment_id_binding, UnsignedTx};
    use crate::utils::io::{write_tx_construction_data, write_varinteger};

    let wallet = Party::new();
    let keypair = KeyPair::new(PrivateKey::new(wallet.view), PrivateKey::new(wallet.spend));
    let payee_a = Party::new();
    let payee_b = Party::new();
    let payment_id_a = [0x11u8; 8];
    let payment_id_b = [0x22u8; 8];

    let payment_id_nonce = |payment_id: [u8; 8]| {
        let mut extra = vec![0x02u8, 9, 1];
        extra.extend_from_slice(&payment_id);
        extra
    };

    let tx_with = |payee: &Party, payment_id: [u8; 8], change: TxDestinationEntry| {
        let mut dest = payee.standard_dest(2);
        dest.original = integrated_address_string(
            &PublicKey::new(payee.spend_point().compress()),
            &PublicKey::new(payee.view_point().compress()),
            &payment_id,
        )
        .unwrap();
        dest.is_integrated = true;

        let mut data = tx_data(vec![dest], change);
        data.extra = payment_id_nonce(payment_id);
        data.sources[0].amount = 100;
        data
    };

    let first = tx_with(&payee_a, payment_id_a, wallet.standard_dest(1));
    let second = tx_with(&payee_b, payment_id_b, wallet.standard_dest(1));

    assert!(validate_payment_id_binding(core::slice::from_ref(&first)).is_ok());
    assert!(validate_payment_id_binding(core::slice::from_ref(&second)).is_ok());
    assert!(validate_payment_id_binding(&[first.clone(), second.clone()]).is_ok());

    let mut payload = write_varinteger(2);
    payload.extend_from_slice(&write_varinteger(2));
    payload.extend_from_slice(&write_tx_construction_data(&first));
    payload.extend_from_slice(&write_tx_construction_data(&second));
    payload.extend_from_slice(&write_varinteger(0));

    let parsed = UnsignedTx::deserialize(&payload).unwrap();
    let info = parsed
        .display_info(&keypair.get_public_spend(), &keypair.view)
        .unwrap();

    assert_eq!(info.outputs.len(), 4);
    assert_eq!(info.outputs[0].payment_id, hex::encode(payment_id_a));
    assert_eq!(info.outputs[2].payment_id, hex::encode(payment_id_b));
    assert_ne!(
        info.outputs[0].payment_id, info.outputs[2].payment_id,
        "the second transaction must not overwrite the first one's ID"
    );
    assert!(info.outputs[1].payment_id.is_empty());
    assert!(info.outputs[3].payment_id.is_empty());
}

#[test]
fn payload_digest_tracks_the_payload() {
    use crate::transfer::payload_digest;

    assert_eq!(
        payload_digest(b"unsigned-tx-set"),
        payload_digest(b"unsigned-tx-set")
    );
    assert_ne!(
        payload_digest(b"unsigned-tx-set"),
        payload_digest(b"unsigned-tx-seu")
    );
    assert_ne!(payload_digest(b""), payload_digest(b"\x00"));
}

#[test]
fn integrated_destinations_are_displayed_with_their_payment_id() {
    use crate::address::integrated_address_string;
    use crate::key::{KeyPair, PrivateKey, PublicKey};
    use crate::transfer::UnsignedTx;
    use crate::utils::io::{write_tx_construction_data, write_varinteger};

    let wallet = Party::new();
    let keypair = KeyPair::new(PrivateKey::new(wallet.view), PrivateKey::new(wallet.spend));
    let payee = Party::new();
    let payment_id = [0xAA, 0xBB, 0xCC, 0xDD, 0xEE, 0xFF, 0x11, 0x22];

    let integrated = integrated_address_string(
        &PublicKey::new(payee.spend_point().compress()),
        &PublicKey::new(payee.view_point().compress()),
        &payment_id,
    )
    .unwrap();
    assert!(integrated.starts_with('4') && integrated.len() > 100);

    let mut dest = payee.standard_dest(2);
    dest.original = integrated.clone();
    dest.is_integrated = true;

    let plain_address = crate::address::Address::new(
        crate::structs::Network::Mainnet,
        crate::structs::AddressType::Standard,
        PublicKey::new(payee.spend_point().compress()),
        PublicKey::new(payee.view_point().compress()),
    )
    .to_string();
    let mut plain_dest = payee.standard_dest(3);
    plain_dest.original = plain_address.clone();

    let mut data = tx_data(vec![dest, plain_dest], wallet.standard_dest(1));
    data.sources[0].amount = 100;

    assert!(crate::transfer::validate_destinations(core::slice::from_ref(&data)).is_ok());

    let mut payload = write_varinteger(2);
    payload.extend_from_slice(&write_varinteger(1));
    payload.extend_from_slice(&write_tx_construction_data(&data));
    payload.extend_from_slice(&write_varinteger(0));

    let parsed = UnsignedTx::deserialize(&payload).unwrap();
    let info = parsed
        .display_info(&keypair.get_public_spend(), &keypair.view)
        .unwrap();

    assert_eq!(info.outputs.len(), 3);
    assert_eq!(
        info.outputs[0].address, integrated,
        "integrated address must be shown as requested"
    );
    assert!(
        info.outputs[0].is_integrated,
        "the integrated output must be flagged so the UI can show the payment ID"
    );
    assert_eq!(
        info.outputs[1].address, plain_address,
        "plain destination stays a plain address"
    );
    assert!(!info.outputs[1].is_integrated);
    assert!(
        info.outputs[2].is_change,
        "change is badged (it is our own address)"
    );
    assert_eq!(info.outputs[0].payment_id, hex::encode(payment_id));
    assert_eq!(info.outputs[0].payment_id.len(), 16);
    assert!(info.outputs[1].payment_id.is_empty());
    assert!(info.outputs[2].payment_id.is_empty());
}

#[test]
fn rings_are_normalised_to_ascending_output_indices() {
    use crate::transfer::UnsignedTx;
    use crate::utils::io::{write_tx_construction_data, write_varinteger};

    let sender = Party::new();
    let payee = Party::new();

    let mut data = tx_data(vec![payee.standard_dest(2)], sender.standard_dest(1));
    data.sources[0].outputs = vec![
        OutputEntry {
            index: 900,
            key: CtKey {
                dest: [0x33; 32],
                mask: [0x44; 32],
            },
        },
        OutputEntry {
            index: 500,
            key: CtKey {
                dest: [0x11; 32],
                mask: [0x22; 32],
            },
        },
    ];
    data.sources[0].real_output = 1; // the index 500 member

    let mut payload = write_varinteger(2); // version
    payload.extend_from_slice(&write_varinteger(1)); // one transaction
    payload.extend_from_slice(&write_tx_construction_data(&data));
    payload.extend_from_slice(&write_varinteger(0)); // no exported transfers

    let parsed = UnsignedTx::deserialize(&payload).unwrap();
    let source = &parsed.txes[0].sources[0];
    let indices: Vec<u64> = source.outputs.iter().map(|output| output.index).collect();
    assert_eq!(indices, vec![500, 900], "ring must ascend by output index");
    assert_eq!(
        source.outputs[source.real_output as usize].index, 500,
        "the selected output must stay selected after normalisation"
    );
}

#[test]
fn malformed_payloads_return_an_error() {
    use crate::transfer::UnsignedTx;

    assert!(UnsignedTx::deserialize(&[0x02, 0x01, 0x01, 0x80]).is_err());
    assert!(UnsignedTx::deserialize(&[]).is_err());
    assert!(UnsignedTx::deserialize(&[0x02, 0xff, 0xff, 0xff, 0xff, 0x0f]).is_err());
    let mut bytes = vec![0x02, 0x01, 0x01];
    bytes.push(0x01); // one ring member
    bytes.extend_from_slice(&[0x02, 0x00]); // tag + index
    bytes.extend_from_slice(&[0u8; 64]); // dest + mask
    bytes.extend_from_slice(&1u64.to_le_bytes()); // real_output = 1 >= ring size
    assert!(UnsignedTx::deserialize(&bytes).is_err());
}

#[test]
fn change_destination_consistency() {
    use crate::key::{KeyPair, PrivateKey};
    use crate::structs::Network;
    use monero_wallet::SharedKeyDerivations;
    use zeroize::Zeroizing;

    let sender = Party::new();
    let payee = Party::new();
    let keypair = KeyPair::new(PrivateKey::new(sender.view), PrivateKey::new(sender.spend));

    let minor = 1u32;
    let m = random_scalar();
    let change = sender.subaddress_dest(&m, 1);
    let change_addr = change.addr.to_address(Network::Mainnet, true).to_string();

    let mut data = tx_data(vec![payee.standard_dest(2)], change);
    data.subaddr_indices = vec![0, minor];
    let change_index = data.splitted_dsts.len() - 1;

    data.rct_config.bp_version = crate::transfer::RctType::RCTTypeBulletproof2;

    data.splitted_dsts[change_index].original = change_addr;
    assert_ne!(data.splitted_dsts[change_index], data.change_dts);
    assert!(
        data.validate_ok(),
        "destination representation is consistent"
    );

    assert!(data.is_change_dest(&data.splitted_dsts[change_index]));

    let published = published_keys(&data);
    let additional_keys: Vec<PrivateKey> = published
        .additional_keys
        .iter()
        .map(|key| PrivateKey::new(*key))
        .collect();
    let inner = data
        .outputs(
            &keypair,
            &PrivateKey::new(published.tx_key),
            &additional_keys,
            &published.tx_key_pub,
        )
        .unwrap();
    let derived = inner.get_outputs()[change_index].key;

    let subaddress_spend = sender.spend_point() + EdwardsPoint::mul_base(&m);

    let change_derivation = SharedKeyDerivations::output_derivations(
        None,
        Zeroizing::new(keypair.view.scalar * published.tx_key_pub),
        change_index,
    );
    let expected_change = subaddress_spend + EdwardsPoint::mul_base(&change_derivation.shared_key);

    let subaddress_view = subaddress_spend * sender.view;
    let recipient_derivation = SharedKeyDerivations::output_derivations(
        None,
        Zeroizing::new(published.tx_key * subaddress_view),
        change_index,
    );
    let alternate_recipient =
        subaddress_spend + EdwardsPoint::mul_base(&recipient_derivation.shared_key);

    assert_eq!(derived, expected_change.compress());
    assert_ne!(derived, alternate_recipient.compress());
}

#[test]
fn key_image_export_verifies_the_transaction_key_it_used() {
    use crate::key::{KeyPair, PrivateKey};
    use crate::outputs::ExportedTransferDetail;
    use monero_serai::generators::hash_to_point;
    use monero_wallet::SharedKeyDerivations;
    use rand_core::SeedableRng;
    use zeroize::Zeroizing;

    let wallet = Party::new();
    let keypair = KeyPair::new(PrivateKey::new(wallet.view), PrivateKey::new(wallet.spend));

    let r = random_scalar();
    let index = 3u64;
    let (major, minor) = (1u32, 1u32);
    let derivation = SharedKeyDerivations::output_derivations(
        None,
        Zeroizing::new(keypair.view.scalar * EdwardsPoint::mul_base(&r)),
        index as usize,
    );
    let offset = derivation.shared_key + Scalar::from_bytes_mod_order(keypair.get_m(major, minor));
    let prv_key = keypair.spend.scalar + offset;
    let pubkey = EdwardsPoint::mul_base(&prv_key).compress().to_bytes();

    let unrelated = random_scalar();
    let detail = ExportedTransferDetail {
        pubkey,
        internal_output_index: index,
        global_output_index: 0,
        tx_pubkey: EdwardsPoint::mul_base(&r).compress().to_bytes(),
        flags: 0,
        amount: 1,
        additional_tx_keys: vec![PrivateKey::new(unrelated).get_public_key()],
        major,
        minor,
    };

    let expected = (prv_key * hash_to_point(pubkey)).compress().to_bytes();

    let mut rng = rand_chacha::ChaCha20Rng::from_seed([7u8; 32]);
    let exported = detail.key_image(&keypair, &mut rng).unwrap();
    assert_eq!(
        exported.image.0, expected,
        "the export must return the key image of the requested output"
    );

    let (_, image) = detail
        .generate_key_image_without_signature(&keypair)
        .unwrap();
    assert_eq!(image.0, expected);

    let foreign = Party::new();
    let mut unknown = detail.clone();
    unknown.pubkey = foreign.standard_dest(1).addr.spend_public_key;
    assert!(unknown.key_image(&keypair, &mut rng).is_err());
    assert!(unknown
        .generate_key_image_without_signature(&keypair)
        .is_err());
}

#[test]
fn curve_point_validation() {
    use crate::key::{KeyPair, PrivateKey};
    use crate::key_images::try_to_generate_image;
    use crate::outputs::ExportedTransferDetail;
    use crate::transfer::validate_curve_points;
    use rand_core::SeedableRng;

    let bad = off_curve_point();

    let sender = Party::new();
    let payee = Party::new();
    let data = tx_data(vec![payee.standard_dest(2)], sender.standard_dest(1));
    assert!(validate_curve_points(core::slice::from_ref(&data)).is_ok());

    let mut tx_key = data.clone();
    tx_key.sources[0].real_out_tx_key = bad;
    assert!(validate_curve_points(core::slice::from_ref(&tx_key)).is_err());

    let mut additional = data.clone();
    additional.sources[0].real_out_additional_tx_keys =
        vec![payee.standard_dest(1).addr.view_public_key, bad];
    assert!(validate_curve_points(core::slice::from_ref(&additional)).is_err());

    let mut ring_key = data.clone();
    ring_key.sources[0].outputs[0].key.dest = bad;
    assert!(validate_curve_points(core::slice::from_ref(&ring_key)).is_err());

    let mut ring_mask = data.clone();
    ring_mask.sources[0].outputs[0].key.mask = bad;
    assert!(validate_curve_points(core::slice::from_ref(&ring_mask)).is_err());

    let mut dest_spend = data.clone();
    dest_spend.splitted_dsts[0].addr.spend_public_key = bad;
    assert!(validate_curve_points(core::slice::from_ref(&dest_spend)).is_err());

    let mut dest_view = data.clone();
    dest_view.splitted_dsts[0].addr.view_public_key = bad;
    assert!(validate_curve_points(core::slice::from_ref(&dest_view)).is_err());

    let mut change_view = data.clone();
    change_view.change_dts.addr.view_public_key = bad;
    assert!(validate_curve_points(core::slice::from_ref(&change_view)).is_err());

    let wallet = Party::new();
    let keypair = KeyPair::new(PrivateKey::new(wallet.view), PrivateKey::new(wallet.spend));
    let output = wallet.standard_dest(1).addr.spend_public_key;
    assert!(try_to_generate_image(&keypair, &bad, &output, vec![], 0, 0, vec![0]).is_err());
    assert!(try_to_generate_image(&keypair, &output, &bad, vec![], 0, 0, vec![0]).is_err());

    let detail = ExportedTransferDetail {
        pubkey: bad,
        internal_output_index: 0,
        global_output_index: 0,
        tx_pubkey: bad,
        flags: 0,
        amount: 1,
        additional_tx_keys: vec![],
        major: 0,
        minor: 0,
    };
    let mut rng = rand_chacha::ChaCha20Rng::from_seed([3u8; 32]);
    assert!(detail.key_image(&keypair, &mut rng).is_err());
    assert!(detail
        .generate_key_image_without_signature(&keypair)
        .is_err());
}

fn off_curve_point() -> [u8; 32] {
    (0u8..=u8::MAX)
        .map(|byte| [byte; 32])
        .find(|bytes| {
            curve25519_dalek::edwards::CompressedEdwardsY::from_slice(bytes)
                .expect("32 bytes")
                .decompress()
                .is_none()
        })
        .expect("a repeated-byte pattern off the curve must exist")
}

fn extra_field(extra: &[u8], tag: u8) -> Option<Vec<u8>> {
    fn skip_varint(extra: &[u8], offset: usize) -> Option<usize> {
        let mut at = offset;
        for _ in 0..10 {
            let byte = *extra.get(at)?;
            at += 1;
            if byte & 0x80 == 0 {
                return Some(at - offset);
            }
        }
        None
    }

    let mut offset = 0usize;
    while offset < extra.len() {
        let field_tag = extra[offset];
        offset += 1;

        let data = match field_tag {
            0x00 => vec![],
            0x01 => {
                let data = extra.get(offset..offset + 32)?.to_vec();
                offset += 32;
                data
            }
            0x02 => {
                let len = *extra.get(offset)? as usize;
                offset += 1;
                let data = extra.get(offset..offset + len)?.to_vec();
                offset += len;
                data
            }
            0x03 => {
                offset += skip_varint(extra, offset)?;
                let data = extra.get(offset..offset + 32)?.to_vec();
                offset += 32;
                data
            }
            0x04 => {
                let count = *extra.get(offset)? as usize;
                offset += 1;
                let data = extra.get(offset..offset + 32 * count)?.to_vec();
                offset += 32 * count;
                data
            }
            _ => return None,
        };

        if field_tag == tag {
            return Some(data);
        }
    }
    None
}

impl TxDestinationEntry {
    fn addr_spend_point(&self) -> EdwardsPoint {
        curve25519_dalek::edwards::CompressedEdwardsY::from_slice(&self.addr.spend_public_key)
            .unwrap()
            .decompress()
            .unwrap()
    }

    fn addr_view_point(&self) -> EdwardsPoint {
        curve25519_dalek::edwards::CompressedEdwardsY::from_slice(&self.addr.view_public_key)
            .unwrap()
            .decompress()
            .unwrap()
    }
}

impl TxConstructionData {
    fn validate_ok(&self) -> bool {
        crate::transfer::validate_destinations(core::slice::from_ref(self)).is_ok()
    }

    fn extra_of(
        &self,
        keypair: &crate::key::KeyPair,
        tx_key: &crate::key::PrivateKey,
        additional_keys: &Vec<crate::key::PrivateKey>,
        tx_key_pub: &EdwardsPoint,
        additional_keys_pub: &Vec<EdwardsPoint>,
    ) -> Vec<u8> {
        self.extra(
            keypair,
            tx_key,
            additional_keys,
            tx_key_pub,
            additional_keys_pub,
        )
        .unwrap()
    }
}

#[test]
fn preflight_wallet_validation() {
    use crate::errors::MoneroError;
    use crate::key::{KeyPair, PrivateKey};
    use crate::transfer::UnsignedTx;
    let wallet = Party::new();
    let recipient = Party::new();
    let keys = KeyPair::new(PrivateKey::new(wallet.view), PrivateKey::new(wallet.spend));
    let mut tx = tx_data(vec![recipient.standard_dest(2)], wallet.standard_dest(1));
    tx.sources[0].amount = 4;
    prepare_wallet_input(&mut tx, &wallet);
    let mut set = UnsignedTx::default();
    set.txes.push(tx);
    assert!(set
        .validate_for_wallet(&keys.get_public_spend(), &keys.view)
        .is_ok());
    set.txes[0].sources[0].amount = 2;
    assert!(matches!(
        set.validate_for_wallet(&keys.get_public_spend(), &keys.view),
        Err(MoneroError::FormatError)
    ));
    set.txes[0].sources[0].amount = 4;
    set.txes[0].change_dts = recipient.standard_dest(1);
    set.txes[0].splitted_dsts[1] = recipient.standard_dest(1);
    assert!(matches!(
        set.validate_for_wallet(&keys.get_public_spend(), &keys.view),
        Err(MoneroError::ChangeAddressMismatch)
    ));
}

#[test]
fn preflight_envelope_validation() {
    use crate::transfer::check_unsigned;
    use crate::utils::constants::UNSIGNED_TX_PREFIX;
    for payload in [
        vec![],
        vec![0; 8],
        UNSIGNED_TX_PREFIX.as_bytes().to_vec(),
        [UNSIGNED_TX_PREFIX.as_bytes(), &[0; 71]].concat(),
    ] {
        assert!(check_unsigned(payload, [0; 32], [0; 32], [0; 32]).is_err());
    }
}

#[test]
fn subaddress_output_order_consistency() {
    use crate::key::{KeyPair, PrivateKey, PublicKey};
    use crate::utils::{hash::hash_to_scalar, io::write_varinteger};
    let sender = Party::new();
    let receiver = Party::new();
    let wallet = KeyPair::new(PrivateKey::new(sender.view), PrivateKey::new(sender.spend));
    let change = dest(
        wallet
            .get_public_sub_spend(0, 1)
            .point
            .decompress()
            .unwrap(),
        wallet.get_public_sub_view(0, 1).point.decompress().unwrap(),
        true,
        1,
    );
    let payment = receiver.subaddress_dest(&Scalar::from(7u64), 2);
    for change_first in [false, true] {
        let mut data = tx_data(vec![payment.clone()], change.clone());
        data.rct_config.bp_version = crate::transfer::RctType::RCTTypeNull;
        if change_first {
            data.splitted_dsts.swap(0, 1);
        }
        let payment_index = if change_first { 1 } else { 0 };
        let reference_key = PrivateKey::new(Scalar::from(123u64));
        let first_sub = data.splitted_dsts.iter().find(|d| d.is_subaddress).unwrap();
        let reference_public = reference_key.scalar
            * PublicKey::from_bytes(&first_sub.addr.spend_public_key)
                .unwrap()
                .point
                .decompress()
                .unwrap();
        let reference_outputs = data
            .outputs(&wallet, &reference_key, &vec![], &reference_public)
            .unwrap()
            .get_outputs();
        let recover = |r: EdwardsPoint, index: usize, view: Scalar, spend: EdwardsPoint| {
            let mut b = (view * r).mul_by_cofactor().compress().to_bytes().to_vec();
            b.extend(write_varinteger(index as u64));
            (spend + EdwardsPoint::mul_base(&hash_to_scalar(&b))).compress()
        };
        let receiver_spend = PublicKey::from_bytes(&payment.addr.spend_public_key)
            .unwrap()
            .point
            .decompress()
            .unwrap();
        assert_eq!(
            recover(
                reference_public,
                payment_index,
                receiver.view,
                receiver_spend
            ) == reference_outputs[payment_index].key,
            !change_first
        );
        let (r, additional, public, _) = data.transaction_keys();
        assert!(additional.is_empty());
        assert_eq!(public, r.scalar * receiver_spend);
        let outputs = data
            .outputs(&wallet, &r, &additional, &public)
            .unwrap()
            .get_outputs();
        assert_eq!(
            recover(public, payment_index, receiver.view, receiver_spend),
            outputs[payment_index].key
        );
        let ci = 1 - payment_index;
        let change_spend = PublicKey::from_bytes(&change.addr.spend_public_key)
            .unwrap()
            .point
            .decompress()
            .unwrap();
        assert_eq!(
            recover(public, ci, sender.view, change_spend),
            outputs[ci].key
        );
    }
}

#[test]
fn change_address_index_boundaries() {
    use crate::errors::MoneroError;
    use crate::key::calc_subaddress_m;
    use crate::key::{KeyPair, PrivateKey};
    use crate::transfer::UnsignedTx;

    let wallet = Party::new();
    let recipient = Party::new();
    let keys = KeyPair::new(PrivateKey::new(wallet.view), PrivateKey::new(wallet.spend));
    for (major, minor) in [(0, 0), (0, 1), (1, 0)] {
        let m =
            Scalar::from_bytes_mod_order(calc_subaddress_m(&keys.view.to_bytes(), major, minor));
        let change = wallet.subaddress_dest(&m, 1);
        for indices in [vec![], vec![minor]] {
            assert_eq!(
                is_own_change_address(
                    &keys.get_public_spend(),
                    &keys.view,
                    major,
                    &indices,
                    &change
                ),
                (major != 0 || minor != 0) && (minor == 0 || indices.contains(&minor))
            );
        }
        let mut tx = tx_data(vec![recipient.standard_dest(2)], change);
        tx.subaddr_account = major;
        tx.subaddr_indices = vec![minor];
        tx.sources[0].amount = 4;
        prepare_wallet_input(&mut tx, &wallet);
        let mut set = UnsignedTx::default();
        set.txes.push(tx);
        let result = set.validate_for_wallet(&keys.get_public_spend(), &keys.view);
        if major == 0 && minor == 0 {
            assert!(matches!(result, Err(MoneroError::ChangeAddressMismatch)));
            assert!(matches!(
                set.sign(&keys),
                Err(MoneroError::ChangeAddressMismatch)
            ));
            let primary = wallet.standard_dest(1);
            set.txes[0].change_dts = primary.clone();
            set.txes[0].splitted_dsts[1] = primary;
            assert!(set
                .validate_for_wallet(&keys.get_public_spend(), &keys.view)
                .is_ok());
        } else {
            assert!(result.is_ok());
        }
    }
}

#[test]
fn amount_total_boundaries() {
    let wallet = Party::new();
    let mut tx = tx_data(vec![wallet.standard_dest(1)], wallet.standard_dest(0));
    tx.sources[0].amount = u64::MAX;
    assert_eq!(tx.sum_source_amounts().unwrap(), u64::MAX);
    tx.sources.push(tx.sources[0].clone());
    assert!(tx.sum_source_amounts().is_err());
    assert!(tx.fee().is_err());
    tx.splitted_dsts[0].amount = u64::MAX;
    tx.splitted_dsts[1].amount = 1;
    assert!(tx.sum_destination_amounts().is_err());
}

#[test]
fn keyring_text_validation() {
    let text = alloc::format!("{}é{}", "a".repeat(63), "a".repeat(63));
    assert_eq!(text.len(), 128);
    assert!(
        crate::address::pub_keyring_to_address(crate::structs::Network::Mainnet, false, text)
            .is_err()
    );
}

#[test]
fn extra_field_length_encoding() {
    use crate::utils::io::write_varinteger;
    let wallet = Party::new();
    let mut tx = tx_data(vec![wallet.standard_dest(1)], wallet.standard_dest(0));
    tx.extra = vec![2];
    tx.extra.extend(write_varinteger(128));
    tx.extra.extend(vec![1; 128]);
    assert_eq!(tx.requested_extra().unwrap().nonce.unwrap().len(), 128);
    tx.extra = vec![3, 33, 0];
    tx.extra.extend([0; 32]);
    assert!(tx.requested_extra().is_ok());
    tx.extra.push(0);
    assert_eq!(tx.requested_extra().unwrap().padding, 1);
    tx.extra = vec![4];
    tx.extra.extend(write_varinteger(128));
    tx.extra.extend(vec![0; 128 * 32]);
    tx.extra.extend([2, 1, 7]);
    assert_eq!(tx.requested_extra().unwrap().nonce, Some(vec![7]));
    tx.extra.pop();
    assert!(tx.requested_extra().is_err());
}

#[test]
fn request_discriminator_validation() {
    use crate::outputs::ExportedTransferDetails;
    use crate::transfer::UnsignedTx;
    assert!(UnsignedTx::deserialize(&[2, 0, 0]).is_ok());
    assert!(UnsignedTx::deserialize(&[3, 0, 0]).is_err());
    assert!(UnsignedTx::deserialize(&[2, 0, 0, 0]).is_err());
    assert!(ExportedTransferDetails::from_bytes(&[0]).is_ok());
    assert!(ExportedTransferDetails::from_bytes(&[3, 16, 16, 0]).is_ok());
    assert!(ExportedTransferDetails::from_bytes(&[3, 16, 16, 0, 0]).is_err());
    assert!(ExportedTransferDetails::from_bytes(&[0, 1]).is_err());
    assert!(ExportedTransferDetails::from_bytes(&[2]).is_err());
    assert!(UnsignedTx::deserialize(&vec![0; crate::utils::io::MAX_REQUEST_BYTES + 1]).is_err());
}

#[test]
fn transaction_set_total_boundaries() {
    use crate::key::{KeyPair, PrivateKey};
    use crate::transfer::UnsignedTx;
    let wallet = Party::new();
    let keys = KeyPair::new(PrivateKey::new(wallet.view), PrivateKey::new(wallet.spend));
    let mut tx = tx_data(vec![wallet.standard_dest(1)], wallet.standard_dest(0));
    tx.sources[0].amount = u64::MAX;
    prepare_wallet_input(&mut tx, &wallet);
    let mut set = UnsignedTx::default();
    set.txes = vec![tx.clone()];
    assert!(set
        .validate_for_wallet(&keys.get_public_spend(), &keys.view)
        .is_ok());
    set.txes.push(tx);
    assert!(set
        .validate_for_wallet(&keys.get_public_spend(), &keys.view)
        .is_err());
    assert!(set
        .display_info(&keys.get_public_spend(), &keys.view)
        .is_err());
}

#[test]
fn serialized_index_and_tag_boundaries() {
    use crate::transfer::UnsignedTx;
    use crate::utils::io::{write_tx_construction_data, write_varinteger};
    let wallet = Party::new();
    let mut tx = tx_data(vec![wallet.standard_dest(1)], wallet.standard_dest(0));
    tx.subaddr_indices = vec![0];
    prepare_wallet_input(&mut tx, &wallet);
    let mut bytes = vec![2, 1];
    bytes.extend(write_tx_construction_data(&tx));
    bytes.push(0);
    assert!(UnsignedTx::deserialize(&bytes).is_ok());
    let mut bad_tag = bytes.clone();
    bad_tag[4] = 3;
    assert!(UnsignedTx::deserialize(&bad_tag).is_err());
    bytes.truncate(bytes.len() - 2);
    bytes.extend(write_varinteger(u32::MAX as u64 + 1));
    bytes.push(0);
    assert!(UnsignedTx::deserialize(&bytes).is_err());
}

#[test]
fn integrated_payment_destination_identity() {
    use crate::address::integrated_address_string;
    use crate::key::{KeyPair, PrivateKey, PublicKey};
    use crate::transfer::{validate_payment_id_binding, UnsignedTx};

    let wallet = Party::new();
    let payee = Party::new();
    let other = Party::new();
    let keys = KeyPair::new(PrivateKey::new(wallet.view), PrivateKey::new(wallet.spend));
    for payment_id in [[0; 8], [0x11; 8]] {
        let mut integrated = payee.standard_dest(2);
        integrated.is_integrated = true;
        integrated.original = integrated_address_string(
            &PublicKey::new(payee.spend_point().compress()),
            &PublicKey::new(payee.view_point().compress()),
            &payment_id,
        )
        .unwrap();
        let mut tx = tx_data(vec![integrated.clone()], wallet.standard_dest(1));
        tx.extra = vec![2, 9, 1];
        tx.extra.extend(payment_id);
        tx.sources[0].amount = 4;
        prepare_wallet_input(&mut tx, &wallet);
        assert!(validate_payment_id_binding(core::slice::from_ref(&tx)).is_ok());
        tx.splitted_dsts[0].amount = 0;
        tx.splitted_dsts.insert(1, other.standard_dest(2));
        let mut set = UnsignedTx::default();
        set.txes.push(tx.clone());
        assert!(set
            .validate_for_wallet(&keys.get_public_spend(), &keys.view)
            .is_err());
        assert!(set.sign(&keys).is_err());
        tx.splitted_dsts[1] = payee.standard_dest(2);
        assert!(validate_payment_id_binding(core::slice::from_ref(&tx)).is_err());
        tx.splitted_dsts[0] = integrated.clone();
        assert!(validate_payment_id_binding(core::slice::from_ref(&tx)).is_ok());
        tx.change_dts = integrated;
        assert!(validate_payment_id_binding(core::slice::from_ref(&tx)).is_err());
    }
}

#[test]
fn display_amount_field_semantics() {
    use crate::key::{KeyPair, PrivateKey};
    use crate::transfer::UnsignedTx;
    let wallet = Party::new();
    let recipient = Party::new();
    let keys = KeyPair::new(PrivateKey::new(wallet.view), PrivateKey::new(wallet.spend));
    let mut tx = tx_data(
        vec![recipient.standard_dest(2_000_000_000_000)],
        wallet.standard_dest(1_000_000_000_000),
    );
    tx.sources[0].amount = 3_010_000_000_000;
    let mut set = UnsignedTx::default();
    set.txes.push(tx);
    let display = set
        .display_info(&keys.get_public_spend(), &keys.view)
        .unwrap();
    assert_eq!(display.input_amount, "3.01 XMR");
    assert_eq!(display.output_amount, "3 XMR");
    assert_eq!(display.fee, "0.01 XMR");
}

#[test]
fn view_key_encoding_boundaries() {
    use crate::key::PrivateKey;
    assert!(PrivateKey::try_from_slice(&[0; 32]).is_err());
    assert!(PrivateKey::try_from_slice(&[255; 32]).is_err());
    assert!(PrivateKey::try_from_slice(&[1; 31]).is_err());
    let scalar = Scalar::from(1u64).to_bytes();
    assert_eq!(
        PrivateKey::try_from_slice(&scalar).unwrap().to_bytes(),
        scalar
    );
}

#[test]
fn envelope_length_boundaries() {
    use crate::utils::{constants::*, decrypt_data_with_decrypt_key};
    for magic in [
        UNSIGNED_TX_PREFIX,
        OUTPUT_EXPORT_MAGIC,
        KEY_IMAGE_EXPORT_MAGIC,
    ] {
        for length in 0..140 {
            let mut data = magic.as_bytes().to_vec();
            data.extend(vec![0; length]);
            assert!(
                decrypt_data_with_decrypt_key([0; 32], Scalar::ONE.to_bytes(), data, magic)
                    .is_err()
            );
        }
        assert!(decrypt_data_with_decrypt_key(
            [0; 32],
            Scalar::ONE.to_bytes(),
            vec![0; 256],
            magic
        )
        .is_err());
    }
}

#[test]
fn preflight_supported_transaction_boundaries() {
    use crate::key::PrivateKey;
    use crate::transfer::{RctType, UnsignedTx};
    let wallet = Party::new();
    let view = PrivateKey::new(wallet.view);
    let spend = PrivateKey::new(wallet.spend).get_public_key();
    let mut set = UnsignedTx::default();
    assert!(set.validate_for_wallet(&spend, &view).is_err());
    let recipient = Party::new();
    let mut tx = tx_data(vec![recipient.standard_dest(2)], wallet.standard_dest(1));
    tx.sources[0].amount = 4;
    prepare_wallet_input(&mut tx, &wallet);
    set.txes.push(tx);
    assert!(set.validate_for_wallet(&spend, &view).is_ok());
    let valid = set.clone();
    set.txes[0].rct_config.bp_version = RctType::RCTTypeCLSAG;
    assert!(set.validate_for_wallet(&spend, &view).is_err());
    set = valid.clone();
    set.txes[0].splitted_dsts = vec![recipient.standard_dest(0); 17];
    assert!(set.validate_for_wallet(&spend, &view).is_err());
    set = valid.clone();
    set.txes[0].sources.clear();
    assert!(set.validate_for_wallet(&spend, &view).is_err());
    set = valid.clone();
    set.txes[0].splitted_dsts.clear();
    assert!(set.validate_for_wallet(&spend, &view).is_err());
    set = valid;
    set.txes[0].subaddr_indices = (0..513).collect();
    assert!(set.validate_for_wallet(&spend, &view).is_err());
}

#[test]
fn destination_type_consistency() {
    use crate::key::{KeyPair, PrivateKey};
    use crate::structs::Network;
    use crate::transfer::UnsignedTx;
    let wallet = Party::new();
    let recipient = Party::new();
    let sub = recipient.subaddress_dest(&Scalar::from(7u64), 2);
    let mut second = sub.clone();
    second.original = second.addr.to_address(Network::Mainnet, true).to_string();
    let keys = KeyPair::new(PrivateKey::new(wallet.view), PrivateKey::new(wallet.spend));
    let mut tx = tx_data(vec![sub.clone(), second], wallet.standard_dest(1));
    tx.sources[0].amount = 6;
    prepare_wallet_input(&mut tx, &wallet);
    let mut set = UnsignedTx::default();
    set.txes.push(tx);
    assert!(set
        .validate_for_wallet(&keys.get_public_spend(), &keys.view)
        .is_ok());
    for first in [false, true] {
        set.txes[0].splitted_dsts[0].is_subaddress = first;
        set.txes[0].splitted_dsts[1].is_subaddress = !first;
        set.txes[0].splitted_dsts[1].original.clear();
        assert!(set
            .validate_for_wallet(&keys.get_public_spend(), &keys.view)
            .is_err());
        assert!(set.sign(&keys).is_err());
    }
    set.txes[0].splitted_dsts[0] = sub.clone();
    set.txes[0].splitted_dsts[1] = sub.clone();
    let mut entry = sub;
    entry.is_subaddress = false;
    set.txes[0].dests.push(entry);
    assert!(set.validate_destinations().is_err());
}

#[test]
fn short_nonce_destination_consistency() {
    use crate::key::{KeyPair, PrivateKey};
    use crate::transfer::UnsignedTx;
    let wallet = Party::new();
    let first = Party::new();
    let second = Party::new();
    let keys = KeyPair::new(PrivateKey::new(wallet.view), PrivateKey::new(wallet.spend));
    let mut tx = tx_data(
        vec![first.standard_dest(2), second.standard_dest(2)],
        wallet.standard_dest(1),
    );
    tx.sources[0].amount = 6;
    prepare_wallet_input(&mut tx, &wallet);
    let mut set = UnsignedTx::default();
    set.txes.push(tx);
    assert!(set
        .validate_for_wallet(&keys.get_public_spend(), &keys.view)
        .is_ok());
    set.txes[0].extra = vec![2, 9, 1, 0, 0, 0, 0, 0, 0, 0, 0];
    assert!(set
        .validate_for_wallet(&keys.get_public_spend(), &keys.view)
        .is_err());
    assert!(set.sign(&keys).is_err());
    set.txes[0].splitted_dsts[1] = first.standard_dest(2);
    assert!(set
        .validate_for_wallet(&keys.get_public_spend(), &keys.view)
        .is_ok());
    assert!(set.txes[0].payment_id_destination_index().is_some());
}

#[test]
fn ring_member_identity_after_sorting() {
    use crate::transfer::UnsignedTx;
    use crate::utils::io::write_tx_construction_data;
    let wallet = Party::new();
    let mut tx = tx_data(vec![wallet.standard_dest(1)], wallet.standard_dest(0));
    prepare_wallet_input(&mut tx, &wallet);
    tx.sources[0].outputs[0].index = 20;
    let selected = tx.sources[0].outputs[0].clone();
    let mut other = selected.clone();
    other.index = 10;
    other.key.mask = wallet.view_point().compress().to_bytes();
    tx.sources[0].outputs.push(other);
    let mut bytes = vec![2, 1];
    bytes.extend(write_tx_construction_data(&tx));
    bytes.push(0);
    let decoded = UnsignedTx::deserialize(&bytes).unwrap();
    let source = &decoded.txes[0].sources[0];
    assert_eq!(source.real_output, 1);
    let restored = &source.outputs[source.real_output as usize];
    assert_eq!(restored.index, selected.index);
    assert_eq!(restored.key.mask, selected.key.mask);
}

#[test]
fn public_key_text_boundaries() {
    use crate::key::PublicKey;
    for text in ["not-hex", "0", "", "ff", &"ff".repeat(33)] {
        assert!(PublicKey::from_str(text).is_err());
    }
    let point = Party::new().spend_point().compress().to_bytes();
    assert_eq!(
        PublicKey::from_str(&hex::encode(point)).unwrap().as_bytes(),
        point
    );
}

#[test]
fn receive_address_key_validation() {
    use crate::address::generate_address;
    use crate::key::{PrivateKey, PublicKey};
    use curve25519_dalek::constants::EIGHT_TORSION;
    let wallet = Party::new();
    let spend = PrivateKey::new(wallet.spend).get_public_key();
    let view = PrivateKey::new(wallet.view);
    let invalid_encoding = (0u8..=255)
        .map(|byte| [byte; 32])
        .find(|bytes| {
            curve25519_dalek::edwards::CompressedEdwardsY(*bytes)
                .decompress()
                .is_none()
        })
        .unwrap();
    let mut noncanonical = [255; 32];
    noncanonical[0] = 238;
    noncanonical[31] = 127;
    for bytes in [
        invalid_encoding,
        noncanonical,
        EIGHT_TORSION[0].compress().to_bytes(),
        EIGHT_TORSION[1].compress().to_bytes(),
        (wallet.spend_point() + EIGHT_TORSION[1])
            .compress()
            .to_bytes(),
    ] {
        let key = PublicKey::from_bytes(&bytes).unwrap();
        for subaddress in [false, true] {
            assert!(generate_address(&key, &view, 0, 1, subaddress).is_err());
        }
    }
    for subaddress in [false, true] {
        assert!(generate_address(&spend, &view, 0, 1, subaddress).is_ok());
        assert!(generate_address(&spend, &PrivateKey::default(), 0, 1, subaddress).is_err());
    }
}

#[test]
fn request_display_count_boundaries() {
    use crate::errors::MoneroError;
    use crate::key::{KeyPair, PrivateKey};
    use crate::transfer::UnsignedTx;
    let wallet = Party::new();
    let recipient = Party::new();
    let keys = KeyPair::new(PrivateKey::new(wallet.view), PrivateKey::new(wallet.spend));
    let mut tx = tx_data(vec![recipient.standard_dest(1)], wallet.standard_dest(1));
    tx.sources[0].amount = 32;
    prepare_wallet_input(&mut tx, &wallet);
    let mut set = UnsignedTx::default();
    set.txes = vec![tx.clone(); 8];
    assert!(set
        .validate_for_wallet(&keys.get_public_spend(), &keys.view)
        .is_ok());
    set.txes.push(tx.clone());
    assert_eq!(
        set.validate_for_wallet(&keys.get_public_spend(), &keys.view),
        Err(MoneroError::RequestTooComplex)
    );
    assert!(matches!(
        set.sign(&keys),
        Err(MoneroError::RequestTooComplex)
    ));
    set.txes = vec![tx.clone()];
    set.txes[0].sources = vec![tx.sources[0].clone(); 16];
    assert!(set
        .validate_for_wallet(&keys.get_public_spend(), &keys.view)
        .is_ok());
    set.txes[0].sources.push(tx.sources[0].clone());
    assert_eq!(
        set.validate_for_wallet(&keys.get_public_spend(), &keys.view),
        Err(MoneroError::RequestTooComplex)
    );
    set.txes = vec![tx.clone(); 4];
    for t in &mut set.txes {
        t.splitted_dsts = vec![recipient.standard_dest(1); 16];
    }
    assert_eq!(
        set.validate_for_wallet(&keys.get_public_spend(), &keys.view),
        Err(MoneroError::RequestTooComplex)
    );
}

fn prepare_wallet_input(tx: &mut TxConstructionData, wallet: &Party) {
    let r = Scalar::from(19u64);
    let source = &mut tx.sources[0];
    source.real_out_tx_key = EdwardsPoint::mul_base(&r).compress().to_bytes();
    let derivation = (wallet.view * EdwardsPoint::mul_base(&r)).mul_by_cofactor();
    let mut data = derivation.compress().to_bytes().to_vec();
    data.extend(crate::utils::io::write_varinteger(
        source.real_output_in_tx_index,
    ));
    let mut offset = crate::utils::hash::hash_to_scalar(&data);
    let minor = tx.subaddr_indices.first().copied().unwrap_or(0);
    if tx.subaddr_account != 0 || minor != 0 {
        offset += Scalar::from_bytes_mod_order(crate::key::calc_subaddress_m(
            &wallet.view.to_bytes(),
            tx.subaddr_account,
            minor,
        ));
    }
    let real = &mut source.outputs[source.real_output as usize];
    real.key.dest = (wallet.spend_point() + EdwardsPoint::mul_base(&offset))
        .compress()
        .to_bytes();
    real.key.mask = monero_serai::primitives::Commitment {
        mask: Scalar::from_bytes_mod_order(source.mask),
        amount: source.amount,
    }
    .calculate()
    .compress()
    .to_bytes();
}

#[test]
fn preflight_input_validation() {
    use crate::key::PrivateKey;
    use crate::transfer::UnsignedTx;
    let wallet = Party::new();
    let recipient = Party::new();
    let mut tx = tx_data(vec![recipient.standard_dest(2)], wallet.standard_dest(1));
    tx.sources[0].amount = 4;
    prepare_wallet_input(&mut tx, &wallet);
    let mut set = UnsignedTx::default();
    set.txes.push(tx);
    let spend = PrivateKey::new(wallet.spend).get_public_key();
    let view = PrivateKey::new(wallet.view);
    assert!(set.validate_for_wallet(&spend, &view).is_ok());
    let valid = set.clone();
    for (major, minor) in [(0, 0), (0, 2), (1, 0)] {
        for additional in [false, true] {
            let mut candidate = valid.clone();
            let tx = &mut candidate.txes[0];
            tx.subaddr_account = major;
            tx.subaddr_indices = vec![minor];
            tx.sources[0].real_output_in_tx_index = 2;
            prepare_wallet_input(tx, &wallet);
            if additional {
                let key = tx.sources[0].real_out_tx_key;
                tx.sources[0].real_out_additional_tx_keys = vec![key; 3];
                tx.sources[0].real_out_tx_key = EdwardsPoint::mul_base(&Scalar::from(23u64))
                    .compress()
                    .to_bytes();
            }
            assert!(candidate.validate_for_wallet(&spend, &view).is_ok());
            let keys = crate::key::KeyPair::new(
                PrivateKey::new(wallet.view),
                PrivateKey::new(wallet.spend),
            );
            assert!(candidate.txes[0].inputs(&keys).is_ok());
        }
    }
    set.txes[0].sources[0].outputs[0].key.dest = recipient.spend_point().compress().to_bytes();
    assert!(set.validate_for_wallet(&spend, &view).is_err());
    set = valid.clone();
    set.txes[0].sources[0].mask = Scalar::from(9u64).to_bytes();
    assert!(set.validate_for_wallet(&spend, &view).is_err());
    set = valid;
    set.txes[0].sources[0].amount += 1;
    assert!(set.validate_for_wallet(&spend, &view).is_err());
}

#[test]
fn preflight_attached_transfer_validation() {
    use crate::errors::MoneroError;
    use crate::key::{KeyPair, PrivateKey, PublicKey};
    use crate::outputs::ExportedTransferDetail;
    use crate::transfer::UnsignedTx;
    let attach = |set: &UnsignedTx, details: Vec<ExportedTransferDetail>| {
        use crate::utils::io::{write_tx_construction_data, write_varinteger};
        let mut bytes = vec![2];
        bytes.extend(write_varinteger(set.txes.len() as u64));
        for tx in &set.txes {
            bytes.extend(write_tx_construction_data(tx));
        }
        bytes.extend([3, 0, 0]);
        bytes.extend(write_varinteger(details.len() as u64));
        for detail in details {
            bytes.push(0);
            bytes.extend(detail.pubkey);
            bytes.extend(write_varinteger(detail.internal_output_index));
            bytes.extend(write_varinteger(detail.global_output_index));
            bytes.extend(detail.tx_pubkey);
            bytes.push(detail.flags);
            bytes.extend(write_varinteger(detail.amount));
            bytes.extend(write_varinteger(detail.additional_tx_keys.len() as u64));
            for key in detail.additional_tx_keys {
                bytes.extend(key.as_bytes());
            }
            bytes.extend(write_varinteger(detail.major as u64));
            bytes.extend(write_varinteger(detail.minor as u64));
        }
        UnsignedTx::deserialize(&bytes).unwrap()
    };
    let wallet = Party::new();
    let recipient = Party::new();
    let keys = KeyPair::new(PrivateKey::new(wallet.view), PrivateKey::new(wallet.spend));
    let mut tx = tx_data(vec![recipient.standard_dest(2)], wallet.standard_dest(1));
    tx.sources[0].amount = 4;
    prepare_wallet_input(&mut tx, &wallet);
    let mut set = UnsignedTx::default();
    set.txes.push(tx);
    for (major, minor) in [(0, 0), (0, 2), (1, 0)] {
        for additional in [false, true] {
            let mut input = set.txes[0].clone();
            input.subaddr_account = major;
            input.subaddr_indices = vec![minor];
            input.sources[0].real_output_in_tx_index = 2;
            prepare_wallet_input(&mut input, &wallet);
            let source = &input.sources[0];
            let mut detail = ExportedTransferDetail {
                pubkey: source.outputs[0].key.dest,
                internal_output_index: 2,
                global_output_index: 10,
                tx_pubkey: source.real_out_tx_key,
                flags: 4,
                amount: source.amount,
                additional_tx_keys: vec![],
                major,
                minor,
            };
            if additional {
                detail.additional_tx_keys =
                    vec![PublicKey::from_bytes(&detail.tx_pubkey).unwrap(); 3];
                detail.tx_pubkey = EdwardsPoint::mul_base(&Scalar::from(23u64))
                    .compress()
                    .to_bytes();
            }
            set = attach(&set, vec![detail.clone()]);
            assert!(set
                .validate_for_wallet(&keys.get_public_spend(), &keys.view)
                .is_ok());
            assert!(detail.generate_key_image_without_signature(&keys).is_ok());
            let mut other = detail.clone();
            other.pubkey = recipient.spend_point().compress().to_bytes();
            set = attach(&set, vec![detail.clone(), other]);
            assert_eq!(
                set.validate_for_wallet(&keys.get_public_spend(), &keys.view),
                Err(MoneroError::GenerateKeyImageError)
            );
            assert!(matches!(
                set.sign(&keys),
                Err(MoneroError::GenerateKeyImageError)
            ));
            detail.minor += 1;
            set = attach(&set, vec![detail]);
            assert!(set
                .validate_for_wallet(&keys.get_public_spend(), &keys.view)
                .is_err());
        }
    }
}
