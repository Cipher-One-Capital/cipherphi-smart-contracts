// SPDX-License-Identifier: Apache-2.0
#![cfg(test)]

extern crate std;

use soroban_sdk::{testutils::Address as _, xdr::ToXdr, Address, BytesN, Env};

use crate::{Attestation, AttestationArgs, AttestationClient};
use cipherphi_interface::{Attestation as Payload, Category};

#[test]
fn constructor_stores_the_approval_account() {
    let env = Env::default();
    let authority = Address::generate(&env);
    let id = env.register(Attestation, AttestationArgs::__constructor(&authority));
    assert_eq!(AttestationClient::new(&env, &id).authority(), authority);
}

// --- the demo's encoding, unchanged ------------------------------------------

const VECTOR_JSON: &str = include_str!("../../test_vectors/attestation.json");

fn hex(bytes: &[u8]) -> std::string::String {
    let mut out = std::string::String::new();
    for byte in bytes {
        out.push_str(&std::format!("{:02x}", byte));
    }
    out
}

fn vector_field(key: &str) -> &'static str {
    let needle = std::format!("\"{}\":", key);
    let after = VECTOR_JSON
        .find(&needle)
        .map(|at| at + needle.len())
        .unwrap_or_else(|| panic!("the committed vector has no {} field", key));
    let rest = &VECTOR_JSON[after..];
    let open = rest.find('"').expect("an unquoted value") + 1;
    let close = open + rest[open..].find('"').expect("an unterminated value");
    &rest[open..close]
}

/// The approval account signs over `distribute` with this struct as its
/// argument, and the Disbursement event carries the hash of its XDR, so the app
/// and the contract must encode it identically. The vector's signature fields
/// belong to the earlier in-contract check and are not read here.
#[test]
fn attestation_xdr_matches_the_committed_vector() {
    let env = Env::default();
    let payload = Payload {
        recipient: Address::from_str(&env, vector_field("recipient")),
        asset: Address::from_str(&env, vector_field("asset")),
        category: Category::Poor,
        amount: vector_field("amount").parse().unwrap(),
        sequence: vector_field("sequence").parse().unwrap(),
        expiry_ledger: vector_field("expiry_ledger").parse().unwrap(),
        pool: Address::from_str(&env, vector_field("pool")),
        network_id: BytesN::from_array(&env, &[7u8; 32]),
    };
    assert_eq!(vector_field("network_id"), hex(&[7u8; 32]));

    let encoded: std::vec::Vec<u8> = payload.to_xdr(&env).iter().collect();
    assert_eq!(hex(&encoded), vector_field("signed_bytes_hex"));
}
