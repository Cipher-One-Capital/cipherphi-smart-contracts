// SPDX-License-Identifier: Apache-2.0

//! # zakat_pool
//!
//! The custody contract that holds the 8 category balances per asset and the
//! only 2 entry points that move value.
//!
//! Its code cannot be replaced in place: custody can only be changed via a new
//! deployment and a visible migration. This omits
//! `update_current_contract_wasm` to keep the code that holds donor funds from
//! becoming an admin key by another name.
//!
//! `policy` is treated as untrusted: a split can only be accepted if it has
//! exactly 8 elements, each zero or positive, summing exactly to the amount
//! received.
//!
//! A payout can only be made via the approval multisig's authorisation over its
//! exact arguments, and only to a recipient that governance approved. This limits
//! where a payout can go to keep stolen approval keys from reaching an address
//! the thief picks.
//!
//! A short quorum, a changed argument, a replay, another network
//! or another pool is refused by Stellar at `require_auth`, before any of the
//! pool's checks.
//!
//! Contributions are paused at deployment and can only be resumed by governance.
//! This keeps the pool closed until the deploy script has verified both multisig
//! accounts from the ledger, to guarantee nothing reaches the pool first.
//!
//! No TTL is extended by either value-moving function. This leaves rent to the
//! scheduled job to keep restoration fees off donor transactions while the job
//! runs.
//!
//! Testnet only, and unaudited. See `ARCHITECTURE.md`.

#![no_std]

#[cfg(test)]
mod test;

mod types;

pub use cipherphi_interface::Category;
pub use types::*;

use cipherphi_interface::{PolicyClient, CATEGORY_COUNT};
use soroban_sdk::{
    contract, contractimpl, panic_with_error, token, Address, Env, IntoVal, TryFromVal, Val, Vec,
};

/// The minimum delay, in ledgers, that a constructor can set: 7 days at the
/// 5-second ledger close Stellar targets.
///
/// The delay can only be counted in ledgers, so faster ledgers would make it
/// shorter in wall-clock time.
pub const MIN_TIMELOCK_LEDGERS: u32 = 120_960;

/// The number of ledgers, after `execute_after`, during which a queued change can
/// execute: 7 days at the 5-second ledger close.
///
/// A change that is not executed in this window can only be cancelled. This ends
/// every change's life to keep an old approval from executing once nobody is
/// watching for it.
pub const CHANGE_WINDOW_LEDGERS: u32 = 120_960;

#[contract]
pub struct ZakatPool;

#[contractimpl]
impl ZakatPool {
    /// Stores the configuration when the pool is deployed.
    ///
    /// The constructor can only run once, atomically with deployment. This removes
    /// any separately callable initialiser to leave no window in which the pool can
    /// be captured.
    ///
    /// An empty `recipients` can be accepted, and a recipient can only be accepted
    /// if it is not the pool, governance, the approval multisig or a duplicate.
    ///
    /// The approval multisig cannot be the pool or `policy`. This refuses both to
    /// keep payouts from being stopped by an account that never signs.
    pub fn __constructor(
        env: Env,
        governance: Address,
        authority: Address,
        policy: Address,
        asset: Address,
        recipients: Vec<Address>,
        timelock_ledgers: u32,
    ) -> Result<(), Error> {
        governance.require_auth();

        if timelock_ledgers < MIN_TIMELOCK_LEDGERS {
            return Err(Error::TimelockTooShort);
        }
        if governance == authority {
            return Err(Error::RolesOverlap);
        }
        if authority == env.current_contract_address() || authority == policy {
            return Err(Error::AuthorityCannotSign);
        }
        // The `policy` can only be accepted if it names this pool as its controller,
        // as in a repoint.
        require_controller(&env, &policy)?;

        let storage = env.storage().instance();
        storage.set(&DataKey::Governance, &governance);
        storage.set(&DataKey::Authority, &authority);
        storage.set(&DataKey::PolicyAddr, &policy);
        // Contributions cannot be accepted until the deployment preconditions are
        // verified.
        storage.set(&DataKey::Paused, &true);
        storage.set(&DataKey::TimelockLedgers, &timelock_ledgers);

        let mut assets = Vec::new(&env);
        assets.push_back(asset.clone());
        storage.set(&DataKey::ApprovedAssets, &assets);
        env.storage().persistent().set(
            &DataKey::CategoryBalances(asset.clone()),
            &empty_buckets(&env),
        );

        PoolConfigured {
            governance,
            authority,
            policy,
            asset,
            timelock_ledgers,
        }
        .publish(&env);

        for recipient in recipients.iter() {
            approve_recipient(&env, &recipient)?;
        }
        Ok(())
    }

