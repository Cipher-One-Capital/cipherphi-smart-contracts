// SPDX-License-Identifier: Apache-2.0
#![cfg(test)]

extern crate std;

use soroban_sdk::{
    testutils::{Address as _, AuthorizedFunction, MockAuth, MockAuthInvoke},
    vec,
    xdr::ToXdr,
    Address, Env, IntoVal, Symbol, Val, Vec,
};

use crate::{
    Allocation, Category, DischargeTiming, Error, NisabBasis, Parameters, Policy, PolicyArgs,
    PolicyClient, MAX_AMIL_CAP_BPS, MAX_RATE_BPS,
};

const PLACEHOLDER: [u32; 8] = [3000, 2000, 1000, 500, 500, 1500, 1000, 500];

fn allocation(env: &Env, split: &[u32], remainder: Category) -> Allocation {
    Allocation {
        split_bps: Vec::from_slice(env, split),
        remainder,
    }
}

fn parameters() -> Parameters {
    Parameters {
        nisab_basis: NisabBasis::Gold,
        rate_bps: 250,
        amil_cap_bps: 1250,
        discharge_timing: DischargeTiming::Received,
    }
}

fn setup() -> (Env, Address, Address) {
    let env = Env::default();
    let controller = Address::generate(&env);
    let id = env.register(
        Policy,
        PolicyArgs::__constructor(
            &controller,
            &allocation(&env, &PLACEHOLDER, Category::Poor),
            &parameters(),
        ),
    );
    (env, id, controller)
}

/// Returns the refusal from the panic message when `register` panics on a constructor error.
/// The refusal can only be read from the panic message, not from a typed result.
fn refusal_of(register: impl FnOnce()) -> std::string::String {
    let payload = std::panic::catch_unwind(std::panic::AssertUnwindSafe(register))
        .expect_err("the constructor accepted arguments it should have refused");
    payload
        .downcast_ref::<std::string::String>()
        .cloned()
        .or_else(|| {
            payload
                .downcast_ref::<&str>()
                .map(|text| std::string::String::from(*text))
        })
        .unwrap_or_default()
}

fn assert_refused(message: &str, code: u32) {
    let want = std::format!("Error(Contract, #{})", code);
    assert!(message.contains(&want), "wanted {} in: {}", want, message);
}

fn refused_at_construction(split: &[u32], remainder: Category, p: Parameters, want: Error) {
    let env = Env::default();
    let controller = Address::generate(&env);
    let value = allocation(&env, split, remainder);
    let message = refusal_of(|| {
        env.register(Policy, PolicyArgs::__constructor(&controller, &value, &p));
    });
    assert_refused(&message, want as u32);
}

#[test]
fn constructor_stores_the_placeholders() {
    let (env, id, controller) = setup();
    let client = PolicyClient::new(&env, &id);
    assert_eq!(client.controller(), controller);
    assert_eq!(
        client.allocation(),
        allocation(&env, &PLACEHOLDER, Category::Poor)
    );
    assert_eq!(client.parameters(), parameters());
}

#[test]
fn allocate_follows_the_placeholder_split() {
    let (env, id, _) = setup();
    assert_eq!(
        PolicyClient::new(&env, &id).allocate(&1_000_000),
        vec![&env, 300_000, 200_000, 100_000, 50_000, 50_000, 150_000, 100_000, 50_000]
    );
}

#[test]
fn the_rounding_remainder_goes_to_the_configured_category() {
    let (env, id, _) = setup();
    // A 1-stroop amount can only be credited to the remainder, since every floor is zero.
    assert_eq!(
        PolicyClient::new(&env, &id).allocate(&1),
        vec![&env, 1, 0, 0, 0, 0, 0, 0, 0]
    );
}

#[test]
fn the_remainder_category_is_a_setting() {
    let env = Env::default();
    let id = env.register(
        Policy,
        PolicyArgs::__constructor(
            &Address::generate(&env),
            &allocation(&env, &PLACEHOLDER, Category::Wayfarers),
            &parameters(),
        ),
    );
    assert_eq!(
        PolicyClient::new(&env, &id).allocate(&1),
        vec![&env, 0, 0, 0, 0, 0, 0, 0, 1]
    );
}

