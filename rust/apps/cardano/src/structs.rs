use crate::address::derive_pubkey_hash;
use crate::errors::{CardanoError, Result};
use alloc::collections::BTreeMap;
use alloc::string::{String, ToString};
use alloc::vec;
use alloc::vec::Vec;
use app_utils::{impl_internal_struct, impl_public_struct};
use cardano_serialization_lib::{
    Address, BaseAddress, CredKind, Credential, DRepKind, EnterpriseAddress,
    FixedTransaction as Transaction, GovernanceActionKind, MIRKind, MIRPot, MultiAsset,
    NetworkIdKind, RewardAddress, VoteKind, VoterKind,
};

use alloc::format;
use bitcoin::bip32::ChildNumber::{Hardened, Normal};
use bitcoin::bip32::DerivationPath;
use core::ops::Div;
use cryptoxide::hashing::blake2b_224;
use hex;

impl_public_struct!(ParseContext {
    utxos: Vec<CardanoUtxo>,
    cert_keys: Vec<CardanoCertKey>,
    cardano_xpub: Option<String>,
    master_fingerprint: Vec<u8>
});
impl_public_struct!(ParsedCardanoSignCip8Data {
    payload: String,
    payload_is_hex: bool,
    derivation_path: String,
    message_hash: String,
    xpub: String,
    hash_payload: bool
});

impl_public_struct!(CardanoUtxo {
    master_fingerprint: Vec<u8>,
    address: String,
    path: DerivationPath,
    value: u64,
    transaction_hash: Vec<u8>,
    index: u32
});

impl_public_struct!(CardanoCertKey {
    master_fingerprint: Vec<u8>,
    key_hash: Vec<u8>,
    path: DerivationPath
});

impl_public_struct!(ParsedCardanoSignData {
    payload: String,
    payload_is_hex: bool,
    derivation_path: String,
    message_hash: String,
    xpub: String
});

impl_public_struct!(VotingProcedure {
    voter_type: String,
    voter: String,
    transaction_id: String,
    index: String,
    vote: String
});

impl_public_struct!(VotingProposal {
    action: String,
    deposit: String,
    reward_account: String,
    anchor_url: String,
    anchor_data_hash: String
});

impl_public_struct!(ParsedCardanoTx {
    fee: String,
    total_input: String,
    total_output: String,
    from: Vec<CardanoFrom>,
    to: Vec<CardanoTo>,
    network: String,
    ttl: Option<String>,
    validity_start: Option<String>,
    withdrawals_total: Option<String>,
    total_collateral: Option<String>,
    collateral_return: Option<String>,
    collateral_inputs_count: u32,
    reference_inputs_count: u32,
    donation: Option<String>,
    required_signers_count: u32,
    governance_votes_count: u32,
    governance_proposals_count: u32,
    transaction_is_valid: bool,
    certificates: Vec<CardanoCertificate>,
    withdrawals: Vec<CardanoWithdrawal>,
    auxiliary_data: Option<String>,
    governance_data: Option<String>,
    advanced_data: Option<String>,
    voting_procedures: Vec<VotingProcedure>,
    voting_proposals: Vec<VotingProposal>,
    mint_assets: Vec<ParsedCardanoNativeAsset>,
    collateral_assets: Vec<ParsedCardanoNativeAsset>,
    has_multi_assets: bool,
    has_unknown_inputs: bool
});

impl_public_struct!(SignDataResult {
    pub_key: Vec<u8>,
    signature: Vec<u8>
});

impl_public_struct!(SignVotingRegistrationResult {
    signature: Vec<u8>
});

impl_public_struct!(CertField {
    label: String,
    value: String
});

impl_public_struct!(CardanoCertificate {
    cert_type: String,
    fields: Vec<CertField>
});

const LABEL_ADDRESS: &str = "Address";
const LABEL_POOL: &str = "Pool";
const LABEL_DEPOSIT: &str = "Deposit";
const LABEL_DREP: &str = "DRep";
const LABEL_VOTE: &str = "Vote";
#[allow(unused)]
const LABEL_ABCHOR: &str = "Anchor";
const LABEL_ANCHOR_URL: &str = "Anchor URL";
const LABEL_ANCHOR_DATA_HASH: &str = "Anchor Data Hash";
const LABEL_COLD_KEY: &str = "Cold Key";
const LABEL_HOT_KEY: &str = "Hot Key";

impl_public_struct!(CardanoWithdrawal {
    address: String,
    amount: String
});

impl_internal_struct!(ParsedCardanoInput {
    transaction_id: String,
    index: u32,
    value: Option<u64>,
    address: Option<String>,
    path: Option<String>
});

impl_public_struct!(CardanoFrom {
    address: String,
    amount: String,
    path: Option<String>,
    transaction_id: String,
    index: u32,
    known: bool,
    value: u64
});

impl_public_struct!(CardanoTo {
    address: String,
    amount: String,
    assets: BTreeMap<String, ParsedCardanoMultiAsset>,
    assets_text: Option<String>,
    value: u64
});

impl_internal_struct!(ParsedCardanoOutput {
    address: String,
    value: u64,
    assets: Option<Vec<ParsedCardanoMultiAsset>>
});

impl_public_struct!(ParsedCardanoMultiAsset {
    id: String,
    policy_id: Vec<u8>,
    name: Vec<u8>,
    amount: String,
    value: u64
});

impl_public_struct!(ParsedCardanoNativeAsset {
    policy_id: Vec<u8>,
    name: Vec<u8>,
    amount: String
});

impl ParsedCardanoSignData {
    pub fn into_display_parts(self) -> (String, bool, String, String, String) {
        (
            self.payload,
            self.payload_is_hex,
            self.derivation_path,
            self.message_hash,
            self.xpub,
        )
    }

    pub fn build(sign_data: Vec<u8>, derivation_path: String, xpub: String) -> Result<Self> {
        match decode_sign_payload(&sign_data) {
            Ok(raw_payload) => {
                let (payload, payload_is_hex) = match String::from_utf8(raw_payload.to_vec()) {
                    Ok(payload) => (payload, false),
                    Err(_) => (hex::encode(raw_payload), true),
                };
                Ok(Self {
                    payload,
                    payload_is_hex,
                    derivation_path,
                    message_hash: hex::encode(raw_payload),
                    xpub,
                })
            }
            Err(_) => {
                let raw_data = hex::encode(sign_data);
                Ok(Self {
                    payload: raw_data.clone(),
                    payload_is_hex: true,
                    derivation_path,
                    message_hash: raw_data,
                    xpub,
                })
            }
        }
    }
}

impl ParseContext {
    pub fn utxos_len(&self) -> usize {
        self.utxos.len()
    }
}

impl ParsedCardanoSignCip8Data {
    pub fn into_display_parts(self) -> (String, bool, String, String, String, bool) {
        (
            self.payload,
            self.payload_is_hex,
            self.derivation_path,
            self.message_hash,
            self.xpub,
            self.hash_payload,
        )
    }

    pub fn build(
        sign_data: Vec<u8>,
        derivation_path: String,
        xpub: String,
        hash_payload: bool,
    ) -> Result<Self> {
        match decode_sign_payload(&sign_data) {
            Ok(raw_payload) => {
                let (payload, payload_is_hex) = match String::from_utf8(raw_payload.to_vec()) {
                    Ok(payload) => (payload, false),
                    Err(_) => (hex::encode(raw_payload), true),
                };
                // Keep this identical to the bytes transformed by the signer.
                // With hash_payload enabled the signer hashes the complete
                // incoming Sig_structure, not only its decoded payload field.
                let message_hash = if hash_payload {
                    hex::encode(blake2b_224(&sign_data))
                } else {
                    hex::encode(raw_payload)
                };
                Ok(Self {
                    payload,
                    payload_is_hex,
                    derivation_path,
                    message_hash,
                    xpub,
                    hash_payload,
                })
            }
            Err(_) => {
                let raw_data = hex::encode(sign_data);
                Ok(Self {
                    payload: raw_data.clone(),
                    payload_is_hex: true,
                    derivation_path,
                    message_hash: raw_data,
                    xpub,
                    hash_payload,
                })
            }
        }
    }
}

fn decode_sign_payload(data: &[u8]) -> core::result::Result<&[u8], minicbor::decode::Error> {
    use minicbor::data::Type;

    let mut decoder = minicbor::Decoder::new(data);
    let length = decoder.array()?;
    let mut index = 0u64;
    loop {
        if index == 3 {
            return decoder.bytes();
        }
        decoder.skip()?;
        index += 1;
        if length.is_some_and(|length| index >= length)
            || matches!(decoder.datatype()?, Type::Break)
        {
            return Err(minicbor::decode::Error::message(
                "missing Cardano sign payload",
            ));
        }
    }
}

impl ParsedCardanoTx {
    const MAX_NATIVE_ASSETS_PER_OUTPUT: usize = 64;
    const MAX_NATIVE_ASSETS_TOTAL: usize = 128;