    // --- the two functions that move value ---------------------------------

    /// Transfers `amount` of `asset` from `donor` into the pool and credits the 8
    /// category buckets when the asset is approved.
    ///
    /// A contribution cannot be gated. This leaves the entry point open to keep donor
    /// identity off the ledger.
    pub fn contribute(env: Env, donor: Address, asset: Address, amount: i128) -> Result<(), Error> {
        donor.require_auth();
        if amount <= 0 {
            return Err(Error::InvalidAmount);
        }
        if flag(&env, &DataKey::Paused) {
            return Err(Error::ContributionsPaused);
        }
        if !Self::is_approved(env.clone(), asset.clone()) {
            return Err(Error::AssetNotApproved);
        }

        // A bucket can only be credited after the money arrives. This transfers first
        // to guarantee no credit ever exists without its funds, even transiently.
        let pool = env.current_contract_address();
        let token = token::Client::new(&env, &asset);
        let held_before = token.balance(&pool);
        token.transfer(&donor, &pool, &amount);
        if token.balance(&pool).checked_sub(held_before) != Some(amount) {
            return Err(Error::TransferShortfall);
        }

        let split = allocation(&env, amount)?;
        let buckets = env.storage().persistent().try_update(
            &DataKey::CategoryBalances(asset.clone()),
            |stored: Option<Vec<i128>>| {
                let mut buckets = stored.unwrap_or_else(|| empty_buckets(&env));
                for index in 0..CATEGORY_COUNT {
                    let credited = buckets
                        .get_unchecked(index)
                        .checked_add(split.get_unchecked(index))
                        .ok_or(Error::Overflow)?;
                    buckets.set(index, credited);
                }
                Ok(buckets)
            },
        )?;

        assert_solvent(&env, &asset, &buckets)?;

        Contribution {
            donor,
            asset,
            amount,
            split,
        }
        .publish(&env);
        Ok(())
    }

    /// Transfers `amount` of `asset` from a category bucket to `recipient` when the
    /// approval multisig has authorised exactly that payout and governance has
    /// approved the recipient.
    ///
    /// The authorisation covers these 4 arguments, this pool, this network, a
    /// single-use nonce and an expiry. The transaction can be submitted by anyone,
    /// but what it pays cannot be changed.
    ///
    /// A payout cannot be stopped by the contribution pause. This keeps payouts open
    /// to guarantee a pause never strands what the pool already holds.
    pub fn distribute(
        env: Env,
        recipient: Address,
        asset: Address,
        amount: i128,
        category: Category,
    ) -> Result<(), Error> {
        // 1. The approval multisig, over exactly these arguments.
        Self::authority(env.clone()).require_auth();

        // 2. The recipient, approved by governance.
        if !Self::is_approved_recipient(env.clone(), recipient.clone()) {
            return Err(Error::RecipientNotApproved);
        }

        // 3. The arguments.
        if amount <= 0 {
            return Err(Error::InvalidAmount);
        }
        if !Self::is_approved(env.clone(), asset.clone()) {
            return Err(Error::AssetNotApproved);
        }
        let pool = env.current_contract_address();
        if recipient == pool {
            return Err(Error::SelfDistribution);
        }

        // 4. The bucket holds the amount. 5. Decrement, then transfer.
        let index = category as u32;
        let buckets = env.storage().persistent().try_update(
            &DataKey::CategoryBalances(asset.clone()),
            |stored: Option<Vec<i128>>| {
                let mut buckets = stored.unwrap_or_else(|| empty_buckets(&env));
                let balance = buckets.get_unchecked(index);
                if balance < amount {
                    return Err(Error::InsufficientBucket);
                }
                buckets.set(index, balance.checked_sub(amount).ok_or(Error::Overflow)?);
                Ok(buckets)
            },
        )?;

        let token = token::Client::new(&env, &asset);
        let held_before = token.balance(&pool);
        token.transfer(&pool, &recipient, &amount);
        if held_before.checked_sub(token.balance(&pool)) != Some(amount) {
            return Err(Error::TransferShortfall);
        }

        // 6. Solvency.
        assert_solvent(&env, &asset, &buckets)?;

        // 7. The event.
        Disbursement {
            recipient,
            category,
            asset,
            amount,
        }
        .publish(&env);
        Ok(())
    }

    // --- instant controls ----------------------------------------------------

    /// Pauses contributions at once, when governance authorises it.
    ///
    /// Payouts cannot be stopped by the pause.
    pub fn pause_contributions(env: Env) {
        Self::governance(env.clone()).require_auth();
        env.storage().instance().set(&DataKey::Paused, &true);
        ContributionsPaused {}.publish(&env);
    }