#[test]
fn split_sums_to_the_input() {
    let (env, id, _) = setup();
    let client = PolicyClient::new(&env, &id);
    for amount in [
        1i128,
        3,
        7,
        9_999,
        10_000,
        10_001,
        123_456_789,
        i128::MAX / 10_000,
    ] {
        let parts = client.allocate(&amount);
        assert_eq!(parts.len(), 8);
        assert_eq!(parts.iter().sum::<i128>(), amount);
        assert!(parts.iter().all(|part| part >= 0));
    }
}

#[test]
fn zero_and_negative_amounts_are_refused() {
    let (env, id, _) = setup();
    let client = PolicyClient::new(&env, &id);
    assert_eq!(client.try_allocate(&0), Err(Ok(Error::InvalidAmount)));
    assert_eq!(client.try_allocate(&-1), Err(Ok(Error::InvalidAmount)));
}

#[test]
fn an_overflowing_amount_is_refused() {
    let (env, id, _) = setup();
    assert_eq!(
        PolicyClient::new(&env, &id).try_allocate(&i128::MAX),
        Err(Ok(Error::Overflow))
    );
}

#[test]
fn a_split_of_seven_or_nine_is_refused() {
    refused_at_construction(
        &[3000, 2000, 1000, 500, 500, 1500, 1500],
        Category::Poor,
        parameters(),
        Error::SplitLength,
    );
    refused_at_construction(
        &[3000, 2000, 1000, 500, 500, 1500, 1000, 400, 100],
        Category::Poor,
        parameters(),
        Error::SplitLength,
    );
}

#[test]
fn a_split_that_does_not_sum_to_unity_is_refused() {
    refused_at_construction(
        &[3000, 2000, 1000, 500, 500, 1500, 1000, 499],
        Category::Poor,
        parameters(),
        Error::SplitSum,
    );
    refused_at_construction(
        &[3000, 2000, 1000, 500, 500, 1500, 1000, 501],
        Category::Poor,
        parameters(),
        Error::SplitSum,
    );
}

#[test]
fn a_split_that_overflows_u32_is_refused() {
    refused_at_construction(
        &[u32::MAX, 1, 0, 0, 0, 0, 0, 10_000],
        Category::Poor,
        parameters(),
        Error::SplitSum,
    );
}

#[test]
fn a_remainder_with_no_share_is_refused() {
    refused_at_construction(
        &[5000, 2000, 1000, 500, 500, 1000, 0, 0],
        Category::Wayfarers,
        parameters(),
        Error::RemainderWithoutShare,
    );
}

#[test]
fn the_administrators_cannot_take_the_remainder() {
    // The remainder cannot be assigned to Administrators, even when their basis points sit
    // under the cap. This refuses the pairing to keep the remainder from carrying their
    // credit past the cap by a few units.
    refused_at_construction(
        &PLACEHOLDER,
        Category::Administrators,
        parameters(),
        Error::RemainderToAdministrators,
    );
}

#[test]
fn an_amil_cap_above_its_band_is_refused() {
    let mut p = parameters();
    p.amil_cap_bps = MAX_AMIL_CAP_BPS + 1;
    refused_at_construction(&PLACEHOLDER, Category::Poor, p, Error::AmilCapOutOfBand);
}

#[test]
fn a_rate_outside_its_band_is_refused() {
    let mut p = parameters();
    p.rate_bps = 0;
    refused_at_construction(
        &PLACEHOLDER,
        Category::Poor,
        p.clone(),
        Error::RateOutOfBand,
    );
    p.rate_bps = MAX_RATE_BPS + 1;
    refused_at_construction(&PLACEHOLDER, Category::Poor, p, Error::RateOutOfBand);
}