    fn governance_action_name(kind: GovernanceActionKind) -> &'static str {
        match kind {
            GovernanceActionKind::ParameterChangeAction => "Parameter change",
            GovernanceActionKind::HardForkInitiationAction => "Hard fork initiation",
            GovernanceActionKind::TreasuryWithdrawalsAction => "Treasury withdrawals",
            GovernanceActionKind::NoConfidenceAction => "No confidence",
            GovernanceActionKind::UpdateCommitteeAction => "Update committee",
            GovernanceActionKind::NewConstitutionAction => "New constitution",
            GovernanceActionKind::InfoAction => "Information",
        }
    }

    fn voter_type_name(kind: VoterKind) -> &'static str {
        match kind {
            VoterKind::ConstitutionalCommitteeHotKeyHash => "Committee key",
            VoterKind::ConstitutionalCommitteeHotScriptHash => "Committee script",
            VoterKind::DRepKeyHash => "DRep key",
            VoterKind::DRepScriptHash => "DRep script",
            VoterKind::StakingPoolKeyHash => "Stake pool key",
        }
    }

    #[allow(clippy::type_complexity)]
    pub fn into_display_parts(
        self,
    ) -> (
        String,
        String,
        String,
        Vec<CardanoFrom>,
        Vec<CardanoTo>,
        String,
        Option<String>,
        Option<String>,
        Option<String>,
        Option<String>,
        Option<String>,
        u32,
        u32,
        Option<String>,
        u32,
        u32,
        u32,
        bool,
        Vec<CardanoCertificate>,
        Vec<CardanoWithdrawal>,
        Option<String>,
        Option<String>,
        Option<String>,
        Vec<VotingProcedure>,
        Vec<VotingProposal>,
        Vec<ParsedCardanoNativeAsset>,
        Vec<ParsedCardanoNativeAsset>,
        bool,
        bool,
    ) {
        (
            self.fee,
            self.total_input,
            self.total_output,
            self.from,
            self.to,
            self.network,
            self.ttl,
            self.validity_start,
            self.withdrawals_total,
            self.total_collateral,
            self.collateral_return,
            self.collateral_inputs_count,
            self.reference_inputs_count,
            self.donation,
            self.required_signers_count,
            self.governance_votes_count,
            self.governance_proposals_count,
            self.transaction_is_valid,
            self.certificates,
            self.withdrawals,
            self.auxiliary_data,
            self.governance_data,
            self.advanced_data,
            self.voting_procedures,
            self.voting_proposals,
            self.mint_assets,
            self.collateral_assets,
            self.has_multi_assets,
            self.has_unknown_inputs,
        )
    }

    pub fn outputs_ref(&self) -> &[CardanoTo] {
        &self.to
    }

    pub fn mint_assets_ref(&self) -> &[ParsedCardanoNativeAsset] {
        &self.mint_assets
    }

    pub fn collateral_assets_ref(&self) -> &[ParsedCardanoNativeAsset] {
        &self.collateral_assets
    }

    pub fn from_cardano_tx(tx: Transaction, context: ParseContext) -> Result<Self> {
        Self::validate_native_asset_limits(&tx)?;
        let network_id = Self::judge_network_id(&tx, &context)?;
        let network = match network_id {
            Some(1) => "Cardano Mainnet".to_string(),
            Some(_) => "Cardano Testnet".to_string(),
            None => "Unknown".to_string(),
        };
        let parsed_inputs = Self::parse_inputs(&tx, &context, network_id)?;
        let has_unknown_inputs = parsed_inputs
            .iter()
            .any(|input| input.value.is_none() || input.address.is_none());
        let parsed_outputs = Self::parse_outputs(&tx)?;
        let mint_assets = Self::parse_mint_assets(&tx);
        let collateral_assets = Self::parse_collateral_assets(&tx);
        let has_multi_assets = contains_multi_assets(&parsed_outputs);

        let fee = u64::from(tx.body_ref().fee());

        let total_output_amount = {
            let _v = parsed_outputs.iter().fold(0u64, |acc, cur| acc + cur.value);
            normalize_coin(_v)
        };

        let total_input_amount = {
            let _v = parsed_inputs.iter().fold(0u64, |acc, cur| match cur.value {
                Some(v) => acc + v,
                None => acc,
            });
            normalize_coin(_v)
        };

        let from_list_detail = Self::get_from_list(parsed_inputs);
        let to_list_detail = Self::get_to_list(parsed_outputs);

        let fee = normalize_coin(fee);
        let withdrawals_total = tx.body_ref().withdrawals().as_ref().map(|withdrawals| {
            normalize_coin(
                withdrawals
                    .iter()
                    .fold(0u64, |total, (_, value)| total + u64::from(value)),
            )
        });
        let collateral_return = tx
            .body_ref()
            .collateral_return()
            .map(|output| normalize_coin(u64::from(output.amount().coin())));

        let voting_procedures = match tx.body_ref().voting_procedures_ref() {
            Some(v) => {
                let mut voting_procedures = vec![];
                for (voter, action, procedure) in v.iter() {
                    let vote = match procedure.vote_kind() {
                        VoteKind::No => "No".to_string(),
                        VoteKind::Yes => "Yes".to_string(),
                        VoteKind::Abstain => "Abstain".to_string(),
                    };
                    let voter_hash = voter
                        .to_key_hash()
                        .map(|hash| hash.to_string())
                        .or_else(|| {
                            voter
                                .to_constitutional_committee_hot_credential()
                                .and_then(|credential| credential.to_scripthash())
                                .map(|hash| hash.to_string())
                        })
                        .or_else(|| {
                            voter
                                .to_drep_credential()
                                .and_then(|credential| credential.to_scripthash())
                                .map(|hash| hash.to_string())
                        })
                        .ok_or_else(|| {
                            CardanoError::InvalidTransaction(
                                "unsupported voting credential".to_string(),
                            )
                        })?;
                    let voter_type = Self::voter_type_name(voter.kind());
                    voting_procedures.push(VotingProcedure {
                        voter_type: voter_type.to_string(),
                        voter: voter_hash,
                        transaction_id: action.transaction_id_ref().to_string(),
                        index: action.index().to_string(),
                        vote,
                    })
                }
                voting_procedures
            }
            None => vec![],
        };

        let voting_proposals = match tx.body_ref().voting_proposals_ref() {
            Some(v) => {
                let mut voting_proposals = vec![];
                for proposal in v {
                    let action = Self::governance_action_name(proposal.governance_action().kind());
                    voting_proposals.push(VotingProposal {
                        action: action.to_string(),
                        deposit: normalize_coin(u64::from(proposal.deposit())),
                        reward_account: proposal
                            .reward_account()
                            .to_address()
                            .to_bech32(None)
                            .map_err(|e| CardanoError::InvalidTransaction(e.to_string()))?,
                        anchor_url: proposal.anchor_ref().url_ref().url(),
                        anchor_data_hash: proposal.anchor_ref().anchor_data_hash_ref().to_string(),
                    })
                }
                voting_proposals
            }
            None => vec![],
        };

        Ok(Self {
            total_input: total_input_amount,
            total_output: total_output_amount,
            from: from_list_detail,
            to: to_list_detail,
            fee,
            network,
            ttl: tx.body_ref().ttl_bignum().map(|value| value.to_string()),
            validity_start: tx
                .body_ref()
                .validity_start_interval_bignum()
                .map(|value| value.to_string()),
            withdrawals_total,
            total_collateral: tx
                .body_ref()
                .total_collateral()
                .map(|value| normalize_coin(u64::from(value))),
            collateral_return,
            collateral_inputs_count: tx
                .body_ref()
                .collateral()
                .map(|inputs| inputs.into_iter().count() as u32)
                .unwrap_or(0),
            reference_inputs_count: tx
                .body_ref()
                .reference_inputs()
                .map(|inputs| inputs.into_iter().count() as u32)
                .unwrap_or(0),
            donation: tx
                .body_ref()
                .donation()
                .map(|value| normalize_coin(u64::from(value))),
            required_signers_count: tx
                .body_ref()
                .required_signers()
                .map(|values| values.into_iter().count() as u32)
                .unwrap_or(0),
            governance_votes_count: tx
                .body_ref()
                .voting_procedures_ref()
                .map(|values| values.iter().count() as u32)
                .unwrap_or(0),
            governance_proposals_count: tx
                .body_ref()
                .voting_proposals_ref()
                .map(|values| values.into_iter().count() as u32)
                .unwrap_or(0),
            transaction_is_valid: tx.is_valid(),
            certificates: Self::parse_certificates(&tx, network_id)?,
            withdrawals: Self::parse_withdrawals(&tx)?,
            auxiliary_data: Self::parse_auxiliary_data(&tx)?,
            governance_data: None,
            advanced_data: Self::parse_advanced_data(&tx)?,
            voting_procedures,
            voting_proposals,
            mint_assets,
            collateral_assets,
            has_multi_assets,
            has_unknown_inputs,
        })
    }

    fn merge_network_id(network_id: &mut Option<u8>, candidate: u8) -> Result<()> {
        let candidate = if candidate == 1 { 1 } else { 0 };
        if let Some(current) = network_id {
            if *current != candidate {
                return Err(CardanoError::InvalidTransaction(
                    "conflicting transaction network sources".to_string(),
                ));
            }
        } else {
            *network_id = Some(candidate);
        }
        Ok(())
    }

    fn judge_network_id(tx: &Transaction, context: &ParseContext) -> Result<Option<u8>> {
        let mut network_id = None;
        if let Some(id) = tx.body_ref().network_id_ref() {
            Self::merge_network_id(
                &mut network_id,
                match id.kind() {
                    NetworkIdKind::Mainnet => 1,
                    NetworkIdKind::Testnet => 0,
                },
            )?;
        }

        for output in tx.body_ref().outputs() {
            if let Ok(candidate) = output.address_ref().network_id() {
                Self::merge_network_id(&mut network_id, candidate)?;
            }
        }
        if let Some(withdrawals) = tx.body_ref().withdrawals().as_ref() {
            for (address, _) in withdrawals.iter() {
                Self::merge_network_id(&mut network_id, address.network_id())?;
            }
        }
        if let Some(proposals) = tx.body_ref().voting_proposals_ref() {
            for proposal in proposals {
                Self::merge_network_id(&mut network_id, proposal.reward_account().network_id())?;
            }
        }
        if let Some(certificates) = tx.body_ref().certs_ref() {
            for certificate in certificates {
                if let Some(registration) = certificate.as_pool_registration_ref() {
                    Self::merge_network_id(
                        &mut network_id,
                        registration
                            .pool_params_ref()
                            .reward_account_ref()
                            .network_id(),
                    )?;
                }
            }
        }
        for input in tx.body_ref().inputs() {
            let tx_hash = input.transaction_id_ref().to_hex();
            if let Some(candidate) = context
                .utxos
                .iter()
                .find(|utxo| {
                    tx_hash.eq_ignore_ascii_case(&hex::encode(&utxo.transaction_hash))
                        && input.index() == utxo.index
                })
                .and_then(|utxo| Address::from_bech32(&utxo.address).ok())
                .and_then(|address| address.network_id().ok())
            {
                Self::merge_network_id(&mut network_id, candidate)?;
            }
        }
        Ok(network_id)
    }

    pub(crate) fn validate_native_asset_limits(tx: &Transaction) -> Result<()> {
        fn count_assets(multiasset: &MultiAsset) -> usize {
            multiasset
                .iter()
                .map(|(_, assets)| assets.iter().count())
                .sum()
        }

        let mut total = 0usize;
        for output in tx.body_ref().outputs() {
            let count = output
                .amount_ref()
                .multiasset_ref()
                .map(count_assets)
                .unwrap_or(0);
            if count > Self::MAX_NATIVE_ASSETS_PER_OUTPUT {
                return Err(CardanoError::InvalidTransaction(
                    "native asset count in one output exceeds 64 limit".to_string(),
                ));
            }
            total = total.checked_add(count).ok_or_else(|| {
                CardanoError::InvalidTransaction("native asset count overflow".to_string())
            })?;
        }

        if let Some(output) = tx.body_ref().collateral_return_ref() {
            let count = output
                .amount_ref()
                .multiasset_ref()
                .map(count_assets)
                .unwrap_or(0);
            if count > Self::MAX_NATIVE_ASSETS_PER_OUTPUT {
                return Err(CardanoError::InvalidTransaction(
                    "native asset count in collateral return exceeds 64 limit".to_string(),
                ));
            }
            total = total.checked_add(count).ok_or_else(|| {
                CardanoError::InvalidTransaction("native asset count overflow".to_string())
            })?;
        }

        if let Some(mint) = tx.body_ref().mint_ref() {
            for (_, assets) in mint.iter() {
                total = total.checked_add(assets.iter().count()).ok_or_else(|| {
                    CardanoError::InvalidTransaction("native asset count overflow".to_string())
                })?;
            }
        }

        if total > Self::MAX_NATIVE_ASSETS_TOTAL {
            return Err(CardanoError::InvalidTransaction(
                "native asset count exceeds 128 limit".to_string(),
            ));
        }
        Ok(())
    }

    fn parse_auxiliary_data(tx: &Transaction) -> Result<Option<String>> {
        tx.auxiliary_data_ref()
            .map(|v| {
                v.to_json()
                    .map_err(|e| CardanoError::InvalidTransaction(e.to_string()))
            })
            .transpose()
    }

    fn insert_json_field(
        map: &mut serde_json::Map<String, serde_json::Value>,
        key: &str,
        json: String,
    ) -> Result<()> {
        let value = serde_json::from_str(&json)
            .map_err(|e| CardanoError::InvalidTransaction(e.to_string()))?;
        map.insert(key.to_string(), Self::normalize_json_for_display(value));
        Ok(())
    }

    fn normalize_json_for_display(value: serde_json::Value) -> serde_json::Value {
        use serde_json::Value;

        match value {
            Value::String(text) => {
                let trimmed = text.trim();
                if trimmed.starts_with('{') || trimmed.starts_with('[') {
                    if let Ok(embedded) = serde_json::from_str::<Value>(trimmed) {
                        return Self::normalize_json_for_display(embedded);
                    }
                }
                Value::String(text)
            }
            Value::Array(values) => Value::Array(
                values
                    .into_iter()
                    .map(Self::normalize_json_for_display)
                    .collect(),
            ),
            Value::Object(values) => {
                let mut normalized = serde_json::Map::new();
                for (key, value) in values {
                    normalized.insert(key, Self::normalize_json_for_display(value));
                }

                if normalized.len() == 1 {
                    if normalized.contains_key("string") {
                        return normalized.remove("string").unwrap_or(Value::Null);
                    }
                    if normalized.contains_key("int") {
                        return normalized.remove("int").unwrap_or(Value::Null);
                    }
                    if let Some(Value::Array(values)) = normalized.remove("list") {
                        return Value::Array(values);
                    }
                    if let Some(Value::Array(entries)) = normalized.remove("map") {
                        let mut keys = BTreeMap::new();
                        let can_flatten = entries.iter().all(|entry| {
                            let Some(key) = entry.get("k").and_then(Value::as_str) else {
                                return false;
                            };
                            keys.insert(key.to_string(), ()).is_none() && entry.get("v").is_some()
                        });
                        if can_flatten {
                            let mut object = serde_json::Map::new();
                            for entry in entries {
                                if let Value::Object(mut fields) = entry {
                                    let key = match fields.remove("k") {
                                        Some(Value::String(key)) => key,
                                        _ => String::new(),
                                    };
                                    let value = fields.remove("v").unwrap_or(Value::Null);
                                    object.insert(key, value);
                                }
                            }
                            return Value::Object(object);
                        }
                        normalized.insert("map".to_string(), Value::Array(entries));
                    }
                }
                Value::Object(normalized)
            }
            value => value,
        }
    }

    fn finish_json(map: serde_json::Map<String, serde_json::Value>) -> Result<Option<String>> {
        if map.is_empty() {
            return Ok(None);
        }
        let json = serde_json::to_string_pretty(&serde_json::Value::Object(map))
            .map_err(|e| CardanoError::InvalidTransaction(e.to_string()))?;
        if json.len() > 64 * 1024 {
            return Err(CardanoError::InvalidTransaction(
                "advanced JSON exceeds 64 KiB limit".to_string(),
            ));
        }
        Ok(Some(json))
    }

    fn parse_advanced_data(tx: &Transaction) -> Result<Option<String>> {
        let body = tx.body_ref();
        let mut map = serde_json::Map::new();
        if let Some(value) = tx.auxiliary_data_ref() {
            Self::insert_json_field(
                &mut map,
                "auxiliary_data",
                value
                    .to_json()
                    .map_err(|e| CardanoError::InvalidTransaction(e.to_string()))?,
            )?;
        }
        if let Some(value) = body.voting_proposals_ref() {
            Self::insert_json_field(
                &mut map,
                "voting_proposals",
                value
                    .to_json()
                    .map_err(|e| CardanoError::InvalidTransaction(e.to_string()))?,
            )?;
        }
        if let Some(value) = body.voting_procedures_ref() {
            Self::insert_json_field(
                &mut map,
                "voting_procedures",
                value
                    .to_json()
                    .map_err(|e| CardanoError::InvalidTransaction(e.to_string()))?,
            )?;
        }
        if let Some(value) = body.certs_ref() {
            Self::insert_json_field(
                &mut map,
                "certificates",
                value
                    .to_json()
                    .map_err(|e| CardanoError::InvalidTransaction(e.to_string()))?,
            )?;
        }
        if let Some(value) = body.auxiliary_data_hash() {
            map.insert(
                "auxiliary_data_hash".to_string(),
                serde_json::Value::String(value.to_string()),
            );
        }
        if let Some(value) = body.update() {
            Self::insert_json_field(
                &mut map,
                "protocol_update",
                value
                    .to_json()
                    .map_err(|e| CardanoError::InvalidTransaction(e.to_string()))?,
            )?;
        }
        if let Some(value) = body.script_data_hash() {
            map.insert(
                "script_data_hash".to_string(),
                serde_json::Value::String(value.to_string()),
            );
        }
        if let Some(value) = body.collateral() {
            Self::insert_json_field(
                &mut map,
                "collateral_inputs",
                value
                    .to_json()
                    .map_err(|e| CardanoError::InvalidTransaction(e.to_string()))?,
            )?;
        }
        if let Some(value) = body.required_signers() {
            Self::insert_json_field(
                &mut map,
                "required_signers",
                value
                    .to_json()
                    .map_err(|e| CardanoError::InvalidTransaction(e.to_string()))?,
            )?;
        }
        if let Some(value) = body.collateral_return() {
            Self::insert_json_field(
                &mut map,
                "collateral_return",
                value
                    .to_json()
                    .map_err(|e| CardanoError::InvalidTransaction(e.to_string()))?,
            )?;
        }
        if let Some(value) = body.reference_inputs() {
            Self::insert_json_field(
                &mut map,
                "reference_inputs",
                value
                    .to_json()
                    .map_err(|e| CardanoError::InvalidTransaction(e.to_string()))?,
            )?;
        }
        if let Some(value) = body.current_treasury_value() {
            map.insert(
                "current_treasury_value".to_string(),
                serde_json::Value::String(value.to_string()),
            );
        }

        let outputs_with_data = body
            .outputs()
            .into_iter()
            .filter(|output| {
                output.has_data_hash() || output.has_plutus_data() || output.has_script_ref()
            })
            .map(|output| output.to_json())
            .collect::<core::result::Result<Vec<_>, _>>()
            .map_err(|e| CardanoError::InvalidTransaction(e.to_string()))?;
        if !outputs_with_data.is_empty() {
            let values = outputs_with_data
                .into_iter()
                .map(|json| {
                    serde_json::from_str::<serde_json::Value>(&json)
                        .map(Self::normalize_json_for_display)
                        .map_err(|e| CardanoError::InvalidTransaction(e.to_string()))
                })
                .collect::<Result<Vec<_>>>()?;
            map.insert("outputs_with_datum_or_script".to_string(), values.into());
        }

        let witness_set = tx.witness_set();
        if witness_set.native_scripts().is_some()
            || witness_set.plutus_scripts().is_some()
            || witness_set.plutus_data().is_some()
            || witness_set.redeemers().is_some()
        {
            Self::insert_json_field(
                &mut map,
                "witness_set",
                witness_set
                    .to_json()
                    .map_err(|e| CardanoError::InvalidTransaction(e.to_string()))?,
            )?;
        }
        Self::finish_json(map)
    }

    fn format_stake_credential(network_id: Option<u8>, credential: &Credential) -> Result<String> {
        if let Some(network_id) = network_id {
            return RewardAddress::new(network_id, credential)
                .to_address()
                .to_bech32(None)
                .map_err(|e| CardanoError::InvalidTransaction(e.to_string()));
        }
        match credential.kind() {
            CredKind::Key => credential
                .to_keyhash()
                .map(|hash| format!("Key hash: {hash}")),
            CredKind::Script => credential
                .to_scripthash()
                .map(|hash| format!("Script hash: {hash}")),
        }
        .ok_or_else(|| CardanoError::InvalidTransaction("invalid stake credential".to_string()))
    }

    fn parse_certificates(
        tx: &Transaction,
        network_id: Option<u8>,
    ) -> Result<Vec<CardanoCertificate>> {
        let mut certs = vec![];
        if let Some(_certs) = tx.body_ref().certs_ref() {
            for cert in _certs {
                if let Some(_cert) = cert.as_stake_delegation_ref() {
                    let fields = vec![
                        CertField {
                            label: LABEL_ADDRESS.to_string(),
                            value: Self::format_stake_credential(
                                network_id,
                                &_cert.stake_credential(),
                            )?,
                        },
                        CertField {
                            label: LABEL_POOL.to_string(),
                            value: _cert
                                .pool_keyhash()
                                .to_bech32("pool")
                                .map_err(|e| CardanoError::InvalidTransaction(e.to_string()))?,
                        },
                    ];
                    certs.push(CardanoCertificate::new(
                        "Stake Pool Delegation".to_string(),
                        fields,
                    ));
                }
                if let Some(_cert) = cert.as_stake_deregistration_ref() {
                    let mut fields = vec![CertField {
                        label: LABEL_ADDRESS.to_string(),
                        value: Self::format_stake_credential(
                            network_id,
                            &_cert.stake_credential(),
                        )?,
                    }];
                    if let Some(v) = _cert.coin() {
                        fields.push(CertField {
                            label: LABEL_DEPOSIT.to_string(),
                            value: normalize_coin(u64::from(&v)),
                        });
                    }
                    certs.push(CardanoCertificate::new(
                        "Stake Deregistration".to_string(),
                        fields,
                    ));
                }
                if let Some(_cert) = cert.as_stake_registration_ref() {
                    let mut fields = vec![CertField {
                        label: LABEL_ADDRESS.to_string(),
                        value: Self::format_stake_credential(
                            network_id,
                            &_cert.stake_credential(),
                        )?,
                    }];
                    if let Some(v) = _cert.coin() {
                        fields.push(CertField {
                            label: LABEL_DEPOSIT.to_string(),
                            value: normalize_coin(u64::from(&v)),
                        });
                    }
                    certs.push(CardanoCertificate::new(
                        "Account Registration".to_string(),
                        fields,
                    ));
                }
                if let Some(_cert) = cert.as_vote_delegation_ref() {
                    let (variant2, variant2_label) = match _cert.drep().kind() {
                        DRepKind::AlwaysAbstain => ("Abstain".to_string(), LABEL_VOTE.to_string()),
                        DRepKind::AlwaysNoConfidence => {
                            ("No Confidence".to_string(), LABEL_VOTE.to_string())
                        }
                        DRepKind::KeyHash => (
                            _cert
                                .drep()
                                .to_key_hash()
                                .ok_or_else(|| {
                                    CardanoError::InvalidTransaction(
                                        "Invalid DRep key hash".to_string(),
                                    )
                                })?
                                .to_bech32("drep")
                                .map_err(|e| CardanoError::InvalidTransaction(e.to_string()))?,
                            LABEL_DREP.to_string(),
                        ),
                        DRepKind::ScriptHash => (
                            _cert
                                .drep()
                                .to_script_hash()
                                .ok_or_else(|| {
                                    CardanoError::InvalidTransaction(
                                        "Invalid DRep script hash".to_string(),
                                    )
                                })?
                                .to_bech32("")
                                .map_err(|e| CardanoError::InvalidTransaction(e.to_string()))?,
                            LABEL_DREP.to_string(),
                        ),
                    };
                    let fields = vec![
                        CertField {
                            label: LABEL_ADDRESS.to_string(),
                            value: Self::format_stake_credential(
                                network_id,
                                &_cert.stake_credential(),
                            )?,
                        },
                        CertField {
                            label: variant2_label,
                            value: variant2,
                        },
                    ];
                    certs.push(CardanoCertificate::new(
                        "DRep Delegation".to_string(),
                        fields,
                    ));
                }
                if let Some(_cert) = cert.as_pool_registration_ref() {
                    let fields = vec![
                        CertField {
                            label: LABEL_POOL.to_string(),
                            value: _cert
                                .pool_params_ref()
                                .operator()
                                .to_bech32("pool")
                                .map_err(|e| CardanoError::InvalidTransaction(e.to_string()))?,
                        },
                        CertField {
                            label: "Reward Address".to_string(),
                            value: _cert
                                .pool_params_ref()
                                .reward_account_ref()
                                .to_address()
                                .to_bech32(None)
                                .map_err(|e| CardanoError::InvalidTransaction(e.to_string()))?,
                        },
                    ];
                    certs.push(CardanoCertificate::new(
                        "Pool Registration".to_string(),
                        fields,
                    ));
                }
                if let Some(_cert) = cert.as_pool_retirement_ref() {
                    let fields = vec![
                        CertField {
                            label: LABEL_POOL.to_string(),
                            value: _cert
                                .pool_keyhash()
                                .to_bech32("pool")
                                .map_err(|e| CardanoError::InvalidTransaction(e.to_string()))?,
                        },
                        CertField {
                            label: "Epoch".to_string(),
                            value: _cert.epoch().to_string(),
                        },
                    ];
                    certs.push(CardanoCertificate::new(
                        "Pool Retirement".to_string(),
                        fields,
                    ));
                }
                if let Some(_cert) = cert.as_genesis_key_delegation_ref() {
                    let fields = vec![
                        CertField {
                            label: "Genesis Hash".to_string(),
                            value: _cert.genesishash().to_string(),
                        },
                        CertField {
                            label: "Delegate Hash".to_string(),
                            value: _cert.genesis_delegate_hash().to_string(),
                        },
                        CertField {
                            label: "VRF Key Hash".to_string(),
                            value: _cert.vrf_keyhash().to_string(),
                        },
                    ];
                    certs.push(CardanoCertificate::new(
                        "Genesis Key Delegation".to_string(),
                        fields,
                    ));
                }
                if let Some(_cert) = cert.as_move_instantaneous_rewards_cert_ref() {
                    let reward = _cert.move_instantaneous_reward();
                    let pot = match reward.pot() {
                        MIRPot::Reserves => "Reserves",
                        MIRPot::Treasury => "Treasury",
                    };
                    let action = match reward.kind() {
                        MIRKind::ToOtherPot => "Transfer to other pot".to_string(),
                        MIRKind::ToStakeCredentials => format!(
                            "Reward {} stake credential(s)",
                            reward
                                .as_to_stake_creds()
                                .map(|values| values.len())
                                .unwrap_or(0)
                        ),
                    };
                    let fields = vec![
                        CertField {
                            label: "Source Pot".to_string(),
                            value: pot.to_string(),
                        },
                        CertField {
                            label: "Action".to_string(),
                            value: action,
                        },
                    ];
                    certs.push(CardanoCertificate::new(
                        "Move Instantaneous Rewards Cert".to_string(),
                        fields,
                    ));
                }
                if let Some(_cert) = cert.as_committee_hot_auth_ref() {
                    let fields = vec![
                        CertField {
                            label: LABEL_HOT_KEY.to_string(),
                            value: match _cert.committee_hot_credential().kind() {
                                CredKind::Key => _cert
                                    .committee_hot_credential()
                                    .to_keyhash()
                                    .ok_or_else(|| {
                                        CardanoError::InvalidTransaction(
                                            "Invalid committee hot key hash".to_string(),
                                        )
                                    })?
                                    .to_string(),
                                CredKind::Script => _cert
                                    .committee_hot_credential()
                                    .to_scripthash()
                                    .ok_or_else(|| {
                                        CardanoError::InvalidTransaction(
                                            "Invalid committee hot script hash".to_string(),
                                        )
                                    })?
                                    .to_string(),
                            },
                        },
                        CertField {
                            label: LABEL_COLD_KEY.to_string(),
                            value: match _cert.committee_cold_credential().kind() {
                                CredKind::Key => _cert
                                    .committee_cold_credential()
                                    .to_keyhash()
                                    .ok_or_else(|| {
                                        CardanoError::InvalidTransaction(
                                            "Invalid committee cold key hash".to_string(),
                                        )
                                    })?
                                    .to_string(),
                                CredKind::Script => _cert
                                    .committee_cold_credential()
                                    .to_scripthash()
                                    .ok_or_else(|| {
                                        CardanoError::InvalidTransaction(
                                            "Invalid committee cold script hash".to_string(),
                                        )
                                    })?
                                    .to_string(),
                            },
                        },
                    ];
                    certs.push(CardanoCertificate::new(
                        "Committee Hot Auth".to_string(),
                        fields,
                    ));
                }
                if let Some(_cert) = cert.as_committee_cold_resign_ref() {
                    let mut fields = vec![CertField {
                        label: LABEL_COLD_KEY.to_string(),
                        value: match _cert.committee_cold_credential().kind() {
                            CredKind::Key => _cert
                                .committee_cold_credential()
                                .to_keyhash()
                                .ok_or_else(|| {
                                    CardanoError::InvalidTransaction(
                                        "Invalid committee cold key hash".to_string(),
                                    )
                                })?
                                .to_string(),
                            CredKind::Script => _cert
                                .committee_cold_credential()
                                .to_scripthash()
                                .ok_or_else(|| {
                                    CardanoError::InvalidTransaction(
                                        "Invalid committee cold script hash".to_string(),
                                    )
                                })?
                                .to_string(),
                        },
                    }];
                    if let Some(anchor) = _cert.anchor_ref() {
                        fields.push(CertField {
                            label: LABEL_ANCHOR_URL.to_string(),
                            value: anchor.url_ref().url(),
                        });
                        fields.push(CertField {
                            label: LABEL_ANCHOR_DATA_HASH.to_string(),
                            value: anchor.anchor_data_hash_ref().to_string(),
                        });
                    }
                    certs.push(CardanoCertificate::new(
                        "Committee Cold Resign".to_string(),
                        fields,
                    ));
                }
                if let Some(_cert) = cert.as_drep_deregistration_ref() {
                    let deposit = normalize_coin(u64::from(&_cert.coin()));
                    let (variant1, variant1_label) = match _cert.voting_credential().kind() {
                        CredKind::Key => (
                            _cert
                                .voting_credential()
                                .to_keyhash()
                                .unwrap()
                                .to_bech32("drep")
                                .map_err(|e| CardanoError::InvalidTransaction(e.to_string()))?,
                            LABEL_DREP.to_string(),
                        ),
                        CredKind::Script => (
                            _cert
                                .voting_credential()
                                .to_scripthash()
                                .unwrap()
                                .to_bech32("")
                                .map_err(|e| CardanoError::InvalidTransaction(e.to_string()))?,
                            LABEL_DREP.to_string(),
                        ),
                    };
                    let fields = vec![
                        CertField {
                            label: variant1_label,
                            value: variant1,
                        },
                        CertField {
                            label: LABEL_DEPOSIT.to_string(),
                            value: deposit,
                        },
                    ];
                    certs.push(CardanoCertificate::new(
                        "Drep Deregistration".to_string(),
                        fields,
                    ));
                }
                if let Some(_cert) = cert.as_drep_registration_ref() {
                    let deposit = normalize_coin(u64::from(&_cert.coin()));
                    let (variant1, variant1_label) = match _cert.voting_credential().kind() {
                        CredKind::Key => (
                            _cert
                                .voting_credential()
                                .to_keyhash()
                                .unwrap()
                                .to_bech32("drep")
                                .map_err(|e| CardanoError::InvalidTransaction(e.to_string()))?,
                            LABEL_DREP.to_string(),
                        ),
                        CredKind::Script => (
                            _cert
                                .voting_credential()
                                .to_scripthash()
                                .unwrap()
                                .to_bech32("")
                                .map_err(|e| CardanoError::InvalidTransaction(e.to_string()))?,
                            LABEL_DREP.to_string(),
                        ),
                    };
                    let fields = vec![
                        CertField {
                            label: variant1_label,
                            value: variant1,
                        },
                        CertField {
                            label: LABEL_DEPOSIT.to_string(),
                            value: deposit,
                        },
                        CertField {
                            label: LABEL_ANCHOR_URL.to_string(),
                            value: _cert
                                .anchor_ref()
                                .map(|v| v.url_ref().url())
                                .unwrap_or("None".to_string()),
                        },
                        CertField {
                            label: LABEL_ANCHOR_DATA_HASH.to_string(),
                            value: _cert
                                .anchor_ref()
                                .map(|v| v.anchor_data_hash_ref().to_string())
                                .unwrap_or("None".to_string()),
                        },
                    ];
                    certs.push(CardanoCertificate::new(
                        "Drep Registration".to_string(),
                        fields,
                    ));
                }
                if let Some(_cert) = cert.as_drep_update_ref() {
                    let anchor_data_hash = _cert
                        .anchor_ref()
                        .map(|anchor| anchor.anchor_data_hash_ref().to_string());
                    let (variant1, variant1_label) = match _cert.voting_credential().kind() {
                        CredKind::Key => (
                            _cert
                                .voting_credential()
                                .to_keyhash()
                                .unwrap()
                                .to_bech32("drep")
                                .map_err(|e| CardanoError::InvalidTransaction(e.to_string()))?,
                            LABEL_DREP.to_string(),
                        ),
                        CredKind::Script => (
                            _cert
                                .voting_credential()
                                .to_scripthash()
                                .unwrap()
                                .to_bech32("")
                                .map_err(|e| CardanoError::InvalidTransaction(e.to_string()))?,
                            LABEL_DREP.to_string(),
                        ),
                    };
                    let fields = vec![
                        CertField {
                            label: variant1_label,
                            value: variant1,
                        },
                        CertField {
                            label: LABEL_ANCHOR_URL.to_string(),
                            value: _cert
                                .anchor_ref()
                                .map(|v| v.url_ref().url())
                                .unwrap_or("None".to_string()),
                        },
                        CertField {
                            label: LABEL_ANCHOR_DATA_HASH.to_string(),
                            value: anchor_data_hash.unwrap_or("None".to_string()),
                        },
                    ];
                    certs.push(CardanoCertificate::new("Drep Update".to_string(), fields));
                }
                if let Some(_cert) = cert.as_stake_and_vote_delegation_ref() {
                    let (variant3, variant3_label) = match _cert.drep().kind() {
                        DRepKind::AlwaysAbstain => ("Abstain".to_string(), LABEL_VOTE.to_string()),
                        DRepKind::AlwaysNoConfidence => {
                            ("No Confidence".to_string(), LABEL_VOTE.to_string())
                        }
                        DRepKind::KeyHash => (
                            _cert
                                .drep()
                                .to_key_hash()
                                .unwrap()
                                .to_bech32("drep")
                                .map_err(|e| CardanoError::InvalidTransaction(e.to_string()))?,
                            LABEL_DREP.to_string(),
                        ),
                        DRepKind::ScriptHash => (
                            _cert
                                .drep()
                                .to_script_hash()
                                .unwrap()
                                .to_bech32("")
                                .map_err(|e| CardanoError::InvalidTransaction(e.to_string()))?,
                            LABEL_DREP.to_string(),
                        ),
                    };
                    let fields = vec![
                        CertField {
                            label: LABEL_ADDRESS.to_string(),
                            value: Self::format_stake_credential(
                                network_id,
                                &_cert.stake_credential(),
                            )?,
                        },
                        CertField {
                            label: LABEL_POOL.to_string(),
                            value: _cert
                                .pool_keyhash()
                                .to_bech32("pool")
                                .map_err(|e| CardanoError::InvalidTransaction(e.to_string()))?,
                        },
                        CertField {
                            label: variant3_label,
                            value: variant3,
                        },
                    ];
                    certs.push(CardanoCertificate::new(
                        "Stake And Vote Delegation".to_string(),
                        fields,
                    ));
                }
                if let Some(_cert) = cert.as_stake_registration_and_delegation_ref() {
                    let deposit = normalize_coin(u64::from(&_cert.coin()));
                    let fields = vec![
                        CertField {
                            label: LABEL_ADDRESS.to_string(),
                            value: Self::format_stake_credential(
                                network_id,
                                &_cert.stake_credential(),
                            )?,
                        },
                        CertField {
                            label: LABEL_POOL.to_string(),
                            value: _cert
                                .pool_keyhash()
                                .to_bech32("pool")
                                .map_err(|e| CardanoError::InvalidTransaction(e.to_string()))?,
                        },
                        CertField {
                            label: LABEL_DEPOSIT.to_string(),
                            value: deposit,
                        },
                    ];
                    certs.push(CardanoCertificate::new(
                        "Stake Registration & Delegation".to_string(),
                        fields,
                    ));
                }
                if let Some(_cert) = cert.as_stake_vote_registration_and_delegation_ref() {
                    let (variant3, variant3_label) = match _cert.drep().kind() {
                        DRepKind::AlwaysAbstain => ("Abstain".to_string(), LABEL_VOTE.to_string()),
                        DRepKind::AlwaysNoConfidence => {
                            ("No Confidence".to_string(), LABEL_VOTE.to_string())
                        }
                        DRepKind::KeyHash => (
                            _cert
                                .drep()
                                .to_key_hash()
                                .unwrap()
                                .to_bech32("drep")
                                .map_err(|e| CardanoError::InvalidTransaction(e.to_string()))?,
                            LABEL_DREP.to_string(),
                        ),
                        DRepKind::ScriptHash => (
                            _cert
                                .drep()
                                .to_script_hash()
                                .unwrap()
                                .to_bech32("")
                                .map_err(|e| CardanoError::InvalidTransaction(e.to_string()))?,
                            LABEL_DREP.to_string(),
                        ),
                    };
                    let deposit = normalize_coin(u64::from(&_cert.coin()));
                    let fields = vec![
                        CertField {
                            label: LABEL_ADDRESS.to_string(),
                            value: Self::format_stake_credential(
                                network_id,
                                &_cert.stake_credential(),
                            )?,
                        },
                        CertField {
                            label: LABEL_POOL.to_string(),
                            value: _cert
                                .pool_keyhash()
                                .to_bech32("pool")
                                .map_err(|e| CardanoError::InvalidTransaction(e.to_string()))?,
                        },
                        CertField {
                            label: variant3_label,
                            value: variant3,
                        },
                        CertField {
                            label: LABEL_DEPOSIT.to_string(),
                            value: deposit,
                        },
                    ];
                    certs.push(CardanoCertificate::new(
                        "Stake Vote Registration & Delegation".to_string(),
                        fields,
                    ));
                }
                if let Some(_cert) = cert.as_vote_registration_and_delegation_ref() {
                    let (variant2, variant2_label) = match _cert.drep().kind() {
                        DRepKind::AlwaysAbstain => ("Abstain".to_string(), LABEL_VOTE.to_string()),
                        DRepKind::AlwaysNoConfidence => {
                            ("No Confidence".to_string(), LABEL_VOTE.to_string())
                        }
                        DRepKind::KeyHash => (
                            _cert
                                .drep()
                                .to_key_hash()
                                .unwrap()
                                .to_bech32("drep")
                                .map_err(|e| CardanoError::InvalidTransaction(e.to_string()))?,
                            LABEL_DREP.to_string(),
                        ),
                        DRepKind::ScriptHash => (
                            _cert
                                .drep()
                                .to_script_hash()
                                .unwrap()
                                .to_bech32("")
                                .map_err(|e| CardanoError::InvalidTransaction(e.to_string()))?,
                            LABEL_DREP.to_string(),
                        ),
                    };
                    let deposit = normalize_coin(u64::from(&_cert.coin()));
                    let fields = vec![
                        CertField {
                            label: LABEL_ADDRESS.to_string(),
                            value: Self::format_stake_credential(
                                network_id,
                                &_cert.stake_credential(),
                            )?,
                        },
                        CertField {
                            label: variant2_label,
                            value: variant2,
                        },
                        CertField {
                            label: LABEL_DEPOSIT.to_string(),
                            value: deposit,
                        },
                    ];
                    certs.push(CardanoCertificate::new(
                        "Vote Registration And Delegation".to_string(),
                        fields,
                    ));
                }
            }
        }
        Ok(certs)
    }

    fn parse_withdrawals(tx: &Transaction) -> Result<Vec<CardanoWithdrawal>> {
        let mut withdrawals = vec![];
        if let Some(_withdrawals) = tx.body_ref().withdrawals() {
            for (address, value) in _withdrawals.iter() {
                withdrawals.push(CardanoWithdrawal::new(
                    address
                        .to_address()
                        .to_bech32(None)
                        .map_err(|e| CardanoError::InvalidTransaction(e.to_string()))?,
                    normalize_coin(u64::from(value)),
                ))
            }
        }
        Ok(withdrawals)
    }

    fn get_from_list(inputs: Vec<ParsedCardanoInput>) -> Vec<CardanoFrom> {
        inputs
            .into_iter()
            .map(|input| {
                let known = input.value.is_some() && input.address.is_some();
                let value = input.value.unwrap_or(0);
                CardanoFrom {
                    address: input
                        .address
                        .unwrap_or_else(|| "Details unavailable".to_string()),
                    amount: input
                        .value
                        .map(normalize_coin)
                        .unwrap_or_else(|| "Details unavailable".to_string()),
                    path: input.path,
                    transaction_id: input.transaction_id,
                    index: input.index,
                    known,
                    value,
                }
            })
            .collect()
    }

    fn get_to_list(outputs: Vec<ParsedCardanoOutput>) -> Vec<CardanoTo> {
        outputs
            .into_iter()
            .map(|output| {
                let assets = output
                    .assets
                    .unwrap_or_default()
                    .into_iter()
                    .map(|asset| (asset.id.clone(), asset))
                    .collect::<BTreeMap<_, _>>();
                CardanoTo {
                    address: output.address,
                    amount: normalize_coin(output.value),
                    assets_text: match assets.len() {
                        0 => None,
                        count => Some(format!("{count} more assets")),
                    },
                    assets,
                    value: output.value,
                }
            })
            .collect()
    }

    pub fn verify(tx: Transaction, context: ParseContext) -> Result<()> {
        let network_id = Self::judge_network_id(&tx, &context)?;
        let parsed_inputs = Self::parse_inputs(&tx, &context, network_id)?;

        let mfp = hex::encode(context.get_master_fingerprint());
        let has_my_signer = context
            .get_cert_keys()
            .iter()
            .filter(|v| hex::encode(v.get_master_fingerprint()).eq(&mfp))
            .any(|cur| hex::encode(cur.get_master_fingerprint()).eq(&mfp));

        if has_my_signer {
            return Ok(());
        }

        if parsed_inputs
            .iter()
            .filter(|v| v.address.is_some())
            .collect::<Vec<&ParsedCardanoInput>>()
            .is_empty()
        {
            return Err(CardanoError::InvalidTransaction(
                "no input related to this account".to_string(),
            ));
        }
        Self::parse_certificates(&tx, network_id)?;
        Ok(())
    }

    fn parse_inputs(
        tx: &Transaction,
        context: &ParseContext,
        _network_id: Option<u8>,
    ) -> Result<Vec<ParsedCardanoInput>> {
        let mut parsed_inputs: Vec<ParsedCardanoInput> = vec![];
        for input in tx.body_ref().inputs() {
            let hash = input.transaction_id_ref().to_hex();
            let input_index = input.index();
            let m = context.utxos.iter().find(|v| {
                hash.eq_ignore_ascii_case(&hex::encode(&v.transaction_hash))
                    && input_index.eq(&v.index)
            });
            match m {
                //known utxo
                Some(utxo) => {
                    let mut iter = utxo.path.into_iter();
                    let _root = match iter.next() {
                        Some(Hardened { index: 1852 }) => Ok(1852u32),
                        _ => Err(CardanoError::DerivationError(
                            "invalid derivation path".to_string(),
                        )),
                    }?;
                    let _coin_type = match iter.next() {
                        Some(Hardened { index: 1815 }) => Ok(1815u32),
                        _ => Err(CardanoError::DerivationError(
                            "invalid derivation path".to_string(),
                        )),
                    }?;
                    let _account = match iter.next() {
                        Some(Hardened { index: _i }) => Ok(_i),
                        _ => Err(CardanoError::DerivationError(
                            "invalid derivation path".to_string(),
                        )),
                    }?;
                    let change = match iter.next() {
                        Some(Normal { index: _i }) => Ok(_i),
                        _ => Err(CardanoError::DerivationError(
                            "invalid derivation path".to_string(),
                        )),
                    }?;
                    let index = match iter.next() {
                        Some(Normal { index: _i }) => Ok(_i),
                        _ => Err(CardanoError::DerivationError(
                            "invalid derivation path".to_string(),
                        )),
                    }?;

                    let address = utxo.address.clone();

                    let addr_in_utxo = Address::from_bech32(&utxo.address)
                        .map_err(|e| CardanoError::InvalidTransaction(e.to_string()))?;

                    let xpub = context.get_cardano_xpub();
                    if xpub.is_none() {
                        parsed_inputs.push(ParsedCardanoInput {
                            transaction_id: hash.clone(),
                            index: input_index,
                            value: Some(utxo.value),
                            address: Some(address),
                            path: Some(utxo.path.to_string()),
                        });
                        continue;
                    }

                    let mut pubkey_hash_paired = false;

                    //check utxo address with payment keyhash;
                    let my_pubkey_hash = hex::encode(derive_pubkey_hash(
                        context.get_cardano_xpub().unwrap(),
                        *change,
                        *index,
                    )?);

                    if let Some(addr) = BaseAddress::from_address(&addr_in_utxo) {
                        if let Some(keyhash) = addr.payment_cred().to_keyhash() {
                            if my_pubkey_hash.eq(&keyhash.to_hex()) {
                                pubkey_hash_paired = true;
                            }
                        }
                    }

                    if let Some(addr) = EnterpriseAddress::from_address(&addr_in_utxo) {
                        if let Some(keyhash) = addr.payment_cred().to_keyhash() {
                            if my_pubkey_hash.eq(&keyhash.to_hex()) {
                                pubkey_hash_paired = true;
                            }
                        }
                    }

                    if !pubkey_hash_paired {
                        return Err(CardanoError::InvalidTransaction(
                            "invalid address".to_string(),
                        ));
                    }

                    parsed_inputs.push(ParsedCardanoInput {
                        transaction_id: hash,
                        index: input_index,
                        value: Some(utxo.value),
                        address: Some(address),
                        path: Some(utxo.path.to_string()),
                    })
                }
                None => parsed_inputs.push(ParsedCardanoInput {
                    transaction_id: hash,
                    index: input_index,
                    value: None,
                    address: None,
                    path: None,
                }),
            }
        }
        Ok(parsed_inputs)
    }

    fn parse_outputs(tx: &Transaction) -> Result<Vec<ParsedCardanoOutput>> {
        let mut parsed_outputs = vec![];
        for output in tx.body_ref().outputs() {
            let value = output.amount_ref();
            let parsed_output = ParsedCardanoOutput {
                address: output
                    .address_ref()
                    .to_bech32(None)
                    .map_err(|e| CardanoError::AddressEncodingError(e.to_string()))?,
                value: u64::from(&value.coin()),
                assets: value.multiasset_ref().map(|multiasset| {
                    multiasset
                        .iter()
                        .flat_map(|(policy_id, assets)| {
                            assets.iter().map(move |(name, quantity)| {
                                let policy_id = policy_id.to_bytes();
                                let name = name.to_bytes();
                                let value = u64::from(quantity);
                                ParsedCardanoMultiAsset {
                                    id: format!(
                                        "{}#{}",
                                        hex::encode(&policy_id),
                                        hex::encode(&name)
                                    ),
                                    policy_id,
                                    name,
                                    amount: value.to_string(),
                                    value,
                                }
                            })
                        })
                        .collect()
                }),
            };
            parsed_outputs.push(parsed_output);
        }
        Ok(parsed_outputs)
    }

    fn parse_mint_assets(tx: &Transaction) -> Vec<ParsedCardanoNativeAsset> {
        tx.body_ref()
            .mint_ref()
            .into_iter()
            .flat_map(|mint| mint.iter())
            .flat_map(|(policy_id, assets)| {
                assets
                    .iter()
                    .map(move |(name, quantity)| ParsedCardanoNativeAsset {
                        policy_id: policy_id.to_bytes(),
                        name: name.to_bytes(),
                        amount: quantity.to_str(),
                    })
            })
            .collect()
    }

    fn parse_collateral_assets(tx: &Transaction) -> Vec<ParsedCardanoNativeAsset> {
        tx.body_ref()
            .collateral_return_ref()
            .and_then(|output| output.amount_ref().multiasset_ref())
            .into_iter()
            .flat_map(|multiasset| multiasset.iter())
            .flat_map(|(policy_id, assets)| {
                assets
                    .iter()
                    .map(move |(name, quantity)| ParsedCardanoNativeAsset {
                        policy_id: policy_id.to_bytes(),
                        name: name.to_bytes(),
                        amount: quantity.to_string(),
                    })
            })
            .collect()
    }
}

