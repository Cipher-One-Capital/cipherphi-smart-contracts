// SPDX-License-Identifier: Apache-2.0

//! # policy
//!
//! The contract that holds the 8-category split and the Shariah parameters, and
//! computes the allocation of a contribution. It holds no funds and makes no call
//! to the pool.
//!
//! Every value is bounds-checked at the setter, and the setters can only be
//! called by the `controller`, which is the `zakat_pool` contract. This answers
//! only to the pool to keep every change behind governance and the pool's
//! timelock.
//!
//! The deployed values are placeholders: they are not the Swiss Zakat
//! Foundation's allocation, and the Shariah authority has not validated them.
//!
//! Testnet only, and unaudited.

#![no_std]

#[cfg(test)]
mod test;

pub use cipherphi_interface::{Allocation, Category, DischargeTiming, NisabBasis, Parameters};
use cipherphi_interface::{BPS_TOTAL, CATEGORY_COUNT};
use soroban_sdk::{
    contract, contracterror, contractimpl, contracttype, panic_with_error, Address, Env, IntoVal,
    TryFromVal, Val, Vec,
};

/// The widest administrators' share that any setting can allow.
///
/// The bound is technical, not a religious ruling, and the validated cap sits
/// inside it. This rejects bad input to keep every setting inside a sane band.
pub const MAX_AMIL_CAP_BPS: u32 = 2_500;
/// The widest zakat rate that any setting can allow, on the same basis.
pub const MAX_RATE_BPS: u32 = 1_000;

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
enum DataKey {
    Controller,
    Allocation,
    Parameters,
}

/// The refusals that `policy` returns, numbered 30 to 39.
///
/// A number can only be used by one of the two contracts. This keeps `policy`
/// inside 30 to 39 to let a failed transaction name its contract without
/// ambiguity.
#[contracterror]
#[derive(Copy, Clone, Debug, Eq, PartialEq, PartialOrd, Ord)]
#[repr(u32)]
pub enum Error {
    /// The split does not have exactly 8 elements.
    SplitLength = 30,
    /// The split does not sum to 10,000 basis points.
    SplitSum = 31,
    /// The remainder category has no share of the split.
    RemainderWithoutShare = 32,
    /// The amil cap is outside its allowed band.
    AmilCapOutOfBand = 33,
    /// The zakat rate is outside its allowed band.
    RateOutOfBand = 34,
    /// The administrators' share is above the amil cap.
    AdministratorsAboveCap = 35,
    /// The remainder category is the administrators. This refuses it to stop the
    /// rounding remainder from carrying their credit past the amil cap.
    RemainderToAdministrators = 36,
    /// The configuration entry is missing.
    Unconfigured = 37,
    /// The amount is not positive.
    InvalidAmount = 38,
    /// The arithmetic overflows.
    Overflow = 39,
}

#[contract]
pub struct Policy;

#[contractimpl]
impl Policy {
    /// Stores the controller, the split and the parameters when the contract is
    /// deployed.
    ///
    /// The controller is the pool, whose address the deploy script computes before
    /// this contract is deployed. This orders deployment to let each contract name
    /// the other.
    pub fn __constructor(
        env: Env,
        controller: Address,
        allocation: Allocation,
        parameters: Parameters,
    ) -> Result<(), Error> {
        check(&allocation, &parameters)?;
        let storage = env.storage().instance();
        storage.set(&DataKey::Controller, &controller);
        storage.set(&DataKey::Allocation, &allocation);
        storage.set(&DataKey::Parameters, &parameters);
        Ok(())
    }

    pub fn set_allocation(env: Env, allocation: Allocation) -> Result<(), Error> {
        Self::controller(env.clone()).require_auth();
        check(&allocation, &Self::parameters(env.clone()))?;
        env.storage()
            .instance()
            .set(&DataKey::Allocation, &allocation);
        Ok(())
    }

    pub fn set_parameters(env: Env, parameters: Parameters) -> Result<(), Error> {
        Self::controller(env.clone()).require_auth();
        check(&Self::allocation(env.clone()), &parameters)?;
        env.storage()
            .instance()
            .set(&DataKey::Parameters, &parameters);
        Ok(())
    }

