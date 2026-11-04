// SPDX-License-Identifier: Apache-2.0

//! # attestation
//!
//! Names the approval account: the Stellar account whose signers approve a
//! payout. The account is a multisig, 3 signers with a medium threshold of 2 and
//! the master key at weight 0, and Stellar itself refuses an approval that does
//! not reach the threshold. A contract cannot see an account's signers, so the
//! deploy script reads them from the ledger and refuses to continue unless they
//! hold.
//!
//! The address is fixed at construction and has no setter. Pointing the pool at
//! a different account means deploying a new contract and repointing the pool
//! at it, under the timelock. It holds no funds and cannot call back into the
//! pool, and the pool checks every binding of a payout itself.
//!
//! Testnet only, and unaudited.

#![no_std]

#[cfg(test)]
mod test;

use soroban_sdk::{contract, contractimpl, contracttype, Address, Env};

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
enum DataKey {
    Authority,
}

#[contract]
pub struct Attestation;

#[contractimpl]
impl Attestation {
    /// `__constructor` runs once, atomically with deployment, so no window
    /// exists in which someone else could install their own account.
    pub fn __constructor(env: Env, authority: Address) {
        env.storage()
            .instance()
            .set(&DataKey::Authority, &authority);
    }

    /// The approval account.
    pub fn authority(env: Env) -> Address {
        env.storage().instance().get(&DataKey::Authority).unwrap()
    }
}