impl CardanoFrom {
    pub fn into_display_parts(self) -> (String, String, Option<String>, String, u32, bool) {
        (
            self.address,
            self.amount,
            self.path,
            self.transaction_id,
            self.index,
            self.known,
        )
    }
}

impl CardanoTo {
    pub fn into_display_parts(
        self,
    ) -> (String, String, Option<String>, Vec<ParsedCardanoMultiAsset>) {
        (
            self.address,
            self.amount,
            self.assets_text,
            self.assets.into_values().collect(),
        )
    }

    pub fn address_ref(&self) -> &str {
        &self.address
    }

    pub fn amount_ref(&self) -> &str {
        &self.amount
    }

    pub fn assets_ref(&self) -> impl Iterator<Item = &ParsedCardanoMultiAsset> {
        self.assets.values()
    }
}

impl ParsedCardanoMultiAsset {
    pub fn into_display_parts(self) -> (Vec<u8>, Vec<u8>, String) {
        (self.policy_id, self.name, self.amount)
    }

    pub fn policy_id_ref(&self) -> &[u8] {
        &self.policy_id
    }

    pub fn name_ref(&self) -> &[u8] {
        &self.name
    }

    pub fn amount_ref(&self) -> &str {
        &self.amount
    }
}

impl ParsedCardanoNativeAsset {
    pub fn into_display_parts(self) -> (Vec<u8>, Vec<u8>, String) {
        (self.policy_id, self.name, self.amount)
    }