    /// Resumes contributions at once, when governance authorises it.
    ///
    /// Resuming cannot be delayed by the timelock. This keeps it instant to stop a
    /// stolen governance quorum from holding donations shut for a full delay.
    pub fn resume_contributions(env: Env) {
        Self::governance(env.clone()).require_auth();
        env.storage().instance().set(&DataKey::Paused, &false);
        ContributionsResumed {}.publish(&env);
    }

    /// Removes `recipient` from the approved recipients at once, when the approval
    /// multisig authorises it.
    ///
    /// An instant removal cannot be made by governance. This keeps it with the
    /// approval multisig to stop a stolen governance quorum from freezing payouts,
    /// and to let the signers withdraw an approval they signed before a partner was
    /// compromised.
    pub fn remove_recipient(env: Env, caller: Address, recipient: Address) -> Result<(), Error> {
        if caller != Self::authority(env.clone()) {
            return Err(Error::Unauthorised);
        }
        caller.require_auth();
        remove_recipient(&env, &recipient, caller)
    }

    // --- changes behind the timelock -----------------------------------------

    /// Queues `change` and returns the ledger after which it may execute, when
    /// governance authorises it.
    ///
    /// Only one change can be queued at a time. This keeps a single slot to
    /// guarantee each change is executed or cancelled before the next is proposed.
    ///
    /// A change can only be queued if it passes the checks it must pass at
    /// execution. This checks it twice to keep a bad value from waiting out the
    /// delay in the only slot.
    pub fn propose_change(env: Env, change: Change) -> Result<u32, Error> {
        Self::governance(env.clone()).require_auth();
        let storage = env.storage().instance();
        if storage.has(&DataKey::PendingChange) {
            return Err(Error::ChangeAlreadyPending);
        }
        check_change(&env, &change)?;
        let execute_after = env
            .ledger()
            .sequence()
            .checked_add(Self::timelock_ledgers(env.clone()))
            .ok_or(Error::Overflow)?;
        let execute_until = execute_after
            .checked_add(CHANGE_WINDOW_LEDGERS)
            .ok_or(Error::Overflow)?;
        storage.set(
            &DataKey::PendingChange,
            &PendingChange {
                change: change.clone(),
                execute_after,
                execute_until,
            },
        );
        ChangeProposed {
            change,
            execute_after,
            execute_until,
        }
        .publish(&env);
        Ok(execute_after)
    }

    /// Applies the queued change when governance authorises it and the ledger is
    /// past its `execute_after` and not past its `execute_until`.
    ///
    /// The change is checked again before it is applied. This repeats the checks to
    /// guarantee the change still holds against the state it now meets.
    pub fn execute_change(env: Env) -> Result<(), Error> {
        Self::governance(env.clone()).require_auth();
        let PendingChange {
            change,
            execute_after,
            execute_until,
        } = Self::pending_change(env.clone()).ok_or(Error::NoPendingChange)?;
        let ledger = env.ledger().sequence();
        if ledger <= execute_after {
            return Err(Error::TimelockNotElapsed);
        }
        if ledger > execute_until {
            return Err(Error::ChangeExpired);
        }
        check_change(&env, &change)?;

        let storage = env.storage().instance();
        match change.clone() {
            Change::SetPolicy(address) => storage.set(&DataKey::PolicyAddr, &address),
            Change::SetAuthority(address) => storage.set(&DataKey::Authority, &address),
            Change::ApproveAsset(asset) => {
                storage.try_update(&DataKey::ApprovedAssets, |stored: Option<Vec<Address>>| {
                    let mut assets = stored.unwrap_or_else(|| Vec::new(&env));
                    assets.push_back(asset.clone());
                    Ok::<_, Error>(assets)
                })?;
                env.storage()
                    .persistent()
                    .set(&DataKey::CategoryBalances(asset), &empty_buckets(&env));
            }
            Change::SetAllocation(allocation) => {
                match policy_client(&env).try_set_allocation(&allocation) {
                    Ok(Ok(())) => {}
                    _ => return Err(Error::PolicyRefused),
                }
            }
            Change::SetParameters(parameters) => {
                match policy_client(&env).try_set_parameters(&parameters) {
                    Ok(Ok(())) => {}
                    _ => return Err(Error::PolicyRefused),
                }
            }
            Change::ApproveRecipient(recipient) => approve_recipient(&env, &recipient)?,
            Change::RemoveRecipient(recipient) => {
                remove_recipient(&env, &recipient, Self::governance(env.clone()))?
            }
        }

        storage.remove(&DataKey::PendingChange);
        ChangeExecuted {
            change,
            execute_after,
        }
        .publish(&env);
        Ok(())
    }

