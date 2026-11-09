// SPDX-License-Identifier: Apache-2.0

//! The crate that holds what the two CipherPhi contracts share: the types, and
//! the interface that `zakat_pool` calls.
//!
//! `zakat_pool` can only link this crate, never `policy`. This keeps policy code
//! out of the custody Wasm to let the pool treat whatever answers at the pinned
//! address as untrusted.

#![no_std]

use soroban_sdk::{contractclient, contracttype, Address, Env, Vec};

/// The number of zakat categories, used as the length of every split.
pub const CATEGORY_COUNT: u32 = 8;
/// The total, in basis points, that every split sums to.
pub const BPS_TOTAL: u32 = 10_000;

/// The eight zakat categories that Qur'an 9:60 names, in the order the verse
/// names them.
///
/// Balances can only be reported in this order, by events and views alike.
#[contracttype]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Category {
    /// The poor (al-fuqara).
    Poor = 0,
    /// The needy (al-masakin).
    Needy = 1,
    /// The administrators of zakat (al-amilina alayha).
    Administrators = 2,
    /// The people whose hearts are to be reconciled (al-muallafati qulubuhum).
    Reconciliation = 3,
    /// The captives to be freed (fi al-riqab).
    Captives = 4,
    /// The people in debt (al-gharimin).
    Debtors = 5,
    /// The cause of God (fi sabilillah).
    CauseOfGod = 6,
    /// The wayfarer (ibn al-sabil).
    Wayfarers = 7,
}

/// The category split that `policy` applies to every contribution.
///
/// The values are placeholders until the Shariah authority validates them.
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Allocation {
    /// The basis points per category, in [`Category`] order, summing to 10,000.
    pub split_bps: Vec<u32>,
    /// The category that takes the rounding remainder.
    pub remainder: Category,
}

/// The standard, gold or silver, that the nisab threshold is measured in.
///
/// The price and its timestamp can only be supplied by the application, and are
/// shown beside the figure they produce.
#[contracttype]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum NisabBasis {
    Gold = 0,
    Silver = 1,
}

/// The point at which a donor's obligation is treated as discharged.
#[contracttype]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DischargeTiming {
    Received = 0,
    Allocated = 1,
    Delivered = 2,
}

/// The Shariah parameters that `policy` holds besides the split.
///
/// The values are placeholders until the Shariah authority validates them.
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Parameters {
    pub nisab_basis: NisabBasis,
    /// The zakat rate in basis points, read by the application's calculator.
    pub rate_bps: u32,
    /// The ceiling on the administrators' share, in basis points.
    pub amil_cap_bps: u32,
    pub discharge_timing: DischargeTiming,
}

/// The interface that `zakat_pool` calls on `policy`.
#[contractclient(name = "PolicyClient")]
pub trait PolicyInterface {
    /// Splits `amount` into 8 amounts in [`Category`] order.
    fn allocate(env: Env, amount: i128) -> Vec<i128>;
    /// Replaces the split when the controller authorises it.
    fn set_allocation(env: Env, allocation: Allocation);
    /// Replaces the parameters when the controller authorises it.
    fn set_parameters(env: Env, parameters: Parameters);
    /// Returns the refusal that `set_allocation` would give, without storing.
    fn check_allocation(env: Env, allocation: Allocation);
    /// Returns the refusal that `set_parameters` would give, without storing.
    fn check_parameters(env: Env, parameters: Parameters);
    /// Returns the one address that the setters answer to.
    fn controller(env: Env) -> Address;
}