    pub fn policy_id_ref(&self) -> &[u8] {
        &self.policy_id
    }

    pub fn name_ref(&self) -> &[u8] {
        &self.name
    }

    pub fn amount_ref(&self) -> &str {
        &self.amount
    }
}

impl VotingProcedure {
    pub fn into_display_parts(self) -> (String, String, String, String, String) {
        (
            self.voter_type,
            self.voter,
            self.transaction_id,
            self.index,
            self.vote,
        )
    }
}

impl VotingProposal {
    pub fn into_display_parts(self) -> (String, String, String, String, String) {
        (
            self.action,
            self.deposit,
            self.reward_account,
            self.anchor_url,
            self.anchor_data_hash,
        )
    }
}

impl CardanoCertificate {
    pub fn into_display_parts(self) -> (String, Vec<CertField>) {
        (self.cert_type, self.fields)
    }
}

impl CertField {
    pub fn into_display_parts(self) -> (String, String) {
        (self.label, self.value)
    }
}

impl CardanoWithdrawal {
    pub fn into_display_parts(self) -> (String, String) {
        (self.address, self.amount)
    }
}

const DIVIDER: f64 = 1_000_000f64;

fn normalize_coin(value: u64) -> String {
    format!("{} ADA", (value as f64).div(DIVIDER))
}