    /// Drops the queued change when governance or the approval multisig authorises
    /// it.
    ///
    /// A change can be cancelled by either role. This lets the approval multisig
    /// cancel a `SetAuthority` queued by a stolen governance quorum, to guarantee
    /// governance alone never reaches the funds.
    pub fn cancel_change(env: Env, caller: Address) -> Result<(), Error> {
        if caller != Self::governance(env.clone()) && caller != Self::authority(env.clone()) {
            return Err(Error::Unauthorised);
        }
        caller.require_auth();
        let PendingChange {
            change,
            execute_after,
            ..
        } = Self::pending_change(env.clone()).ok_or(Error::NoPendingChange)?;
        env.storage().instance().remove(&DataKey::PendingChange);
        ChangeCancelled {
            change,
            execute_after,
            by: caller,
        }
        .publish(&env);
        Ok(())
    }

    // --- views ---------------------------------------------------------------

    /// Returns the 8 balances of `asset` in [`Category`] order, or zeros if the
    /// asset is not approved.
    pub fn balances(env: Env, asset: Address) -> Vec<i128> {
        env.storage()
            .persistent()
            .get(&DataKey::CategoryBalances(asset))
            .unwrap_or_else(|| empty_buckets(&env))
    }

    pub fn is_approved(env: Env, asset: Address) -> bool {
        Self::approved_assets(env).contains(&asset)
    }

    pub fn approved_assets(env: Env) -> Vec<Address> {
        config(&env, &DataKey::ApprovedAssets)
    }

    pub fn is_approved_recipient(env: Env, recipient: Address) -> bool {
        env.storage()
            .persistent()
            .get(&DataKey::ApprovedRecipient(recipient))
            .unwrap_or(false)
    }

    /// Returns whether `recipient` was removed, which bars it for good.
    pub fn is_removed_recipient(env: Env, recipient: Address) -> bool {
        env.storage()
            .persistent()
            .get(&DataKey::ApprovedRecipient(recipient))
            == Some(false)
    }

    pub fn governance(env: Env) -> Address {
        config(&env, &DataKey::Governance)
    }

    /// Returns the approval multisig.
    pub fn authority(env: Env) -> Address {
        config(&env, &DataKey::Authority)
    }

    pub fn policy(env: Env) -> Address {
        config(&env, &DataKey::PolicyAddr)
    }

    pub fn is_paused(env: Env) -> bool {
        flag(&env, &DataKey::Paused)
    }

    pub fn timelock_ledgers(env: Env) -> u32 {
        config(&env, &DataKey::TimelockLedgers)
    }