    /// Returns the refusal that `set_allocation` would give `allocation`, or
    /// nothing when it would be accepted.
    ///
    /// The check can be made by anyone and stores nothing. This lets the pool
    /// check a queued split when it is proposed to keep a bad value from waiting
    /// out the timelock.
    pub fn check_allocation(env: Env, allocation: Allocation) -> Result<(), Error> {
        check(&allocation, &Self::parameters(env))
    }

    /// Returns the refusal that `set_parameters` would give `parameters`, or
    /// nothing when they would be accepted.
    pub fn check_parameters(env: Env, parameters: Parameters) -> Result<(), Error> {
        check(&Self::allocation(env), &parameters)
    }

    /// Splits `amount` into 8 amounts in [`Category`] order.
    ///
    /// Every category but the remainder is floor-divided, and the remainder takes
    /// what is left. This assigns the leftover to one category to make the total
    /// exact by construction.
    pub fn allocate(env: Env, amount: i128) -> Result<Vec<i128>, Error> {
        if amount <= 0 {
            return Err(Error::InvalidAmount);
        }
        let allocation = Self::allocation(env.clone());
        let remainder_index = allocation.remainder as u32;

        let mut rest = amount;
        let mut parts: Vec<i128> = Vec::new(&env);
        for (index, bps) in allocation.split_bps.iter().enumerate() {
            if index as u32 == remainder_index {
                parts.push_back(0);
                continue;
            }
            let part = amount.checked_mul(bps as i128).ok_or(Error::Overflow)? / BPS_TOTAL as i128;
            rest = rest.checked_sub(part).ok_or(Error::Overflow)?;
            parts.push_back(part);
        }
        parts.set(remainder_index, rest);
        Ok(parts)
    }

    pub fn controller(env: Env) -> Address {
        config(&env, &DataKey::Controller)
    }

    pub fn allocation(env: Env) -> Allocation {
        config(&env, &DataKey::Allocation)
    }

    pub fn parameters(env: Env) -> Parameters {
        config(&env, &DataKey::Parameters)
    }
}

/// Reads a configuration entry that the constructor set.
///
/// A missing entry can only be reported as the typed `Unconfigured` error, never as
/// a bare panic. This names the failure to keep every refusal readable from the
/// transaction result.
fn config<V>(env: &Env, key: &DataKey) -> V
where
    V: IntoVal<Env, Val> + TryFromVal<Env, Val>,
{
    env.storage()
        .instance()
        .get(key)
        .unwrap_or_else(|| panic_with_error!(env, Error::Unconfigured))
}

/// Rejects an allocation and parameter pair that breaks any bound.
///
/// Each setter passes the stored half of the pair. This checks the pair together
/// to stop a cap from being lowered beneath the live split, or the other way
/// round.
fn check(allocation: &Allocation, parameters: &Parameters) -> Result<(), Error> {
    let split = &allocation.split_bps;
    if split.len() != CATEGORY_COUNT {
        return Err(Error::SplitLength);
    }
    let mut sum: u32 = 0;
    for bps in split.iter() {
        sum = sum.checked_add(bps).ok_or(Error::SplitSum)?;
    }
    if sum != BPS_TOTAL {
        return Err(Error::SplitSum);
    }
    if split.get_unchecked(allocation.remainder as u32) == 0 {
        return Err(Error::RemainderWithoutShare);
    }
    if allocation.remainder == Category::Administrators {
        return Err(Error::RemainderToAdministrators);
    }
    if parameters.amil_cap_bps > MAX_AMIL_CAP_BPS {
        return Err(Error::AmilCapOutOfBand);
    }
    if parameters.rate_bps == 0 || parameters.rate_bps > MAX_RATE_BPS {
        return Err(Error::RateOutOfBand);
    }
    if split.get_unchecked(Category::Administrators as u32) > parameters.amil_cap_bps {
        return Err(Error::AdministratorsAboveCap);
    }
    Ok(())
}