#[cfg(test)]
fn normalize_value(value: u64) -> String {
    format!("{}", (value as f64).div(DIVIDER))
}

fn contains_multi_assets(outputs: &[ParsedCardanoOutput]) -> bool {
    outputs.iter().any(|output| output.assets.is_some())
}

#[cfg(test)]
mod tests {
    use super::*;
    use bitcoin::bip32::DerivationPath;
    use cardano_serialization_lib::{
        Certificate, Certificates, Ed25519KeyHash, Ed25519KeyHashes, PoolParams, PoolRegistration,
        Relays, TransactionBody, TransactionInputs, TransactionOutput, TransactionOutputs,
        TransactionWitnessSet, UnitInterval, VRFKeyHash, Value,
    };
    use core::str::FromStr;
    use ur_registry::cardano::cardano_sign_request::CardanoSignRequest;

    #[test]
    fn test_normalize_coin() {
        let value = 1_000_000u64;
        let result = normalize_coin(value);
        assert_eq!(result, "1 ADA");
    }

    #[test]
    fn test_normalize_value() {
        let value = 1_000_000u64;
        let result = normalize_value(value);
        assert_eq!(result, "1");
    }

    #[test]
    fn test_normalize_coin_zero() {
        let value = 0u64;
        let result = normalize_coin(value);
        assert_eq!(result, "0 ADA");
    }

