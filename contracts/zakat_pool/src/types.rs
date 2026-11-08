// SPDX-License-Identifier: Apache-2.0

use cipherphi_interface::{Allocation, Category, Parameters};
use soroban_sdk::{contracterror, contractevent, contracttype, Address, Vec};

/// The storage keys that the pool uses.
///
/// Configuration is held in instance storage, which shares the contract's
/// lifetime, and only the per-asset balances are keyed separately. Nothing can be
/// held in temporary storage. This avoids temporary storage to stop a queued
/// change from vanishing mid-timelock.
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum DataKey {
    Governance,
    /// The approval multisig that authorises each payout.
    Authority,
    PolicyAddr,
    ApprovedAssets,
    Paused,
    TimelockLedgers,
    PendingChange,
    /// The persistent entry that holds the 8 buckets of one approved asset.
    CategoryBalances(Address),
}

/// A governance change that waits out the timelock.
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Change {
    SetPolicy(Address),
    /// A replacement for the approval multisig, used when its keys are lost.
    SetAuthority(Address),
    ApproveAsset(Address),
    SetAllocation(Allocation),
    SetParameters(Parameters),
}

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PendingChange {
    pub change: Change,
    /// The ledger after which the change may execute.
    pub execute_after: u32,
}

/// The refusals that the pool returns, one variant per refusal.
///
/// A retired number cannot be reused, and the pool's numbers stay below
/// `policy`'s, which start at 30. This keeps every number stable to let a failed
/// transaction name its refusal without ambiguity.
#[contracterror]
#[derive(Copy, Clone, Debug, Eq, PartialEq, PartialOrd, Ord)]
#[repr(u32)]
pub enum Error {
    /// The amount is not positive.
    InvalidAmount = 2,
    /// The allocation does not hold exactly 8 non-negative amounts summing to the
    /// amount received.
    InvalidAllocation = 3,
    /// The arithmetic overflows.
    Overflow = 4,
    // Numbers 5 to 10 cannot be reused: they were the attestation binding errors
    // (`AttestationMismatch`, `WrongPool`, `WrongNetwork`, `WrongAsset`, `Expired`,
    // `SequenceMismatch`). These bindings can only be enforced by Stellar now, at
    // `require_auth`.
    // Numbers 11 and 12 cannot be reused: they were `UnknownSigner` and
    // `ThresholdNotMet`, from when this contract counted signatures itself.
    /// The category bucket does not hold the amount.
    InsufficientBucket = 13,
    /// The buckets claim more than the pool holds.
    Insolvent = 14,
    /// The caller is neither governance nor the approval multisig. Number 15 was
    /// never assigned before.
    Unauthorised = 15,
    /// The recipient is the pool itself.
    SelfDistribution = 16,
    /// The asset is not in `approved_assets`.
    AssetNotApproved = 17,
    /// The pool is paused for contributions.
    ContributionsPaused = 18,
    // Number 19 cannot be reused: it was `DistributorDisabled`, and there is no
    // distributor.
    /// The delay is under 120,960 ledgers.
    TimelockTooShort = 20,
    /// The queue already holds a change.
    ChangeAlreadyPending = 21,
    /// The queue holds no change.
    NoPendingChange = 22,
    /// The ledger has not passed the change's `execute_after`.
    TimelockNotElapsed = 23,
    // Number 24 cannot be reused: it was `AttestationRejected`, and there is no
    // attestation contract.
    /// The token moves a different amount from the one requested.
    TransferShortfall = 25,
    /// The asset is already approved.
    AssetAlreadyApproved = 26,
    /// The `policy` contract refuses the queued value.
    PolicyRefused = 27,
    /// The governance and approval accounts are the same address.
    RolesOverlap = 28,
    /// The `policy` contract does not name this pool as its controller.
    PolicyControllerMismatch = 29,
}

/// The event that `contribute` emits, with the whole split in [`Category`]
/// order.
///
/// This carries all 8 amounts to let a reader with only the event stream rebuild
/// every balance.
#[contractevent]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Contribution {
    #[topic]
    pub donor: Address,
    #[topic]
    pub asset: Address,
    pub amount: i128,
    pub split: Vec<i128>,
}

/// The event that `distribute` emits.
///
/// The signed approval can only be found in the transaction that carries this
/// event. This leaves the approval out of the event to keep a single copy of
/// exactly what the signers approved.
#[contractevent]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Disbursement {
    #[topic]
    pub recipient: Address,
    #[topic]
    pub category: Category,
    pub asset: Address,
    pub amount: i128,
}

#[contractevent]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ChangeProposed {
    pub change: Change,
    pub execute_after: u32,
}

#[contractevent]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ChangeExecuted {
    pub change: Change,
    pub execute_after: u32,
}

#[contractevent]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ChangeCancelled {
    pub change: Change,
    pub execute_after: u32,
    /// The role that cancelled the change: governance or the approval multisig.
    pub by: Address,
}

#[contractevent]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ContributionsPaused {}

#[contractevent]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ContributionsResumed {}
