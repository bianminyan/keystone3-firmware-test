//! Structural review information derived only from the validated message.
use crate::message::Message;
use alloc::{format, string::String, vec::Vec};
use bitcoin::base58;

#[derive(Clone, Debug)]
pub struct AdvancedItem {
    pub title: String,
    pub value: String,
}

fn item(title: impl Into<String>, value: impl Into<String>) -> AdvancedItem {
    AdvancedItem {
        title: title.into(),
        value: value.into(),
    }
}

impl Message {
    pub(crate) fn advanced_review(&self, signer: Option<&[u8; 32]>) -> Vec<AdvancedItem> {
        let required = self.header.num_required_signatures as usize;
        let static_len = self.accounts.len();
        let mut accounts: Vec<(String, bool, bool)> = self
            .accounts
            .iter()
            .enumerate()
            .map(|(i, a)| {
                let is_signer = i < required;
                let writable = if is_signer {
                    i < required - self.header.num_readonly_signed_accounts as usize
                } else {
                    i < static_len - self.header.num_readonly_unsigned_accounts as usize
                };
                (base58::encode(&a.value), is_signer, writable)
            })
            .collect();
        // Loaded writable accounts precede ALL loaded readonly accounts.
        if let Some(lookups) = &self.address_table_lookups {
            for writable in [true, false] {
                for lookup in lookups {
                    let indexes = if writable {
                        &lookup.writable_indexes
                    } else {
                        &lookup.readonly_indexes
                    };
                    for index in indexes {
                        accounts.push((
                            format!(
                                "Unresolved ALT\nTable: {}\nIndex: {}",
                                base58::encode(&lookup.account_key.value),
                                index
                            ),
                            false,
                            writable,
                        ));
                    }
                }
            }
        }
        let version = if self.transaction_config.is_some() {
            "V1"
        } else if self.is_versioned {
            "V0"
        } else {
            "Legacy"
        };
        let mut items = alloc::vec![
            item("Message version", version),
            item("Required signatures", format!("{}", required)),
            item(
                "Fee payer",
                self.accounts
                    .first()
                    .map(|a| base58::encode(&a.value))
                    .unwrap_or_else(|| String::from("Not present"))
            ),
        ];
        if let Some(signer) = signer {
            items.push(item("Device signer", base58::encode(signer)));
        }
        for (i, (address, signer, writable)) in accounts.iter().enumerate() {
            items.push(item(
                format!("Account #{}", i),
                format!(
                    "{} | {}\n{}",
                    if *signer { "Signer" } else { "Non-signer" },
                    if *writable { "Writable" } else { "Read-only" },
                    address
                ),
            ));
        }
        for (i, instruction) in self.instructions.iter().enumerate() {
            let program = &accounts[instruction.program_index as usize].0;
            let mut value = format!(
                "Program [{}]\n{}\nAccount indexes: {:?}\nData: {} bytes",
                instruction.program_index,
                program,
                instruction.account_indexes,
                instruction.data.len()
            );
            if self.transaction_config.is_some()
                && program == "ComputeBudget111111111111111111111111111111"
            {
                value.push_str("\nIgnored in V1");
            }
            items.push(item(format!("Instruction #{}", i + 1), value));
        }
        items.push(item(
            if self.transaction_config.is_some() {
                "Lifetime specifier"
            } else {
                "Recent blockhash"
            },
            base58::encode(&self.block_hash.value),
        ));
        items
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use alloc::vec;

    #[test]
    fn empty_account_list_does_not_panic() {
        let mut raw = vec![0, 0, 0, 0];
        raw.extend([0; 32]);
        raw.push(0);
        let message = Message::read_exact(&mut raw).unwrap();
        assert!(message
            .advanced_review(None)
            .iter()
            .any(|i| i.title == "Fee payer" && i.value == "Not present"));
    }

    #[test]
    fn permissions_and_alt_order_are_preserved() {
        // Two signers (second readonly), one writable unsigned account, one
        // readonly program, then two tables with writable and readonly entries.
        let mut raw = vec![128, 2, 1, 1, 4];
        for byte in 1..=4 {
            raw.extend([byte; 32]);
        }
        raw.extend([9; 32]);
        raw.extend([1, 3, 4, 0, 4, 5, 7, 0, 2]);
        for byte in 5..=6 {
            raw.extend([byte; 32]);
            raw.extend([1, 7, 1, 8]);
        }
        let message = Message::read_exact(&mut raw).unwrap();
        let fields = message.advanced_review(Some(&[1; 32]));
        let value = |title: &str| {
            fields
                .iter()
                .find(|i| i.title == title)
                .unwrap()
                .value
                .as_str()
        };
        assert!(value("Account #0").starts_with("Signer | Writable"));
        assert!(value("Account #1").starts_with("Signer | Read-only"));
        assert!(value("Account #2").starts_with("Non-signer | Writable"));
        assert!(value("Account #3").starts_with("Non-signer | Read-only"));
        assert_eq!(
            value("Account #5"),
            format!(
                "Non-signer | Writable\nUnresolved ALT\nTable: {}\nIndex: 7",
                base58::encode(&[6; 32])
            )
        );
        assert_eq!(
            value("Account #6"),
            format!(
                "Non-signer | Read-only\nUnresolved ALT\nTable: {}\nIndex: 8",
                base58::encode(&[5; 32])
            )
        );
        assert!(value("Instruction #1").contains("Program [3]"));
        assert!(value("Instruction #1").contains("Account indexes: [0, 4, 5, 7]"));
        assert_eq!(value("Device signer"), base58::encode(&[1; 32]));
    }
}
