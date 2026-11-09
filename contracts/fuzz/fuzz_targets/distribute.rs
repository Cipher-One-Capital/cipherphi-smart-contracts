// SPDX-License-Identifier: Apache-2.0

//! Fuzzes `distribute` with arbitrary payouts, approvals and tampering in 2 assets,
//! mixed with gifts, removals of the recipient, attempts to approve it again and
//! split changes through the timelock.
//! Checks that a payout is made only when the approval account authorised exactly the
//! submitted arguments, its bucket covers it and its recipient is approved, that a
//! removal is final, and that the buckets never exceed the balance in either asset.
//! The account's own 2-of-3 threshold can only be enforced by Stellar and is not modelled here.

#![no_main]

use cipherphi_fuzz::{Harness, ALL};
use libfuzzer_sys::{arbitrary, fuzz_target};
use policy::Allocation;
use soroban_sdk::{testutils::Address as _, token, Address, Vec};
use zakat_pool::{Change, Error};

#[derive(arbitrary::Arbitrary, Debug)]
struct Payout {
    category: u8,
    amount: i128,
    approved: bool,
    /// The argument that the signers approved differently, if any.
    tamper: u8,
    /// Whether the payout goes to an address that governance never approved.
    unapproved: bool,
    /// The asset paid: the first when even, the second when odd.
    asset: u8,
}

#[derive(arbitrary::Arbitrary, Debug)]
enum Step {
    Gift {
        asset: u8,
        amount: u32,
    },
    Pay(Payout),
    /// A removal of the recipient, at once by the approval account or through the
    /// timelock by governance.
    Remove {
        by_timelock: bool,
    },
    /// An attempt to approve the recipient again through the timelock.
    Reapprove,
    /// A new split through the timelock, from shares normalised to sum to 10,000.
    NewSplit {
        shares: [u8; 8],
    },
}

#[derive(arbitrary::Arbitrary, Debug)]
struct Input {
    steps: std::vec::Vec<Step>,
}

/// Turns arbitrary shares into a split the policy accepts, as the contribute target does.
fn split_from(shares: [u8; 8]) -> [u32; 8] {
    let mut split = [0u32; 8];
    let mut left = 10_000u32;
    for (index, share) in shares.iter().enumerate().skip(1) {
        let bps = (*share as u32 * 9).min(left.saturating_sub(1));
        split[index] = bps;
        left -= bps;
    }
    split[2] = split[2].min(2_500);
    split[0] = 10_000 - split[1..].iter().sum::<u32>();
    split
}

fuzz_target!(|input: Input| {
    let h = Harness::new([3000, 2000, 1000, 500, 500, 1500, 1000, 500]);
    let client = h.client();
    let mut removed = false;

    for step in input.steps.iter().take(16) {
        match step {
            Step::Gift { asset, amount } => {
                if *amount > 0 {
                    client.contribute(&h.donor, h.asset_at(*asset), &(*amount as i128));
                }
            }
            Step::Pay(payout) => pay(&h, payout, removed),
            Step::Remove { by_timelock } => {
                let result = if *by_timelock {
                    h.queue_and_execute(&Change::RemoveRecipient(h.recipient.clone()))
                } else {
                    match client.try_remove_recipient(&h.authority, &h.recipient) {
                        Ok(_) => Ok(()),
                        Err(Ok(error)) => Err(error),
                        Err(Err(error)) => panic!("remove_recipient failed in the host: {error:?}"),
                    }
                };
                if removed {
                    assert_eq!(result, Err(Error::RecipientNotApproved));
                } else {
                    assert_eq!(result, Ok(()));
                }
                removed = true;
            }
            Step::Reapprove => {
                let result = h.queue_and_execute(&Change::ApproveRecipient(h.recipient.clone()));
                let want = if removed {
                    Error::RecipientRemoved
                } else {
                    Error::RecipientAlreadyApproved
                };
                assert_eq!(result, Err(want));
            }
            Step::NewSplit { shares } => {
                let allocation = Allocation {
                    split_bps: Vec::from_slice(&h.env, &split_from(*shares)),
                    remainder: zakat_pool::Category::Poor,
                };
                assert_eq!(
                    h.queue_and_execute(&Change::SetAllocation(allocation)),
                    Ok(())
                );
            }
        }
        assert_eq!(client.is_approved_recipient(&h.recipient), !removed);
        h.assert_invariant();
    }
});

fn pay(h: &Harness, payout: &Payout, removed: bool) {
    let client = h.client();
    let paid_asset = h.asset_at(payout.asset).clone();
    let category = ALL[(payout.category % 8) as usize];
    let bucket = client.balances(&paid_asset).get_unchecked(category as u32);
    let to = if payout.unapproved {
        &h.stranger
    } else {
        &h.recipient
    };
    let token = token::Client::new(&h.env, &paid_asset);
    let paid_before = token.balance(to);
    let held_before = h.held_in(&paid_asset);

    let (mut recipient, mut asset, mut amount, mut signed_category) =
        (to.clone(), paid_asset.clone(), payout.amount, category);
    let tampered = match payout.tamper % 8 {
        1 => {
            recipient = Address::generate(&h.env);
            true
        }
        2 => {
            asset = Address::generate(&h.env);
            true
        }
        3 => {
            amount = payout.amount.wrapping_add(1);
            true
        }
        4 => {
            signed_category = ALL[((payout.category % 8 + 1) % 8) as usize];
            true
        }
        5 => {
            // The approval names the other approved asset.
            asset = h.asset_at(payout.asset.wrapping_add(1)).clone();
            true
        }
        _ => false,
    };
    let approval = payout
        .approved
        .then_some((recipient, asset, amount, signed_category));

    let result = h.try_distribute(to, &paid_asset, category, payout.amount, approval);

    let should_pay = !tampered
        && !payout.unapproved
        && !removed
        && payout.approved
        && payout.amount > 0
        && payout.amount <= bucket;
    assert_eq!(result.is_ok(), should_pay, "{payout:?}");

    let paid = token.balance(to) - paid_before;
    assert_eq!(paid, if should_pay { payout.amount } else { 0 });
    if !should_pay {
        assert_eq!(
            h.held_in(&paid_asset),
            held_before,
            "a refused payout moved funds"
        );
    }
}