/// Checks that a value exactly at its limit is accepted, since every bound is inclusive.
#[test]
fn values_exactly_at_their_bounds_are_accepted() {
    let env = Env::default();
    let controller = Address::generate(&env);
    let split = allocation(&env, &PLACEHOLDER, Category::Poor);
    let administrators = PLACEHOLDER[Category::Administrators as usize];

    let mut at_band = parameters();
    at_band.amil_cap_bps = MAX_AMIL_CAP_BPS;
    at_band.rate_bps = MAX_RATE_BPS;
    let mut at_share = parameters();
    at_share.amil_cap_bps = administrators;

    for p in [at_band, at_share] {
        let id = env.register(Policy, PolicyArgs::__constructor(&controller, &split, &p));
        assert_eq!(PolicyClient::new(&env, &id).parameters(), p);
    }
}

#[test]
fn an_administrators_share_above_the_cap_is_refused() {
    let mut p = parameters();
    p.amil_cap_bps = 999;
    refused_at_construction(
        &PLACEHOLDER,
        Category::Poor,
        p,
        Error::AdministratorsAboveCap,
    );
}

/// Asserts that `controller` alone authorised the most recent call, and that the
/// call was `fn_name` on `id` with exactly `args`.
///
/// A role cannot be proven by `mock_all_auths` alone. This reads the recorded
/// authorisation to catch a setter that checks the wrong address.
fn assert_authorised_by(
    env: &Env,
    id: &Address,
    controller: &Address,
    fn_name: &str,
    args: Vec<Val>,
) {
    let auths = env.auths();
    assert_eq!(auths.len(), 1, "wanted one authorisation for {fn_name}");
    let (who, invocation) = auths[0].clone();
    assert_eq!(&who, controller);
    assert_eq!(
        invocation.function,
        AuthorizedFunction::Contract((id.clone(), Symbol::new(env, fn_name), args))
    );
}

#[test]
fn the_controller_can_set_valid_values() {
    let (env, id, controller) = setup();
    env.mock_all_auths();
    let client = PolicyClient::new(&env, &id);

    let next = allocation(
        &env,
        &[2500, 2500, 1000, 500, 500, 1500, 1000, 500],
        Category::Needy,
    );
    client.set_allocation(&next);
    assert_authorised_by(
        &env,
        &id,
        &controller,
        "set_allocation",
        (next.clone(),).into_val(&env),
    );
    assert_eq!(client.allocation(), next);

    let mut p = parameters();
    p.nisab_basis = NisabBasis::Silver;
    p.discharge_timing = DischargeTiming::Delivered;
    client.set_parameters(&p);
    assert_authorised_by(
        &env,
        &id,
        &controller,
        "set_parameters",
        (p.clone(),).into_val(&env),
    );
    assert_eq!(client.parameters(), p);
}

#[test]
fn the_setters_apply_the_same_bounds() {
    let (env, id, _) = setup();
    env.mock_all_auths();
    let client = PolicyClient::new(&env, &id);
    let before = client.allocation();

    assert_eq!(
        client.try_set_allocation(&allocation(&env, &[1; 8], Category::Poor)),
        Err(Ok(Error::SplitSum))
    );
    // A split above the live cap cannot be set, and neither can a cap below the live split.
    assert_eq!(
        client.try_set_allocation(&allocation(
            &env,
            &[2000, 2000, 2000, 500, 500, 1500, 1000, 500],
            Category::Poor
        )),
        Err(Ok(Error::AdministratorsAboveCap))
    );
    let mut p = parameters();
    p.amil_cap_bps = 500;
    assert_eq!(
        client.try_set_parameters(&p),
        Err(Ok(Error::AdministratorsAboveCap))
    );

    assert_eq!(client.allocation(), before);
    assert_eq!(client.parameters(), parameters());
}