    pub fn pending_change(env: Env) -> Option<PendingChange> {
        env.storage().instance().get(&DataKey::PendingChange)
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

fn flag(env: &Env, key: &DataKey) -> bool {
    env.storage().instance().get(key).unwrap_or(false)
}

fn empty_buckets(env: &Env) -> Vec<i128> {
    let mut buckets = Vec::new(env);
    for _ in 0..CATEGORY_COUNT {
        buckets.push_back(0);
    }
    buckets
}

/// Rejects `change` unless it would be accepted now.
///
/// The check is run when the change is proposed and again when it executes. This
/// keeps one list of checks to guarantee both points refuse the same values.
fn check_change(env: &Env, change: &Change) -> Result<(), Error> {
    match change {
        Change::SetPolicy(address) => require_controller(env, address),
        Change::SetAuthority(address) => {
            if *address == ZakatPool::governance(env.clone()) {
                return Err(Error::RolesOverlap);
            }
            if *address == env.current_contract_address()
                || *address == ZakatPool::policy(env.clone())
            {
                return Err(Error::AuthorityCannotSign);
            }
            if ZakatPool::is_approved_recipient(env.clone(), address.clone()) {
                return Err(Error::RecipientIsRole);
            }
            Ok(())
        }
        Change::ApproveAsset(asset) => {
            if ZakatPool::is_approved(env.clone(), asset.clone()) {
                return Err(Error::AssetAlreadyApproved);
            }
            Ok(())
        }
        Change::SetAllocation(allocation) => {
            match policy_client(env).try_check_allocation(allocation) {
                Ok(Ok(())) => Ok(()),
                _ => Err(Error::PolicyRefused),
            }
        }
        Change::SetParameters(parameters) => {
            match policy_client(env).try_check_parameters(parameters) {
                Ok(Ok(())) => Ok(()),
                _ => Err(Error::PolicyRefused),
            }
        }
        Change::ApproveRecipient(recipient) => check_approvable(env, recipient),
        Change::RemoveRecipient(recipient) => {
            if !ZakatPool::is_approved_recipient(env.clone(), recipient.clone()) {
                return Err(Error::RecipientNotApproved);
            }
            Ok(())
        }
    }
}

/// Rejects `recipient` if it is the pool, governance, the approval multisig,
/// already approved or removed.
fn check_approvable(env: &Env, recipient: &Address) -> Result<(), Error> {
    if *recipient == env.current_contract_address() {
        return Err(Error::SelfDistribution);
    }
    if *recipient == ZakatPool::governance(env.clone())
        || *recipient == ZakatPool::authority(env.clone())
    {
        return Err(Error::RecipientIsRole);
    }
    match env
        .storage()
        .persistent()
        .get::<_, bool>(&DataKey::ApprovedRecipient(recipient.clone()))
    {
        Some(true) => Err(Error::RecipientAlreadyApproved),
        Some(false) => Err(Error::RecipientRemoved),
        None => Ok(()),
    }
}

/// Approves `recipient` and emits the event, unless [`check_approvable`] refuses
/// it.
fn approve_recipient(env: &Env, recipient: &Address) -> Result<(), Error> {
    check_approvable(env, recipient)?;
    env.storage()
        .persistent()
        .set(&DataKey::ApprovedRecipient(recipient.clone()), &true);
    RecipientApproved {
        recipient: recipient.clone(),
    }
    .publish(env);
    Ok(())
}

/// Removes `recipient` for good and emits the event naming `by`, unless it is not
/// approved.
///
/// A removed recipient is kept as `false`, never deleted. This bars it from a
/// later approval to guarantee no approval signed before the removal can pay it.
fn remove_recipient(env: &Env, recipient: &Address, by: Address) -> Result<(), Error> {
    if !ZakatPool::is_approved_recipient(env.clone(), recipient.clone()) {
        return Err(Error::RecipientNotApproved);
    }
    env.storage()
        .persistent()
        .set(&DataKey::ApprovedRecipient(recipient.clone()), &false);
    RecipientRemoved {
        recipient: recipient.clone(),
        by,
    }
    .publish(env);
    Ok(())
}

/// Rejects `policy` unless it names this pool as its controller.
///
/// A dishonest contract can still answer anything, and can only be caught by
/// review during the delay. This catches a deploy mistake to keep parameter
/// changes behind the timelock.
fn require_controller(env: &Env, policy: &Address) -> Result<(), Error> {
    match PolicyClient::new(env, policy).try_controller() {
        Ok(Ok(controller)) if controller == env.current_contract_address() => Ok(()),
        _ => Err(Error::PolicyControllerMismatch),
    }
}

fn policy_client(env: &Env) -> PolicyClient<'_> {
    PolicyClient::new(env, &ZakatPool::policy(env.clone()))
}

/// Asks `policy` for the split of `amount` and rejects it unless it has exactly
/// 8 elements, each zero or positive, summing exactly to `amount`.
///
/// A policy that fails to answer, or answers with the wrong type, is refused the
/// same way. This checks every element as well as the sum to stop
/// `[amount + x, -x, 0, ...]` from crediting one category from another.
fn allocation(env: &Env, amount: i128) -> Result<Vec<i128>, Error> {
    let split = match policy_client(env).try_allocate(&amount) {
        Ok(Ok(split)) => split,
        _ => return Err(Error::InvalidAllocation),
    };
    if split.len() != CATEGORY_COUNT {
        return Err(Error::InvalidAllocation);
    }
    let mut sum: i128 = 0;
    for part in split.iter() {
        if part < 0 {
            return Err(Error::InvalidAllocation);
        }
        sum = sum.checked_add(part).ok_or(Error::InvalidAllocation)?;
    }
    if sum != amount {
        return Err(Error::InvalidAllocation);
    }
    Ok(split)
}

/// Rejects the state if the buckets claim more than the pool holds.
///
/// A surplus sent straight to the pool can only sit in the balance: it belongs to
/// no bucket and cannot be paid out. This checks `<=` rather than `==` to keep a
/// direct transfer from blocking the pool.
fn assert_solvent(env: &Env, asset: &Address, buckets: &Vec<i128>) -> Result<(), Error> {
    let mut claimed: i128 = 0;
    for amount in buckets.iter() {
        claimed = claimed.checked_add(amount).ok_or(Error::Overflow)?;
    }
    let held = token::Client::new(env, asset).balance(&env.current_contract_address());
    if claimed > held {
        return Err(Error::Insolvent);
    }
    Ok(())
}
