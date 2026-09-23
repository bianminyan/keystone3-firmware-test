use crate::errors::{MoneroError, Result};
use crate::extra::*;
use crate::key::*;
use crate::transfer::{AccountPublicAddress, TxConstructionData, TxDestinationEntry};
use crate::utils::*;
use alloc::vec;
use alloc::vec::Vec;
use curve25519_dalek::edwards::EdwardsPoint;
use curve25519_dalek::scalar::Scalar;
use monero_serai::primitives::Commitment;
use monero_serai::ringct::EncryptedAmount;
use monero_wallet::SharedKeyDerivations;
use rand_core::OsRng;
use zeroize::Zeroizing;

/// The parts of the request's `extra` this device consumes.
#[derive(Debug, Default, Clone)]
pub(crate) struct RequestedExtra {
    /// The nonce field (tag 0x02) verbatim, if the request carried one.
    pub nonce: Option<Vec<u8>>,
    /// Number of padding fields (each `0x00` byte on the wire is one).
    pub padding: usize,
}

/// Classification of the transaction's destinations.
///
/// Ported from upstream `classify_addresses()`
/// (`src/cryptonote_core/cryptonote_tx_utils.cpp:79-102`), which is the only
/// input the transaction-key selection may depend on:
///
/// * the change address is not a payment and is skipped,
/// * destinations are counted once per unique address,
/// * the single non-change subaddress destination (if any) is remembered because
///   it defines `R = r*D` in the one case upstream allows it.
#[derive(Debug, Default, Clone, Copy)]
struct DestinationClassification {
    num_standard: usize,
    num_subaddress: usize,
    single_subaddress_spend: Option<[u8; 32]>,
}

impl TxConstructionData {
    fn classify_destinations(&self) -> DestinationClassification {
        // Upstream compares addresses only (`dst_entr.addr == change_addr`).
        let change_addr = self.change_dts.addr;
        let mut unique: Vec<AccountPublicAddress> = vec![];
        let mut res = DestinationClassification::default();

        for dst in self.splitted_dsts.iter() {
            if dst.addr == change_addr || unique.contains(&dst.addr) {
                continue;
            }
            unique.push(dst.addr);

            if dst.is_subaddress {
                res.num_subaddress += 1;
                res.single_subaddress_spend = Some(dst.addr.spend_public_key);
            } else {
                res.num_standard += 1;
            }
        }

        res
    }

    /// Upstream `need_additional_txkeys`
    /// (`src/cryptonote_core/cryptonote_tx_utils.cpp:440`):
    /// a subaddress destination needs its own transaction key whenever another
    /// destination of a different kind exists, because a single `R` can only
    /// serve one kind of destination.
    fn should_use_additional_keys(&self) -> bool {
        let classification = self.classify_destinations();
        classification.num_subaddress > 0
            && (classification.num_standard > 0 || classification.num_subaddress > 1)
    }

    pub fn transaction_keys(
        &self,
    ) -> (PrivateKey, Vec<PrivateKey>, EdwardsPoint, Vec<EdwardsPoint>) {
        let classification = self.classify_destinations();
        let should_use_additional_keys = self.should_use_additional_keys();

        let tx_key = PrivateKey::from_bytes(generate_random_scalar(&mut OsRng).as_bytes());
        let mut additional_keys = vec![];
        if should_use_additional_keys {
            for _ in 0..self.splitted_dsts.len() {
                additional_keys.push(PrivateKey::from_bytes(
                    generate_random_scalar(&mut OsRng).as_bytes(),
                ));
            }
        }

        // `R = r*G` unless the single non-change destination is a subaddress, in
        // which case upstream uses `R = r*D`
        // (`src/cryptonote_core/cryptonote_tx_utils.cpp:423-426`). The spend key is
        // validated by the caller (`UnsignedTx::sign` / `display_info`), so the
        // decompression below cannot fail on a checked payload.
        let mut tx_key_pub = EdwardsPoint::mul_base(&tx_key.scalar);
        let mut additional_keys_pub = vec![];

        if should_use_additional_keys {
            for (additional_key, dest) in
                additional_keys.clone().into_iter().zip(&self.splitted_dsts)
            {
                let spend = PublicKey::from_bytes(&dest.addr.spend_public_key).unwrap();
                if dest.is_subaddress {
                    additional_keys_pub
                        .push(additional_key.scalar * spend.point.decompress().unwrap());
                } else {
                    additional_keys_pub.push(EdwardsPoint::mul_base(&additional_key.scalar))
                }
            }
        } else if classification.num_standard == 0 && classification.num_subaddress == 1 {
            if let Some(spend) = classification.single_subaddress_spend {
                tx_key_pub = tx_key.scalar
                    * PublicKey::from_bytes(&spend)
                        .unwrap()
                        .point
                        .decompress()
                        .unwrap();
            }
        }

        (tx_key, additional_keys, tx_key_pub, additional_keys_pub)
    }