    #[test]
    fn test_normalize_coin_fractional() {
        let value = 500_000u64; // 0.5 ADA
        let result = normalize_coin(value);
        assert_eq!(result, "0.5 ADA");
    }

    #[test]
    fn test_normalize_coin_large() {
        let value = 1_000_000_000_000u64; // 1,000,000 ADA
        let result = normalize_coin(value);
        assert!(result.contains("ADA"));
    }

    #[test]
    fn test_normalize_value_zero() {
        let value = 0u64;
        let result = normalize_value(value);
        assert_eq!(result, "0");
    }

    #[test]
    fn test_contains_multi_assets() {
        let output = |assets| ParsedCardanoOutput {
            address: "addr1...".to_string(),
            value: 1_000_000,
            assets,
        };

        assert!(!contains_multi_assets(&[output(None)]));
        assert!(contains_multi_assets(&[output(Some(vec![]))]));
    }

    #[test]
    fn test_normalize_value_fractional() {
        let value = 500_000u64; // 0.5
        let result = normalize_value(value);
        assert_eq!(result, "0.5");
    }

    #[test]
    fn test_parse_sign_data() {
        let payload = "846a5369676e6174757265315882a301270458390069fa1bd9338574702283d8fb71f8cce1831c3ea4854563f5e4043aea33a4f1f468454744b2ff3644b2ab79d48e76a3187f902fe8a1bcfaad676164647265737358390069fa1bd9338574702283d8fb71f8cce1831c3ea4854563f5e4043aea33a4f1f468454744b2ff3644b2ab79d48e76a3187f902fe8a1bcfaad4043abc123";
        let xpub = "ca0e65d9bb8d0dca5e88adc5e1c644cc7d62e5a139350330281ed7e3a6938d2c";
        let data = ParsedCardanoSignData::build(
            hex::decode(payload).unwrap(),
            "m/1852'/1815'/0'/0/0".to_string(),
            xpub.to_string(),
        )
        .unwrap();
        assert_eq!(data.get_derivation_path(), "m/1852'/1815'/0'/0/0");
        assert_eq!(hex::encode(data.get_payload()), "616263313233");
    }

    #[test]
    fn test_parse_sign_data_invalid_cbor_fallback() {
        // Invalid CBOR should fallback to hex encoding
        let invalid_cbor = vec![0xff, 0xff, 0xff]; // Invalid CBOR
        let xpub = "ca0e65d9bb8d0dca5e88adc5e1c644cc7d62e5a139350330281ed7e3a6938d2c";
        let data = ParsedCardanoSignData::build(
            invalid_cbor.clone(),
            "m/1852'/1815'/0'/0/0".to_string(),
            xpub.to_string(),
        )
        .unwrap();
        assert_eq!(data.get_derivation_path(), "m/1852'/1815'/0'/0/0");
        assert_eq!(data.get_message_hash(), hex::encode(&invalid_cbor));
    }

