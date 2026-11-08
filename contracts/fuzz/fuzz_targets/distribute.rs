// SPDX-License-Identifier: Apache-2.0

//! Fuzzes `distribute` with arbitrary payouts, approvals and tampering.
//! Checks that a payout is made only when the approval account authorised exactly the
//! submitted arguments and its bucket covers it, and that the buckets never exceed the balance.
//! The account's own 2-of-3 threshold can only be enforced by Stellar and is not modelled here.

#![no_main]

use cipherphi_fuzz::{Harness, ALL};
use libfuzzer_sys::{arbitrary, fuzz_target};
use soroban_sdk::{testutils::Address as _, token, Address};

#[derive(arbitrary::Arbitrary, Debug)]
struct Payout {
    category: u8,
    amount: i128,
    approved: bool,
    /// The argument that the signers approved differently, if any.
    tamper: u8,
}

#[derive(arbitrary::Arbitrary, Debug)]
struct Input {
    gifts: Vec<u32>,
    payouts: Vec<Payout>,
}

fuzz_target!(|input: Input| {
    let h = Harness::new([3000, 2000, 1000, 500, 500, 1500, 1000, 500]);
    let client = h.client();

    for gift in input.gifts.iter().take(4) {
        if *gift > 0 {
            client.contribute(&h.donor, &h.asset, &(*gift as i128));
        }
    }

    for payout in input.payouts.iter().take(8) {
        let category = ALL[(payout.category % 8) as usize];
        let bucket = client.balances(&h.asset).get_unchecked(category as u32);
        let paid_before = token::Client::new(&h.env, &h.asset).balance(&h.recipient);

        let (mut recipient, mut asset, mut amount, mut signed_category) =
            (h.recipient.clone(), h.asset.clone(), payout.amount, category);
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
            _ => false,
        };
        let approval = payout
            .approved
            .then(|| (recipient, asset, amount, signed_category));

        let result = h.try_distribute(category, payout.amount, approval);

        let should_pay =
            !tampered && payout.approved && payout.amount > 0 && payout.amount <= bucket;
        assert_eq!(result.is_ok(), should_pay, "{payout:?}");

        let paid = token::Client::new(&h.env, &h.asset).balance(&h.recipient) - paid_before;
        assert_eq!(paid, if should_pay { payout.amount } else { 0 });
        h.assert_invariant();
    }
});
