// SPDX-License-Identifier: Apache-2.0

//! Fuzzes `contribute` with arbitrary amounts and arbitrary valid splits.
//! Checks that a success credits exactly the amount, that a refusal changes nothing, and
//! that the buckets never exceed the balance.

#![no_main]

use cipherphi_fuzz::Harness;
use libfuzzer_sys::{arbitrary, fuzz_target};

#[derive(arbitrary::Arbitrary, Debug)]
struct Input {
    /// The shares of a split that is normalised below to sum to 10,000.
    shares: [u8; 8],
    amounts: Vec<i128>,
    surplus: u32,
}

/// Turns arbitrary shares into a split the policy accepts, where Administrators stays
/// under the cap and Poor (the remainder) keeps a share and takes what is left.
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
    let h = Harness::new(split_from(input.shares));
    let client = h.client();
    let mut credited = 0i128;

    for amount in input.amounts.iter().take(16) {
        let (claimed, held) = (h.claimed(), h.held());
        let result = client.try_contribute(&h.donor, &h.asset, amount);
        if result.is_ok() {
            assert!(*amount > 0);
            credited += *amount;
            assert_eq!(h.claimed(), claimed + *amount);
            assert_eq!(h.held(), held + *amount);
        } else {
            assert_eq!(h.claimed(), claimed);
            assert_eq!(h.held(), held);
        }
        h.assert_invariant();
    }
    assert_eq!(h.claimed(), credited);

    // A surplus is a token amount that reaches the pool without `contribute`.
    // A surplus can only move the balance, not a bucket.
    // This mints the surplus to avoid relying on a donor that the gifts may have spent.
    if input.surplus > 0 {
        let held = h.held();
        soroban_sdk::token::StellarAssetClient::new(&h.env, &h.asset)
            .mint(&h.pool, &(input.surplus as i128));
        assert_eq!(h.held(), held + input.surplus as i128);
        assert_eq!(h.claimed(), credited);
        h.assert_invariant();
    }
});