#[test]
fn a_caller_other_than_the_controller_cannot_set_anything() {
    let (env, id, _) = setup();
    let client = PolicyClient::new(&env, &id);
    let stranger = Address::generate(&env);
    let next = allocation(
        &env,
        &[2500, 2500, 1000, 500, 500, 1500, 1000, 500],
        Category::Poor,
    );

    env.mock_auths(&[MockAuth {
        address: &stranger,
        invoke: &MockAuthInvoke {
            contract: &id,
            fn_name: "set_allocation",
            args: (next.clone(),).into_val(&env),
            sub_invokes: &[],
        },
    }]);
    assert!(client.try_set_allocation(&next).is_err());

    env.mock_auths(&[MockAuth {
        address: &stranger,
        invoke: &MockAuthInvoke {
            contract: &id,
            fn_name: "set_parameters",
            args: (parameters(),).into_val(&env),
            sub_invokes: &[],
        },
    }]);
    assert!(client.try_set_parameters(&parameters()).is_err());
}

/// Checks the bytes that the deploy script, published before security review (see
/// ARCHITECTURE.md), builds by hand in JavaScript for the constructor arguments when they
/// hold the placeholder values. This pins those bytes to make a drift from what this contract
/// decodes fail here rather than on the network.
#[test]
fn the_deploy_script_encodes_arguments_as_the_contract_decodes_them() {
    fn hex(value: soroban_sdk::Bytes) -> std::string::String {
        value
            .iter()
            .map(|byte| std::format!("{:02x}", byte))
            .collect()
    }
    let env = Env::default();
    assert_eq!(
        hex(allocation(&env, &PLACEHOLDER, Category::Poor).to_xdr(&env)),
        "0000001100000001000000020000000f0000000972656d61696e64657200000000000003000000000000000f0000000973706c69745f6270730000000000001000000001000000080000000300000bb800000003000007d000000003000003e800000003000001f400000003000001f400000003000005dc00000003000003e800000003000001f4"
    );
    assert_eq!(
        hex(parameters().to_xdr(&env)),
        "0000001100000001000000040000000f0000000c616d696c5f6361705f62707300000003000004e20000000f000000106469736368617267655f74696d696e6700000003000000000000000f0000000b6e697361625f62617369730000000003000000000000000f00000008726174655f62707300000003000000fa"
    );
}

#[test]
fn every_error_number_sits_between_30_and_39() {
    // The pool holds every other number. This pins the range to keep a failed
    // transaction naming its contract by number alone.
    let all = [
        Error::SplitLength,
        Error::SplitSum,
        Error::RemainderWithoutShare,
        Error::AmilCapOutOfBand,
        Error::RateOutOfBand,
        Error::AdministratorsAboveCap,
        Error::RemainderToAdministrators,
        Error::Unconfigured,
        Error::InvalidAmount,
        Error::Overflow,
    ];
    for (offset, error) in all.iter().enumerate() {
        assert_eq!(*error as u32, 30 + offset as u32);
    }
}

#[test]
fn a_check_gives_the_setters_answer_and_stores_nothing() {
    let (env, id, _) = setup();
    let client = PolicyClient::new(&env, &id);
    let next = allocation(
        &env,
        &[2500, 2500, 1000, 500, 500, 1500, 1000, 500],
        Category::Poor,
    );
    assert_eq!(client.try_check_allocation(&next), Ok(Ok(())));
    let bad = allocation(&env, &[1, 1, 1, 1, 1, 1, 1, 1], Category::Poor);
    assert_eq!(client.try_check_allocation(&bad), Err(Ok(Error::SplitSum)));

    let mut low_cap = parameters();
    low_cap.amil_cap_bps = 500;
    assert_eq!(
        client.try_check_parameters(&low_cap),
        Err(Ok(Error::AdministratorsAboveCap))
    );
    let mut silver = parameters();
    silver.nisab_basis = NisabBasis::Silver;
    assert_eq!(client.try_check_parameters(&silver), Ok(Ok(())));

    // A check needs no authorisation and leaves both halves as they were.
    assert!(env.auths().is_empty());
    assert_eq!(
        client.allocation(),
        allocation(&env, &PLACEHOLDER, Category::Poor)
    );
    assert_eq!(client.parameters(), parameters());
}