    pub fn is_change_dest(&self, dest: &TxDestinationEntry) -> bool {
        dest.addr == self.change_dts.addr
    }

    /// True when a non-zero output pays the host-declared change address, i.e.
    /// when upstream would require that address to be recognized as ours
    /// (`cryptonote_tx_utils.cpp:450-453`).
    pub fn has_change_output(&self) -> bool {
        let change_addr = self.change_dts.addr;
        self.splitted_dsts
            .iter()
            .any(|dst| dst.addr == change_addr && dst.amount > 0)
    }

    fn ecdhs(
        &self,
        keypair: &KeyPair,
        tx_key: &PrivateKey,
        additional_keys: &Vec<PrivateKey>,
        tx_key_pub: &EdwardsPoint,
    ) -> Vec<EdwardsPoint> {
        let mut res = Vec::with_capacity(self.splitted_dsts.len());
        for (i, dest) in self.splitted_dsts.iter().enumerate() {
            let key_to_use = if dest.is_subaddress {
                additional_keys.get(i).unwrap_or(tx_key)
            } else {
                tx_key
            };
            res.push(if !self.is_change_dest(dest) {
                key_to_use.scalar
                    * PublicKey::from_bytes(&dest.addr.view_public_key)
                        .unwrap()
                        .point
                        .decompress()
                        .unwrap()
            } else {
                keypair.view.scalar * tx_key_pub
            });
        }

        res
    }

    fn payment_id_xors(
        &self,
        keypair: &KeyPair,
        tx_key: &PrivateKey,
        additional_keys: &Vec<PrivateKey>,
        tx_key_pub: &EdwardsPoint,
    ) -> Vec<[u8; 8]> {
        let mut res = Vec::with_capacity(self.splitted_dsts.len());
        for ecdh in self.ecdhs(keypair, tx_key, additional_keys, tx_key_pub) {
            res.push(SharedKeyDerivations::payment_id_xor(Zeroizing::new(ecdh)));
        }
        res
    }

    pub(crate) fn requested_extra(&self) -> Result<RequestedExtra> {
        let mut res = RequestedExtra::default();
        let mut offset = 0usize;

        // Advance by `delta` bytes, rejecting anything that leaves the buffer.
        fn advance(offset: usize, delta: usize, len: usize) -> Result<usize> {
            let next = offset.checked_add(delta).ok_or(MoneroError::FormatError)?;
            if next > len {
                return Err(MoneroError::FormatError);
            }
            Ok(next)
        }

        while offset < self.extra.len() {
            let tag = self.extra[offset];
            offset += 1;

            match tag {
                // On the wire every `0x00` byte is one padding field.
                0x00 => res.padding += 1,
                0x01 => offset = advance(offset, 32, self.extra.len())?,
                0x02 => {
                    let len = *self.extra.get(offset).ok_or(MoneroError::FormatError)? as usize;
                    offset = advance(offset, 1, self.extra.len())?;
                    let end = advance(offset, len, self.extra.len())?;
                    res.nonce = Some(self.extra[offset..end].to_vec());
                    offset = end;
                }
                0x03 => {
                    let len = *self.extra.get(offset).ok_or(MoneroError::FormatError)? as usize;
                    offset = advance(offset, 1 + len + 32, self.extra.len())?;
                }
                0x04 => {
                    let count = *self.extra.get(offset).ok_or(MoneroError::FormatError)? as usize;
                    offset = advance(offset, 1 + 32 * count, self.extra.len())?;
                }
                _ => return Err(MoneroError::FormatError),
            }
        }

        Ok(res)
    }

    fn destination_payment_id_xor(&self, payment_id_xors: &[[u8; 8]]) -> Option<[u8; 8]> {
        let change_addr = self.change_dts.addr;
        let mut unique: Vec<AccountPublicAddress> = vec![];
        let mut chosen: Option<[u8; 8]> = None;

        for (dst, xor) in self.splitted_dsts.iter().zip(payment_id_xors) {
            if dst.amount == 0 || dst.addr == change_addr || unique.contains(&dst.addr) {
                continue;
            }
            unique.push(dst.addr);
            if unique.len() > 1 {
                return None;
            }
            chosen = Some(*xor);
        }

        if chosen.is_none() {
            // No non-change destination: fall back to the change address.
            return self
                .splitted_dsts
                .iter()
                .zip(payment_id_xors)
                .find(|(dst, _)| dst.addr == change_addr)
                .map(|(_, xor)| *xor);
        }

        chosen
    }