    #[test]
    fn test_parse_sign_cip8_data_without_hash() {
        let payload = "846a5369676e6174757265315882a301270458390069fa1bd9338574702283d8fb71f8cce1831c3ea4854563f5e4043aea33a4f1f468454744b2ff3644b2ab79d48e76a3187f902fe8a1bcfaad676164647265737358390069fa1bd9338574702283d8fb71f8cce1831c3ea4854563f5e4043aea33a4f1f468454744b2ff3644b2ab79d48e76a3187f902fe8a1bcfaad4043abc123";
        let xpub = "ca0e65d9bb8d0dca5e88adc5e1c644cc7d62e5a139350330281ed7e3a6938d2c";
        let data = ParsedCardanoSignCip8Data::build(
            hex::decode(payload).unwrap(),
            "m/1852'/1815'/0'/0/0".to_string(),
            xpub.to_string(),
            false, // hash_payload = false
        )
        .unwrap();
        assert_eq!(data.get_derivation_path(), "m/1852'/1815'/0'/0/0");
        assert_eq!(data.get_hash_payload(), false);
        assert!(!data.get_message_hash().is_empty());
    }

    #[test]
    fn test_parse_sign_cip8_data_with_hash() {
        let payload = "846a5369676e6174757265315882a301270458390069fa1bd9338574702283d8fb71f8cce1831c3ea4854563f5e4043aea33a4f1f468454744b2ff3644b2ab79d48e76a3187f902fe8a1bcfaad676164647265737358390069fa1bd9338574702283d8fb71f8cce1831c3ea4854563f5e4043aea33a4f1f468454744b2ff3644b2ab79d48e76a3187f902fe8a1bcfaad4043abc123";
        let xpub = "ca0e65d9bb8d0dca5e88adc5e1c644cc7d62e5a139350330281ed7e3a6938d2c";
        let data = ParsedCardanoSignCip8Data::build(
            hex::decode(payload).unwrap(),
            "m/1852'/1815'/0'/0/0".to_string(),
            xpub.to_string(),
            true, // hash_payload = true
        )
        .unwrap();
        assert_eq!(data.get_derivation_path(), "m/1852'/1815'/0'/0/0");
        assert_eq!(data.get_hash_payload(), true);
        // When hash_payload is true, message_hash should be blake2b_224 hash of payload
        assert!(!data.get_message_hash().is_empty());
        assert_eq!(data.get_message_hash().len(), 56); // blake2b_224 produces 28 bytes = 56 hex chars
    }

    #[test]
    fn test_parse_sign() {
        let sign_data = hex::decode("84a400828258204e3a6e7fdcb0d0efa17bf79c13aed2b4cb9baf37fb1aa2e39553d5bd720c5c99038258204e3a6e7fdcb0d0efa17bf79c13aed2b4cb9baf37fb1aa2e39553d5bd720c5c99040182a200581d6179df4c75f7616d7d1fd39cbc1a6ea6b40a0d7b89fea62fc0909b6c370119c350a200581d61c9b0c9761fd1dc0404abd55efc895026628b5035ac623c614fbad0310119c35002198ecb0300a0f5f6").unwrap();
        let _request = CardanoSignRequest::new(
            Some(
                hex::decode("9b1deb4d3b7d4bad9bdd2b0d7b3dcb6d")
                    .unwrap()
                    .try_into()
                    .unwrap(),
            ),
            sign_data.clone().try_into().unwrap(),
            vec![],
            vec![],
            Some("".to_string()),
        );
        let xpub = hex::encode("ca0e65d9bb8d0dca5e88adc5e1c644cc7d62e5a139350330281ed7e3a6938d2c");
        let master_fingerprint = hex::decode("52744703").unwrap();
        let context = ParseContext::new(vec![], vec![], Some(xpub), master_fingerprint);
        let tx = Transaction::from_hex(&hex::encode(sign_data)).unwrap();

        let network_id = ParsedCardanoTx::judge_network_id(&tx, &context).unwrap();
        assert_eq!(network_id, Some(1));

        let auxiliary_data = ParsedCardanoTx::parse_auxiliary_data(&tx);
        assert_eq!(auxiliary_data.unwrap(), None);

        let certificates = ParsedCardanoTx::parse_certificates(&tx, network_id);
        assert_eq!(certificates.unwrap().len(), 0);

        let withdrawals = ParsedCardanoTx::parse_withdrawals(&tx);
        assert_eq!(withdrawals.unwrap().len(), 0);

        let cardano_tx = ParsedCardanoTx::from_cardano_tx(tx, context);
        assert!(cardano_tx.is_ok());
    }

    #[test]
    fn test_judge_network_id_with_network_id() {
        // Test with explicit network_id in transaction body
        let sign_data = hex::decode("84a400828258204e3a6e7fdcb0d0efa17bf79c13aed2b4cb9baf37fb1aa2e39553d5bd720c5c99038258204e3a6e7fdcb0d0efa17bf79c13aed2b4cb9baf37fb1aa2e39553d5bd720c5c99040182a200581d6179df4c75f7616d7d1fd39cbc1a6ea6b40a0d7b89fea62fc0909b6c370119c350a200581d61c9b0c9761fd1dc0404abd55efc895026628b5035ac623c614fbad0310119c35002198ecb0300a0f5f6").unwrap();
        let tx = Transaction::from_hex(&hex::encode(sign_data)).unwrap();
        let context = ParseContext::new(vec![], vec![], None, vec![]);
        let network_id = ParsedCardanoTx::judge_network_id(&tx, &context).unwrap();
        // Should return network_id based on outputs address
        assert!(network_id == Some(0) || network_id == Some(1));
    }

    #[test]
    fn test_no_output_transaction_uses_unknown_network() {
        let body = TransactionBody::new_tx_body(
            &TransactionInputs::new(),
            &TransactionOutputs::new(),
            &0u64.into(),
        );
        let witness_set = TransactionWitnessSet::new();
        let tx = Transaction::new(&body.to_bytes(), &witness_set.to_bytes(), true).unwrap();
        let context = ParseContext::new(vec![], vec![], None, vec![]);

        assert_eq!(
            ParsedCardanoTx::judge_network_id(&tx, &context).unwrap(),
            None
        );
        let parsed = ParsedCardanoTx::from_cardano_tx(tx, context).unwrap();
        assert_eq!(parsed.get_network(), "Unknown");
    }

    #[test]
    fn test_conflicting_output_networks_are_rejected() {
        let key_hash = Ed25519KeyHash::from_bytes(vec![7u8; 28]).unwrap();
        let credential = Credential::from_keyhash(&key_hash);
        let mainnet = EnterpriseAddress::new(1, &credential).to_address();
        let testnet = EnterpriseAddress::new(0, &credential).to_address();
        let amount = Value::new(&1_000_000u64.into());
        let mut outputs = TransactionOutputs::new();
        outputs.add(&TransactionOutput::new(&mainnet, &amount));
        outputs.add(&TransactionOutput::new(&testnet, &amount));
        let body =
            TransactionBody::new_tx_body(&TransactionInputs::new(), &outputs, &200_000u64.into());
        let witness_set = TransactionWitnessSet::new();
        let tx = Transaction::new(&body.to_bytes(), &witness_set.to_bytes(), true).unwrap();
        let context = ParseContext::new(vec![], vec![], None, vec![]);

        assert!(ParsedCardanoTx::judge_network_id(&tx, &context).is_err());
        assert!(ParsedCardanoTx::from_cardano_tx(tx, context).is_err());
    }

    #[test]
    fn test_pool_registration_reward_account_provides_network() {
        let operator = Ed25519KeyHash::from_bytes(vec![1u8; 28]).unwrap();
        let vrf = VRFKeyHash::from_bytes(vec![2u8; 32]).unwrap();
        let reward_credential = Credential::from_keyhash(&operator);
        let reward_account = RewardAddress::new(0, &reward_credential);
        let margin = UnitInterval::new(&1u64.into(), &2u64.into());
        let pool_params = PoolParams::new(
            &operator,
            &vrf,
            &1_000_000u64.into(),
            &340_000_000u64.into(),
            &margin,
            &reward_account,
            &Ed25519KeyHashes::new(),
            &Relays::new(),
            None,
        );
        let mut certificates = Certificates::new();
        certificates.add(&Certificate::new_pool_registration(&PoolRegistration::new(
            &pool_params,
        )));
        let mut body = TransactionBody::new_tx_body(
            &TransactionInputs::new(),
            &TransactionOutputs::new(),
            &200_000u64.into(),
        );
        body.set_certs(&certificates);
        let witness_set = TransactionWitnessSet::new();
        let tx = Transaction::new(&body.to_bytes(), &witness_set.to_bytes(), true).unwrap();
        let context = ParseContext::new(vec![], vec![], None, vec![]);

        assert_eq!(
            ParsedCardanoTx::judge_network_id(&tx, &context).unwrap(),
            Some(0)
        );
    }

    #[test]
    fn test_parse_auxiliary_data_none() {
        let sign_data = hex::decode("84a400828258204e3a6e7fdcb0d0efa17bf79c13aed2b4cb9baf37fb1aa2e39553d5bd720c5c99038258204e3a6e7fdcb0d0efa17bf79c13aed2b4cb9baf37fb1aa2e39553d5bd720c5c99040182a200581d6179df4c75f7616d7d1fd39cbc1a6ea6b40a0d7b89fea62fc0909b6c370119c350a200581d61c9b0c9761fd1dc0404abd55efc895026628b5035ac623c614fbad0310119c35002198ecb0300a0f5f6").unwrap();
        let tx = Transaction::from_hex(&hex::encode(sign_data)).unwrap();
        let aux_data = ParsedCardanoTx::parse_auxiliary_data(&tx);
        assert!(aux_data.is_ok());
        // This transaction likely doesn't have auxiliary data
        assert_eq!(aux_data.unwrap(), None);
    }

    #[test]
    fn test_parse_certificates_empty() {
        let sign_data = hex::decode("84a400828258204e3a6e7fdcb0d0efa17bf79c13aed2b4cb9baf37fb1aa2e39553d5bd720c5c99038258204e3a6e7fdcb0d0efa17bf79c13aed2b4cb9baf37fb1aa2e39553d5bd720c5c99040182a200581d6179df4c75f7616d7d1fd39cbc1a6ea6b40a0d7b89fea62fc0909b6c370119c350a200581d61c9b0c9761fd1dc0404abd55efc895026628b5035ac623c614fbad0310119c35002198ecb0300a0f5f6").unwrap();
        let tx = Transaction::from_hex(&hex::encode(sign_data)).unwrap();
        let context = ParseContext::new(vec![], vec![], None, vec![]);
        let network_id = ParsedCardanoTx::judge_network_id(&tx, &context).unwrap();
        let certs = ParsedCardanoTx::parse_certificates(&tx, network_id);
        assert!(certs.is_ok());
        assert_eq!(certs.unwrap().len(), 0);
    }

    #[test]
    fn test_parse_withdrawals_empty() {
        let sign_data = hex::decode("84a400828258204e3a6e7fdcb0d0efa17bf79c13aed2b4cb9baf37fb1aa2e39553d5bd720c5c99038258204e3a6e7fdcb0d0efa17bf79c13aed2b4cb9baf37fb1aa2e39553d5bd720c5c99040182a200581d6179df4c75f7616d7d1fd39cbc1a6ea6b40a0d7b89fea62fc0909b6c370119c350a200581d61c9b0c9761fd1dc0404abd55efc895026628b5035ac623c614fbad0310119c35002198ecb0300a0f5f6").unwrap();
        let tx = Transaction::from_hex(&hex::encode(sign_data)).unwrap();
        let withdrawals = ParsedCardanoTx::parse_withdrawals(&tx);
        assert!(withdrawals.is_ok());
        assert_eq!(withdrawals.unwrap().len(), 0);
    }