    pub fn extra(
        &self,
        keypair: &KeyPair,
        tx_key: &PrivateKey,
        additional_keys: &Vec<PrivateKey>,
        tx_key_pub: &EdwardsPoint,
        additional_keys_pub: &Vec<EdwardsPoint>,
    ) -> Result<Vec<u8>> {
        let payment_id_xors = self.payment_id_xors(keypair, tx_key, additional_keys, tx_key_pub);
        let mut extra = Extra::new(*tx_key_pub, additional_keys_pub.clone());

        // The payment ID from the request, re-encrypted with the destination's
        // shared secret exactly as upstream does; a long (0x00) ID or arbitrary
        // nonce data is carried verbatim. `sign` rejects a malformed extra before
        // reaching this point, so the fallback only avoids a panic.
        let requested = self.requested_extra().unwrap_or_default();
        let requested_nonce = requested.nonce;
        match requested_nonce {
            Some(nonce) if nonce.first() == Some(&1) && nonce.len() == 9 => {
                // An encrypted payment ID can only be XORed with one destination's
                // shared secret. Upstream refuses the transaction when that
                // destination is ambiguous ("Destinations have to have exactly one
                // output to support encrypted payment ids"), and so do we: writing
                // the plaintext ID in the "encrypted" format would make the recipient
                // decrypt garbage.
                let xor = self
                    .destination_payment_id_xor(&payment_id_xors)
                    .ok_or(MoneroError::FormatError)?;
                let mut id_vec = Vec::with_capacity(1 + 8);
                id_vec.push(1);
                id_vec.extend(nonce[1..9].iter().zip(xor.iter()).map(|(a, b)| a ^ b));
                extra.push_nonce(id_vec);
            }
            Some(nonce) => extra.push_nonce(nonce),
            // No payment ID requested: add the dummy (zero) encrypted one for the
            // usual one destination plus change, and skip it beyond two outputs
            // (`cryptonote_tx_utils.cpp:310-330`).
            None => {
                if self.splitted_dsts.len() <= 2 {
                    if let Some(payment_id_xor) = self.destination_payment_id_xor(&payment_id_xors)
                    {
                        let mut id_vec = Vec::with_capacity(1 + 8);
                        id_vec.push(1);
                        id_vec.extend_from_slice(&payment_id_xor);
                        extra.push_nonce(id_vec);
                    }
                }
            }
        }

        // Keep the wallet's padding so the transaction stays in the size class it
        // picked (wallet2 `pad_tx`).
        extra.push_padding(requested.padding);

        Ok(extra.serialize())
    }

    pub fn shared_key_derivations(
        &self,
        keypair: &KeyPair,
        tx_key: &PrivateKey,
        additional_keys: &Vec<PrivateKey>,
        tx_key_pub: &EdwardsPoint,
    ) -> Vec<Zeroizing<SharedKeyDerivations>> {
        let ecdhs = self.ecdhs(keypair, tx_key, additional_keys, tx_key_pub);
        let mut res = Vec::with_capacity(self.splitted_dsts.len());
        for (i, (_, ecdh)) in self.splitted_dsts.iter().zip(ecdhs).enumerate() {
            res.push(SharedKeyDerivations::output_derivations(
                None,
                Zeroizing::new(ecdh),
                i,
            ));
        }

        res
    }

    pub fn commitments_and_encrypted_amounts(
        &self,
        keypair: &KeyPair,
        tx_key: &PrivateKey,
        additional_keys: &Vec<PrivateKey>,
        tx_key_pub: &EdwardsPoint,
    ) -> Vec<(Commitment, EncryptedAmount)> {
        let shared_key_derivations =
            self.shared_key_derivations(keypair, tx_key, additional_keys, tx_key_pub);

        let mut res = Vec::with_capacity(self.splitted_dsts.len());
        for (dest, shared_key_derivation) in self.splitted_dsts.iter().zip(shared_key_derivations) {
            let amount = dest.amount;

            let commitment = Commitment::new(shared_key_derivation.commitment_mask(), amount);
            let encrypted_amount = EncryptedAmount::Compact {
                amount: shared_key_derivation.compact_amount_encryption(amount),
            };
            res.push((commitment, encrypted_amount));
        }

        res
    }

    pub fn sum_output_masks(
        &self,
        keypair: &KeyPair,
        tx_key: &PrivateKey,
        additional_keys: &Vec<PrivateKey>,
        tx_key_pub: &EdwardsPoint,
    ) -> Scalar {
        self.commitments_and_encrypted_amounts(keypair, tx_key, additional_keys, tx_key_pub)
            .into_iter()
            .map(|(commitment, _)| commitment.mask)
            .sum()
    }
}