    #[test]
    fn test_parse_context_new() {
        let utxos = vec![];
        let cert_keys = vec![];
        let xpub = Some("test_xpub".to_string());
        let master_fingerprint = vec![0x52, 0x74, 0x47, 0x03];
        let context = ParseContext::new(
            utxos.clone(),
            cert_keys.clone(),
            xpub.clone(),
            master_fingerprint.clone(),
        );
        assert_eq!(context.get_utxos().len(), 0);
        assert_eq!(context.get_cert_keys().len(), 0);
        assert_eq!(context.get_cardano_xpub(), xpub);
        assert_eq!(context.get_master_fingerprint(), master_fingerprint);
    }

    #[test]
    fn test_voting_procedure_new() {
        let procedure = VotingProcedure {
            voter_type: "DRep key".to_string(),
            voter: "voter_hash".to_string(),
            transaction_id: "tx_id".to_string(),
            index: "0".to_string(),
            vote: "Yes".to_string(),
        };
        assert_eq!(procedure.get_voter_type(), "DRep key");
        assert_eq!(procedure.get_voter(), "voter_hash");
        assert_eq!(procedure.get_transaction_id(), "tx_id");
        assert_eq!(procedure.get_index(), "0");
        assert_eq!(procedure.get_vote(), "Yes");
    }

    #[test]
    fn test_voting_proposal_new() {
        let proposal = VotingProposal {
            action: "Information".to_string(),
            deposit: "100 ADA".to_string(),
            reward_account: "stake_test1...".to_string(),
            anchor_url: "https://example.com".to_string(),
            anchor_data_hash: "anchor_hash".to_string(),
        };
        assert_eq!(proposal.get_action(), "Information");
        assert_eq!(proposal.get_deposit(), "100 ADA");
        assert_eq!(proposal.get_reward_account(), "stake_test1...");
        assert_eq!(proposal.get_anchor_url(), "https://example.com");
        assert_eq!(proposal.get_anchor_data_hash(), "anchor_hash");
    }

    #[test]
    fn test_governance_action_names() {
        assert_eq!(
            ParsedCardanoTx::governance_action_name(GovernanceActionKind::InfoAction),
            "Information"
        );
        assert_eq!(
            ParsedCardanoTx::governance_action_name(
                GovernanceActionKind::TreasuryWithdrawalsAction
            ),
            "Treasury withdrawals"
        );
        assert_eq!(
            ParsedCardanoTx::governance_action_name(GovernanceActionKind::HardForkInitiationAction),
            "Hard fork initiation"
        );
    }

    #[test]
    fn test_voter_type_names_include_scripts() {
        assert_eq!(
            ParsedCardanoTx::voter_type_name(VoterKind::DRepScriptHash),
            "DRep script"
        );
        assert_eq!(
            ParsedCardanoTx::voter_type_name(VoterKind::ConstitutionalCommitteeHotScriptHash),
            "Committee script"
        );
        assert_eq!(
            ParsedCardanoTx::voter_type_name(VoterKind::StakingPoolKeyHash),
            "Stake pool key"
        );
    }

    #[test]
    fn test_cardano_certificate_new() {
        let fields = vec![CertField {
            label: "Address".to_string(),
            value: "addr1...".to_string(),
        }];
        let cert = CardanoCertificate::new("Stake Pool Delegation".to_string(), fields.clone());
        assert_eq!(cert.get_cert_type(), "Stake Pool Delegation");
        assert_eq!(cert.get_fields().len(), 1);
    }

    #[test]
    fn test_cardano_withdrawal_new() {
        let withdrawal = CardanoWithdrawal {
            address: "addr1...".to_string(),
            amount: "1 ADA".to_string(),
        };
        assert_eq!(withdrawal.get_address(), "addr1...");
        assert_eq!(withdrawal.get_amount(), "1 ADA");
    }

    #[test]
    fn test_cert_field_new() {
        let field = CertField {
            label: "Address".to_string(),
            value: "addr1...".to_string(),
        };
        assert_eq!(field.get_label(), "Address");
        assert_eq!(field.get_value(), "addr1...");
    }

    #[test]
    fn test_cardano_utxo_new() {
        let mfp = vec![0x52, 0x74, 0x47, 0x03];
        let path = DerivationPath::from_str("m/1852'/1815'/0'/0/0").unwrap();
        let tx_hash = vec![0x01, 0x02, 0x03];
        let utxo = CardanoUtxo::new(
            mfp.clone(),
            "addr1...".to_string(),
            path.clone(),
            1000000,
            tx_hash.clone(),
            0,
        );
        assert_eq!(utxo.get_master_fingerprint(), mfp);
        assert_eq!(utxo.get_address(), "addr1...");
        assert_eq!(utxo.get_value(), 1000000);
        assert_eq!(utxo.get_index(), 0);
    }

    #[test]
    fn test_cardano_cert_key_new() {
        let mfp = vec![0x52, 0x74, 0x47, 0x03];
        let key_hash = vec![0x01, 0x02, 0x03];
        let path = DerivationPath::from_str("m/1852'/1815'/0'/2/0").unwrap();
        let cert_key = CardanoCertKey::new(mfp.clone(), key_hash.clone(), path.clone());
        assert_eq!(cert_key.get_master_fingerprint(), mfp);
        assert_eq!(cert_key.get_key_hash(), key_hash);
    }

    #[test]
    fn test_sign_data_result_new() {
        let pub_key = vec![0x01, 0x02, 0x03];
        let signature = vec![0x04, 0x05, 0x06];
        let result = SignDataResult::new(pub_key.clone(), signature.clone());
        assert_eq!(result.get_pub_key(), pub_key);
        assert_eq!(result.get_signature(), signature);
    }

    #[test]
    fn test_sign_voting_registration_result_new() {
        let signature = vec![0x01, 0x02, 0x03];
        let result = SignVotingRegistrationResult::new(signature.clone());
        assert_eq!(result.get_signature(), signature);
    }

    #[test]
    fn test_cardano_from_new() {
        let from = CardanoFrom {
            address: "addr1...".to_string(),
            amount: "1 ADA".to_string(),
            path: Some("m/1852'/1815'/0'/0/0".to_string()),
            transaction_id: "00".repeat(32),
            index: 0,
            known: true,
            value: 1000000,
        };
        assert_eq!(from.get_address(), "addr1...");
        assert_eq!(from.get_amount(), "1 ADA");
        assert_eq!(from.get_path(), Some("m/1852'/1815'/0'/0/0".to_string()));
        assert_eq!(from.get_value(), 1000000);
    }

    #[test]
    fn test_cardano_to_new() {
        let to = CardanoTo {
            address: "addr1...".to_string(),
            amount: "2 ADA".to_string(),
            assets: BTreeMap::new(),
            assets_text: Some("assets".to_string()),
            value: 2000000,
        };
        assert_eq!(to.get_address(), "addr1...");
        assert_eq!(to.get_amount(), "2 ADA");
        assert_eq!(to.get_value(), 2000000);
    }

    #[test]
    fn test_cardano_certificate_multiple_fields() {
        let fields = vec![
            CertField {
                label: "Address".to_string(),
                value: "addr1...".to_string(),
            },
            CertField {
                label: "Pool".to_string(),
                value: "pool1...".to_string(),
            },
        ];
        let cert = CardanoCertificate::new("Stake Pool Delegation".to_string(), fields.clone());
        assert_eq!(cert.get_cert_type(), "Stake Pool Delegation");
        assert_eq!(cert.get_fields().len(), 2);
    }

    #[test]
    fn test_parse_context_with_data() {
        let utxo = CardanoUtxo::new(
            vec![0x52, 0x74, 0x47, 0x03],
            "addr1...".to_string(),
            DerivationPath::from_str("m/1852'/1815'/0'/0/0").unwrap(),
            1000000,
            vec![0x01],
            0,
        );
        let utxos = vec![utxo];
        let cert_key = CardanoCertKey::new(
            vec![0x52, 0x74, 0x47, 0x03],
            vec![0x01],
            DerivationPath::from_str("m/1852'/1815'/0'/2/0").unwrap(),
        );
        let cert_keys = vec![cert_key];
        let xpub = Some("test_xpub".to_string());
        let master_fingerprint = vec![0x52, 0x74, 0x47, 0x03];
        let context = ParseContext::new(
            utxos.clone(),
            cert_keys.clone(),
            xpub.clone(),
            master_fingerprint.clone(),
        );
        assert_eq!(context.get_utxos().len(), 1);
        assert_eq!(context.get_cert_keys().len(), 1);
    }

    #[test]
    fn test_normalize_coin_small_fractional() {
        let value = 1u64; // 0.000001 ADA
        let result = normalize_coin(value);
        assert!(result.contains("ADA"));
    }

    #[test]
    fn test_normalize_value_small_fractional() {
        let value = 1u64; // 0.000001
        let result = normalize_value(value);
        assert!(!result.is_empty());
    }

    #[test]
    fn test_parse_sign_cip8_data_invalid_cbor_fallback() {
        let invalid_cbor = vec![0xff, 0xff, 0xff];
        let xpub = "ca0e65d9bb8d0dca5e88adc5e1c644cc7d62e5a139350330281ed7e3a6938d2c";
        let data = ParsedCardanoSignCip8Data::build(
            invalid_cbor.clone(),
            "m/1852'/1815'/0'/0/0".to_string(),
            xpub.to_string(),
            false,
        )
        .unwrap();
        assert_eq!(data.get_derivation_path(), "m/1852'/1815'/0'/0/0");
        assert_eq!(data.get_hash_payload(), false);
    }

    #[test]
    fn test_voting_procedure_different_votes() {
        let yes_vote = VotingProcedure {
            voter_type: "DRep key".to_string(),
            voter: "voter".to_string(),
            transaction_id: "tx".to_string(),
            index: "0".to_string(),
            vote: "Yes".to_string(),
        };
        let no_vote = VotingProcedure {
            voter_type: "DRep key".to_string(),
            voter: "voter".to_string(),
            transaction_id: "tx".to_string(),
            index: "0".to_string(),
            vote: "No".to_string(),
        };
        assert_ne!(yes_vote.get_vote(), no_vote.get_vote());
    }

    #[test]
    fn test_cardano_withdrawal_different_amounts() {
        let w1 = CardanoWithdrawal {
            address: "addr1...".to_string(),
            amount: "1 ADA".to_string(),
        };
        let w2 = CardanoWithdrawal {
            address: "addr1...".to_string(),
            amount: "2 ADA".to_string(),
        };
        assert_eq!(w1.get_address(), w2.get_address());
        assert_ne!(w1.get_amount(), w2.get_amount());
    }

    #[test]
    fn test_cardano_from_without_path() {
        let from = CardanoFrom {
            address: "addr1...".to_string(),
            amount: "1 ADA".to_string(),
            path: None,
            transaction_id: "00".repeat(32),
            index: 0,
            known: false,
            value: 1000000,
        };
        assert_eq!(from.get_path(), None);
    }
}
