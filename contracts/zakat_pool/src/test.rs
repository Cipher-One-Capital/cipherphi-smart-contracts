// SPDX-License-Identifier: Apache-2.0

//! The tests that cover the happy paths, the authorisation assertions and the
//! refusal matrix, ported from the demo and extended for assets, governance and
//! the untrusted policy.
//!
//! `policy` can only be replaced by a mock at the pinned address when a test is
//! about it misbehaving. The 2-of-3 threshold of the approval multisig can only
//! be enforced by Stellar. The approval multisig can only be modelled here as
//! whether it authorised a payout, and over which arguments.
//! `env.events().all()` and `env.auths()` can only be read for the most recent
//! invocation. This places each such assertion directly after its call to read
//! the invocation it is about. Every refusal must be checked against the exact
//! error and an unmoved pool.

#![cfg(test)]

extern crate std;

use soroban_sdk::{
    contract, contractimpl, contracttype,
    testutils::{Address as _, AuthorizedFunction, Events, Ledger, MockAuth, MockAuthInvoke},
    token, vec, Address, Env, Event, IntoVal, Symbol, Vec,
};

use crate::*;
use ::policy::{
    Allocation, DischargeTiming, NisabBasis, Parameters, Policy, PolicyArgs, PolicyClient,
};

const PLACEHOLDER: [u32; 8] = [3000, 2000, 1000, 500, 500, 1500, 1000, 500];
const CONTRIBUTION: i128 = 1_000_000;
const START_LEDGER: u32 = 100;
const TIMELOCK: u32 = MIN_TIMELOCK_LEDGERS;

/// The state that a refusal has to leave exactly as it found it.
#[derive(Clone, Debug, Eq, PartialEq)]
struct State {
    buckets: Vec<i128>,
    recipient_balance: i128,
    donor_balance: i128,
    pool_balance: i128,
    pending: Option<PendingChange>,
}

struct Fixture {
    env: Env,
    pool: Address,
    policy: Address,
    asset: Address,
    donor: Address,
    recipient: Address,
    governance: Address,
    authority: Address,
}

fn allocation(env: &Env, split: &[u32]) -> Allocation {
    Allocation {
        split_bps: Vec::from_slice(env, split),
        remainder: Category::Poor,
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

/// Deploys the pool and resumes contributions, as when the deployment preconditions pass.
fn setup() -> Fixture {
    let f = setup_paused();
    f.client().resume_contributions();
    f
}

/// Deploys the pool and leaves contributions closed.
fn setup_paused() -> Fixture {
    let env = Env::default();
    env.mock_all_auths();
    env.ledger().set_sequence_number(START_LEDGER);
    // This raises the TTL limits to let the timelock tests run far past the defaults.
    env.ledger().with_mut(|l| {
        l.min_persistent_entry_ttl = 2_000_000;
        l.min_temp_entry_ttl = 2_000_000;
        l.max_entry_ttl = 6_000_000;
    });

    let governance = Address::generate(&env);
    let donor = Address::generate(&env);
    let recipient = Address::generate(&env);

    let asset = env
        .register_stellar_asset_contract_v2(Address::generate(&env))
        .address();
    token::StellarAssetClient::new(&env, &asset).mint(&donor, &(CONTRIBUTION * 10));

    // An address that stands in for the approval multisig. Its 2-of-3 threshold
    // cannot be modelled here and can only be enforced by Stellar. It can only be
    // checked via the deploy script, which asserts it from the ledger, and via the
    // rehearsal and testnet evidence of a 1-signature payout failing.
    let authority = Address::generate(&env);

    // The pool address must be settled first, as the deploy script does with a
    // salt. This fixes it before registration to give policy and pool each
    // other's address.
    let pool = Address::generate(&env);
    let policy = env.register(
        Policy,
        PolicyArgs::__constructor(&pool, &allocation(&env, &PLACEHOLDER), &parameters()),
    );
    env.register_at(
        &pool,
        ZakatPool,
        ZakatPoolArgs::__constructor(
            &governance,
            &authority,
            &policy,
            &asset,
            &vec![&env, recipient.clone()],
            &TIMELOCK,
        ),
    );

    Fixture {
        env,
        pool,
        policy,
        asset,
        donor,
        recipient,
        governance,
        authority,
    }
}

impl Fixture {
    fn client(&self) -> ZakatPoolClient<'_> {
        ZakatPoolClient::new(&self.env, &self.pool)
    }

    fn token(&self) -> token::Client<'_> {
        token::Client::new(&self.env, &self.asset)
    }

    fn state(&self) -> State {
        let pool = self.client();
        State {
            buckets: pool.balances(&self.asset),
            recipient_balance: self.token().balance(&self.recipient),
            donor_balance: self.token().balance(&self.donor),
            pool_balance: self.token().balance(&self.pool),
            pending: pool.pending_change(),
        }
    }

    fn give(&self) {
        self.client()
            .contribute(&self.donor, &self.asset, &CONTRIBUTION);
    }

    fn pay(&self, category: Category, amount: i128) {
        self.client()
            .distribute(&self.recipient, &self.asset, &amount, &category);
    }

    /// Attempts a payout and asserts the error and an unmoved pool when a refusal
    /// test expects `want`.
    fn refuse(
        &self,
        recipient: &Address,
        asset: &Address,
        amount: i128,
        category: Category,
        want: Error,
    ) {
        let before = self.state();
        assert_eq!(
            self.client()
                .try_distribute(recipient, asset, &amount, &category),
            Err(Ok(want))
        );
        assert_eq!(self.state(), before, "a refused payout moved the pool");
    }

    /// Submits one payout when `signer` authorised `approved`, which may be a
    /// different payout, and returns whether it went through. The pool cannot be
    /// moved by a refusal.
    fn submit_with_approval(
        &self,
        signer: &Address,
        submitted: (Address, Address, i128, Category),
        approved: (Address, Address, i128, Category),
    ) -> bool {
        let before = self.state();
        self.env.mock_auths(&[MockAuth {
            address: signer,
            invoke: &MockAuthInvoke {
                contract: &self.pool,
                fn_name: "distribute",
                args: approved.into_val(&self.env),
                sub_invokes: &[],
            },
        }]);
        let (recipient, asset, amount, category) = submitted;
        let ok = self
            .client()
            .try_distribute(&recipient, &asset, &amount, &category)
            .is_ok();
        self.env.mock_all_auths();
        if !ok {
            assert_eq!(
                self.state(),
                before,
                "an unauthorised payout moved the pool"
            );
        }
        ok
    }

    fn payout(&self, amount: i128) -> (Address, Address, i128, Category) {
        (
            self.recipient.clone(),
            self.asset.clone(),
            amount,
            Category::Poor,
        )
    }

    fn past_timelock(&self, execute_after: u32) {
        self.env.ledger().set_sequence_number(execute_after + 1);
    }
}

// --- construction -------------------------------------------------------------

#[test]
fn constructor_stores_the_configuration() {
    let f = setup_paused();
    let pool = f.client();
    assert_eq!(pool.governance(), f.governance);
    assert_eq!(pool.authority(), f.authority);
    assert_eq!(pool.policy(), f.policy);
    assert_eq!(pool.approved_assets(), vec![&f.env, f.asset.clone()]);
    assert_eq!(pool.timelock_ledgers(), TIMELOCK);
    assert!(pool.is_paused());
    assert_eq!(pool.pending_change(), None);
    assert!(pool.is_approved_recipient(&f.recipient));
    assert_eq!(
        pool.balances(&f.asset),
        vec![&f.env, 0, 0, 0, 0, 0, 0, 0, 0]
    );
}

/// Returns the panic message when `register` panics on a constructor error.
/// This reads the refusal from the panic message to work without a typed result.
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

fn refuse_construction(
    env: &Env,
    governance: &Address,
    authority: &Address,
    timelock: u32,
    want: Error,
) {
    let (policy, asset) = (Address::generate(env), Address::generate(env));
    let message = refusal_of(|| {
        env.register(
            ZakatPool,
            ZakatPoolArgs::__constructor(
                governance,
                authority,
                &policy,
                &asset,
                &Vec::new(env),
                &timelock,
            ),
        );
    });
    assert_refused(&message, want as u32);
}

#[test]
fn a_timelock_under_seven_days_is_refused_at_construction() {
    let env = Env::default();
    env.mock_all_auths();
    let (g, a) = (Address::generate(&env), Address::generate(&env));
    refuse_construction(
        &env,
        &g,
        &a,
        MIN_TIMELOCK_LEDGERS - 1,
        Error::TimelockTooShort,
    );
}

#[test]
fn governance_cannot_also_be_the_approval_multisig() {
    let env = Env::default();
    env.mock_all_auths();
    let g = Address::generate(&env);
    refuse_construction(&env, &g, &g, TIMELOCK, Error::RolesOverlap);
}

#[test]
fn the_pool_and_the_policy_cannot_be_the_approval_multisig_at_construction() {
    let env = Env::default();
    env.mock_all_auths();
    let pick: [Pick; 2] = [|r| r.pool.clone(), |r| r.policy.clone()];
    for authority in pick {
        let mut r = roles(&env);
        r.authority = authority(&r);
        let message = refusal_of(|| deploy(&env, &r, &Vec::new(&env)));
        assert_refused(&message, Error::AuthorityCannotSign as u32);
    }
}

#[test]
fn the_pool_is_deployed_closed_to_contributions() {
    let f = setup_paused();
    assert_eq!(
        f.client().try_contribute(&f.donor, &f.asset, &CONTRIBUTION),
        Err(Ok(Error::ContributionsPaused))
    );
    f.client().resume_contributions();
    f.give();
}

#[test]
fn a_policy_answering_to_someone_else_is_refused_at_construction() {
    let env = Env::default();
    env.mock_all_auths();
    let (governance, authority) = (Address::generate(&env), Address::generate(&env));
    let elsewhere = Address::generate(&env);
    let policy = env.register(
        Policy,
        PolicyArgs::__constructor(&elsewhere, &allocation(&env, &PLACEHOLDER), &parameters()),
    );
    let asset = Address::generate(&env);
    let message = refusal_of(|| {
        env.register(
            ZakatPool,
            ZakatPoolArgs::__constructor(
                &governance,
                &authority,
                &policy,
                &asset,
                &Vec::<Address>::new(&env),
                &TIMELOCK,
            ),
        );
    });
    assert_refused(&message, Error::PolicyControllerMismatch as u32);
}

#[test]
fn the_constructor_requires_governance_to_authorise() {
    let f = setup_paused();
    // The auths must be read before any other call, a view included. This reads
    // them first to catch `setup()`'s registration of the pool as the most recent
    // invocation.
    let (who, invocation) = f.env.auths().first().cloned().unwrap();
    assert_eq!(who, f.governance);
    assert_eq!(
        invocation.function,
        AuthorizedFunction::Contract((
            f.pool.clone(),
            Symbol::new(&f.env, "__constructor"),
            (
                f.governance.clone(),
                f.authority.clone(),
                f.policy.clone(),
                f.asset.clone(),
                vec![&f.env, f.recipient.clone()],
                TIMELOCK,
            )
                .into_val(&f.env),
        ))
    );
}

// --- contribute ---------------------------------------------------------------

#[test]
fn contribute_credits_every_bucket_and_emits_the_split() {
    let f = setup();
    f.give();
    let split = vec![
        &f.env,
        300_000i128,
        200_000,
        100_000,
        50_000,
        50_000,
        150_000,
        100_000,
        50_000,
    ];
    assert_eq!(
        f.env.events().all().filter_by_contract(&f.pool),
        std::vec![Contribution {
            donor: f.donor.clone(),
            asset: f.asset.clone(),
            amount: CONTRIBUTION,
            split: split.clone(),
        }
        .to_xdr(&f.env, &f.pool)]
    );
    assert_eq!(f.client().balances(&f.asset), split);
    assert_eq!(f.token().balance(&f.pool), CONTRIBUTION);
}

#[test]
fn a_second_contribution_accumulates() {
    let f = setup();
    f.give();
    f.give();
    assert_eq!(f.client().balances(&f.asset).get_unchecked(0), 600_000);
    assert_eq!(f.token().balance(&f.pool), CONTRIBUTION * 2);
}

#[test]
fn contribute_requires_the_donor_to_authorise() {
    let f = setup();
    f.give();
    let (who, invocation) = f.env.auths().first().cloned().unwrap();
    assert_eq!(who, f.donor);
    assert_eq!(
        invocation.function,
        AuthorizedFunction::Contract((
            f.pool.clone(),
            Symbol::new(&f.env, "contribute"),
            (f.donor.clone(), f.asset.clone(), CONTRIBUTION).into_val(&f.env),
        ))
    );
}

#[test]
fn a_contribution_without_the_donors_authorisation_fails() {
    let f = setup();
    let before = f.state();
    f.env.mock_auths(&[]);
    assert!(f
        .client()
        .try_contribute(&f.donor, &f.asset, &CONTRIBUTION)
        .is_err());
    f.env.mock_all_auths();
    assert_eq!(f.state(), before);
}

#[test]
fn zero_and_negative_contributions_are_refused() {
    let f = setup();
    let before = f.state();
    for amount in [0i128, -1, i128::MIN] {
        assert_eq!(
            f.client().try_contribute(&f.donor, &f.asset, &amount),
            Err(Ok(Error::InvalidAmount))
        );
    }
    assert_eq!(f.state(), before);
}

#[test]
fn an_asset_that_is_not_approved_is_refused() {
    let f = setup();
    let other = f
        .env
        .register_stellar_asset_contract_v2(Address::generate(&f.env))
        .address();
    token::StellarAssetClient::new(&f.env, &other).mint(&f.donor, &CONTRIBUTION);
    let before = f.state();
    assert_eq!(
        f.client().try_contribute(&f.donor, &other, &CONTRIBUTION),
        Err(Ok(Error::AssetNotApproved))
    );
    assert_eq!(f.state(), before);
    assert_eq!(token::Client::new(&f.env, &other).balance(&f.pool), 0);
}

#[test]
fn a_surplus_sent_straight_to_the_pool_belongs_to_no_bucket() {
    let f = setup();
    f.give();
    f.token().transfer(&f.donor, &f.pool, &777);
    let claimed: i128 = f.client().balances(&f.asset).iter().sum();
    assert_eq!(claimed, CONTRIBUTION);
    assert_eq!(f.token().balance(&f.pool), CONTRIBUTION + 777);
}

// --- policy is untrusted ------------------------------------------------------

#[contracttype]
enum MockKey {
    Answer,
}

#[contract]
struct MockPolicy;

#[contractimpl]
impl MockPolicy {
    pub fn __constructor(env: Env, answer: Vec<i128>) {
        env.storage().instance().set(&MockKey::Answer, &answer);
    }

    pub fn allocate(env: Env, _amount: i128) -> Vec<i128> {
        env.storage().instance().get(&MockKey::Answer).unwrap()
    }
}

/// A mock policy that answers `allocate` with a bare number instead of a vector.
#[contract]
struct WrongTypePolicy;

#[contractimpl]
impl WrongTypePolicy {
    pub fn allocate(_env: Env, _amount: i128) -> u32 {
        8
    }
}

/// Repoints the pool directly, standing in for an executed change. This skips
/// the timelock to keep these tests about the shape check.
fn point_at_policy(f: &Fixture, policy: &Address) {
    f.env.as_contract(&f.pool, || {
        f.env.storage().instance().set(&DataKey::PolicyAddr, policy);
    });
}

fn refuse_policy_answer(answer: &[i128]) {
    let f = setup();
    let mock = f
        .env
        .register(MockPolicy, (Vec::<i128>::from_slice(&f.env, answer),));
    point_at_policy(&f, &mock);
    let before = f.state();
    assert_eq!(
        f.client().try_contribute(&f.donor, &f.asset, &100),
        Err(Ok(Error::InvalidAllocation))
    );
    assert_eq!(f.state(), before);
}

#[test]
fn an_allocation_of_seven_elements_is_rejected() {
    refuse_policy_answer(&[50, 10, 10, 10, 10, 5, 5]);
}

#[test]
fn an_allocation_of_nine_elements_is_rejected() {
    refuse_policy_answer(&[50, 10, 10, 10, 10, 5, 5, 0, 0]);
}

#[test]
fn an_allocation_with_a_negative_element_is_rejected() {
    // This sums correctly to 100 to show the case a sum-only check would admit.
    refuse_policy_answer(&[150, -50, 0, 0, 0, 0, 0, 0]);
}

#[test]
fn an_allocation_with_a_short_sum_is_rejected() {
    refuse_policy_answer(&[50, 10, 10, 10, 10, 5, 4, 0]);
}

#[test]
fn an_allocation_with_a_long_sum_is_rejected() {
    refuse_policy_answer(&[50, 10, 10, 10, 10, 5, 6, 0]);
}

#[test]
fn an_allocation_that_overflows_is_rejected() {
    refuse_policy_answer(&[i128::MAX, i128::MAX, 0, 0, 0, 0, 0, 0]);
}

#[test]
fn a_policy_that_returns_the_wrong_type_is_rejected() {
    let f = setup();
    let mock = f.env.register(WrongTypePolicy, ());
    point_at_policy(&f, &mock);
    assert_eq!(
        f.client().try_contribute(&f.donor, &f.asset, &100),
        Err(Ok(Error::InvalidAllocation))
    );
}

#[test]
fn a_well_formed_mock_allocation_is_accepted() {
    let f = setup();
    let answer = [100i128, 0, 0, 0, 0, 0, 0, 0];
    let mock = f
        .env
        .register(MockPolicy, (Vec::<i128>::from_slice(&f.env, &answer),));
    point_at_policy(&f, &mock);
    f.client().contribute(&f.donor, &f.asset, &100);
    assert_eq!(
        f.client().balances(&f.asset),
        Vec::<i128>::from_slice(&f.env, &answer)
    );
}

// --- distribute ---------------------------------------------------------------

#[test]
fn distribute_pays_out_and_emits_the_event() {
    let f = setup();
    f.give();
    f.pay(Category::Poor, 250_000);
    assert_eq!(
        f.env.events().all().filter_by_contract(&f.pool),
        std::vec![Disbursement {
            recipient: f.recipient.clone(),
            category: Category::Poor,
            asset: f.asset.clone(),
            amount: 250_000,
        }
        .to_xdr(&f.env, &f.pool)]
    );
    assert_eq!(f.token().balance(&f.recipient), 250_000);
    assert_eq!(f.client().balances(&f.asset).get_unchecked(0), 50_000);
}

#[test]
fn payouts_are_independent_of_each_other() {
    // Payouts cannot be sequenced. Approvals can be run in any order, each on its own.
    let f = setup();
    f.give();
    for _ in 0..3 {
        f.pay(Category::Needy, 10);
    }
    assert_eq!(f.token().balance(&f.recipient), 30);
}

#[test]
fn distribute_requires_the_approval_multisig_over_its_exact_arguments() {
    let f = setup();
    f.give();
    f.pay(Category::Poor, 1);
    // Only one authorisation can be recorded, and it must cover every argument.
    // This binds every argument to make what the signers sign exactly the payout.
    let auths = f.env.auths();
    assert_eq!(auths.len(), 1);
    let (who, invocation) = auths[0].clone();
    assert_eq!(who, f.authority);
    assert_eq!(
        invocation.function,
        AuthorizedFunction::Contract((
            f.pool.clone(),
            Symbol::new(&f.env, "distribute"),
            (f.recipient.clone(), f.asset.clone(), 1i128, Category::Poor).into_val(&f.env),
        ))
    );
}

#[test]
fn the_approved_payout_goes_through() {
    // A control for the refusals below that uses the same mocks, matching exactly.
    let f = setup();
    f.give();
    assert!(f.submit_with_approval(&f.authority, f.payout(10), f.payout(10)));
    assert_eq!(f.token().balance(&f.recipient), 10);
}

#[test]
fn a_payout_nobody_approved_fails() {
    let f = setup();
    f.give();
    let before = f.state();
    f.env.mock_auths(&[]);
    assert!(f
        .client()
        .try_distribute(&f.recipient, &f.asset, &10, &Category::Poor)
        .is_err());
    f.env.mock_all_auths();
    assert_eq!(f.state(), before);
}

#[test]
fn an_approval_by_anyone_else_fails() {
    let f = setup();
    f.give();
    let stranger = Address::generate(&f.env);
    assert!(!f.submit_with_approval(&stranger, f.payout(10), f.payout(10)));
    // A payout cannot be approved by governance either.
    assert!(!f.submit_with_approval(&f.governance, f.payout(10), f.payout(10)));
}

#[test]
fn a_payout_that_differs_from_the_approval_in_any_argument_fails() {
    let f = setup();
    f.give();
    let other = Address::generate(&f.env);
    let approved = f.payout(10);
    let mut altered = std::vec::Vec::new();
    altered.push((other.clone(), f.asset.clone(), 10, Category::Poor));
    altered.push((f.recipient.clone(), other, 10, Category::Poor));
    altered.push((f.recipient.clone(), f.asset.clone(), 11, Category::Poor));
    altered.push((f.recipient.clone(), f.asset.clone(), 10, Category::Needy));
    for submitted in altered {
        assert!(!f.submit_with_approval(&f.authority, submitted, approved.clone()));
    }
}

#[test]
fn a_payout_of_the_whole_bucket_is_allowed() {
    let f = setup();
    f.give();
    let index = Category::Wayfarers as u32;
    let whole = f.client().balances(&f.asset).get_unchecked(index);
    f.pay(Category::Wayfarers, whole);
    assert_eq!(f.client().balances(&f.asset).get_unchecked(index), 0);
    assert_eq!(f.token().balance(&f.recipient), whole);
}

#[test]
fn an_empty_bucket_is_refused() {
    let f = setup();
    f.give();
    f.refuse(
        &f.recipient,
        &f.asset,
        50_001,
        Category::Wayfarers,
        Error::InsufficientBucket,
    );
}

#[test]
fn zero_and_negative_payouts_are_refused() {
    let f = setup();
    f.give();
    for amount in [0i128, -1] {
        f.refuse(
            &f.recipient,
            &f.asset,
            amount,
            Category::Poor,
            Error::InvalidAmount,
        );
    }
}

#[test]
fn a_payout_to_the_pool_itself_is_refused() {
    // The pool cannot be approved as a recipient, so the allow-list refuses it
    // before `SelfDistribution` is reached.
    let f = setup();
    f.give();
    f.refuse(
        &f.pool,
        &f.asset,
        10,
        Category::Poor,
        Error::RecipientNotApproved,
    );
}

#[test]
fn a_payout_in_an_asset_that_is_not_approved_is_refused() {
    let f = setup();
    f.give();
    let other = Address::generate(&f.env);
    f.refuse(
        &f.recipient,
        &other,
        10,
        Category::Poor,
        Error::AssetNotApproved,
    );
}

// --- a token that does not move what it is asked to ----------------------------

#[contracttype]
enum FeeKey {
    Balance(Address),
}

/// A token that keeps 1 unit of every transfer, which no Stellar Asset Contract does.
#[contract]
struct FeeToken;

#[contractimpl]
impl FeeToken {
    pub fn mint(env: Env, to: Address, amount: i128) {
        env.storage().instance().set(&FeeKey::Balance(to), &amount);
    }

    pub fn balance(env: Env, id: Address) -> i128 {
        env.storage()
            .instance()
            .get(&FeeKey::Balance(id))
            .unwrap_or(0)
    }

    pub fn transfer(env: Env, from: Address, to: Address, amount: i128) {
        let storage = env.storage().instance();
        let from_balance: i128 = storage.get(&FeeKey::Balance(from.clone())).unwrap_or(0);
        let to_balance: i128 = storage.get(&FeeKey::Balance(to.clone())).unwrap_or(0);
        storage.set(&FeeKey::Balance(from), &(from_balance - amount));
        storage.set(&FeeKey::Balance(to), &(to_balance + amount - 1));
    }
}

#[test]
fn a_contribution_that_arrives_short_is_refused() {
    let f = setup();
    let fee_token = f.env.register(FeeToken, ());
    FeeTokenClient::new(&f.env, &fee_token).mint(&f.donor, &1_000);
    f.env.as_contract(&f.pool, || {
        f.env
            .storage()
            .instance()
            .set(&DataKey::ApprovedAssets, &vec![&f.env, fee_token.clone()]);
    });
    assert_eq!(
        f.client().try_contribute(&f.donor, &fee_token, &100),
        Err(Ok(Error::TransferShortfall))
    );
}

// --- pause ---------------------------------------------------------------------

#[test]
fn pause_blocks_contribute_at_once_and_does_not_block_distribute() {
    let f = setup();
    f.give();
    f.client().pause_contributions();
    assert_eq!(
        f.env.events().all().filter_by_contract(&f.pool),
        std::vec![ContributionsPaused {}.to_xdr(&f.env, &f.pool)]
    );
    assert!(f.client().is_paused());

    let before = f.state();
    assert_eq!(
        f.client().try_contribute(&f.donor, &f.asset, &CONTRIBUTION),
        Err(Ok(Error::ContributionsPaused))
    );
    assert_eq!(f.state(), before);

    f.pay(Category::Poor, 10);
    assert_eq!(f.token().balance(&f.recipient), 10);
}

#[test]
fn resume_reopens_contributions() {
    let f = setup();
    assert!(!f.client().is_paused());
    f.client().pause_contributions();
    assert!(f.client().is_paused());
    f.client().resume_contributions();
    assert_eq!(
        f.env.events().all().filter_by_contract(&f.pool),
        std::vec![ContributionsResumed {}.to_xdr(&f.env, &f.pool)]
    );
    assert!(!f.client().is_paused());
    f.give();
}

/// Tries a call that one role alone may make when someone else signs it. An unmet
/// `require_auth` is a host error. A typed variant cannot be asserted for it.
fn outsider_cannot(f: &Fixture, fn_name: &str, args: Vec<soroban_sdk::Val>, call: impl FnOnce()) {
    let stranger = Address::generate(&f.env);
    f.env.mock_auths(&[MockAuth {
        address: &stranger,
        invoke: &MockAuthInvoke {
            contract: &f.pool,
            fn_name,
            args,
            sub_invokes: &[],
        },
    }]);
    call();
    f.env.mock_all_auths();
}

/// Asserts that `who` alone authorised the most recent call to the pool, and that
/// the call was `fn_name` with exactly `args`.
///
/// A role cannot be proven by `mock_all_auths` alone. This reads the recorded
/// authorisation to catch an entry point that checks the wrong role.
fn assert_authorised_by(f: &Fixture, who: &Address, fn_name: &str, args: Vec<soroban_sdk::Val>) {
    let auths = f.env.auths();
    assert_eq!(auths.len(), 1, "wanted one authorisation for {fn_name}");
    let (signer, invocation) = auths[0].clone();
    assert_eq!(&signer, who, "{fn_name} was authorised by the wrong role");
    assert_eq!(
        invocation.function,
        AuthorizedFunction::Contract((f.pool.clone(), Symbol::new(&f.env, fn_name), args))
    );
}

#[test]
fn pause_and_resume_are_authorised_by_governance() {
    let f = setup();
    f.client().pause_contributions();
    assert_authorised_by(&f, &f.governance, "pause_contributions", Vec::new(&f.env));
    f.client().resume_contributions();
    assert_authorised_by(&f, &f.governance, "resume_contributions", Vec::new(&f.env));
}

#[test]
fn propose_and_execute_are_authorised_by_governance() {
    let f = setup();
    let change = Change::ApproveRecipient(Address::generate(&f.env));
    let execute_after = f.client().propose_change(&change);
    assert_authorised_by(
        &f,
        &f.governance,
        "propose_change",
        (change.clone(),).into_val(&f.env),
    );
    f.past_timelock(execute_after);
    f.client().execute_change();
    assert_authorised_by(&f, &f.governance, "execute_change", Vec::new(&f.env));
}

#[test]
fn a_cancel_is_authorised_by_the_role_that_makes_it() {
    let f = setup();
    for role in [f.governance.clone(), f.authority.clone()] {
        let change = Change::SetAllocation(next_allocation(&f));
        f.client().propose_change(&change);
        f.client().cancel_change(&role);
        assert_authorised_by(&f, &role, "cancel_change", (role.clone(),).into_val(&f.env));
    }
}

#[test]
fn an_instant_removal_is_authorised_by_the_approval_multisig() {
    let f = setup();
    f.client().remove_recipient(&f.authority, &f.recipient);
    assert_authorised_by(
        &f,
        &f.authority,
        "remove_recipient",
        (f.authority.clone(), f.recipient.clone()).into_val(&f.env),
    );
}

#[test]
fn only_governance_can_pause_and_resume() {
    let f = setup();
    let empty = Vec::new(&f.env);
    outsider_cannot(&f, "pause_contributions", empty.clone(), || {
        assert!(f.client().try_pause_contributions().is_err());
    });
    f.client().pause_contributions();
    outsider_cannot(&f, "resume_contributions", empty, || {
        assert!(f.client().try_resume_contributions().is_err());
    });
    assert!(f.client().is_paused());
}

// --- the timelock --------------------------------------------------------------

fn next_allocation(f: &Fixture) -> Allocation {
    allocation(&f.env, &[2500, 2500, 1000, 500, 500, 1500, 1000, 500])
}

#[test]
fn a_change_cannot_execute_at_or_before_its_execute_after_ledger() {
    let f = setup();
    let change = Change::SetAllocation(next_allocation(&f));
    let execute_after = f.client().propose_change(&change);
    assert_eq!(execute_after, START_LEDGER + TIMELOCK);

    assert_eq!(
        f.client().try_execute_change(),
        Err(Ok(Error::TimelockNotElapsed))
    );
    f.env.ledger().set_sequence_number(execute_after);
    assert_eq!(
        f.client().try_execute_change(),
        Err(Ok(Error::TimelockNotElapsed))
    );
    assert_eq!(
        PolicyClient::new(&f.env, &f.policy).allocation(),
        allocation(&f.env, &PLACEHOLDER)
    );
    assert!(f.client().pending_change().is_some());
}

#[test]
fn a_change_executes_after_the_delay_and_each_step_emits_its_event() {
    let f = setup();
    let change = Change::SetAllocation(next_allocation(&f));
    let execute_after = f.client().propose_change(&change);
    assert_eq!(
        f.env.events().all().filter_by_contract(&f.pool),
        std::vec![ChangeProposed {
            change: change.clone(),
            execute_after,
            execute_until: execute_after + CHANGE_WINDOW_LEDGERS,
        }
        .to_xdr(&f.env, &f.pool)]
    );

    f.past_timelock(execute_after);
    f.client().execute_change();
    assert_eq!(
        f.env.events().all().filter_by_contract(&f.pool),
        std::vec![ChangeExecuted {
            change,
            execute_after,
        }
        .to_xdr(&f.env, &f.pool)]
    );

    assert_eq!(
        PolicyClient::new(&f.env, &f.policy).allocation(),
        next_allocation(&f)
    );
    assert_eq!(f.client().pending_change(), None);

    // The new split must be used by the next contribution.
    f.give();
    assert_eq!(f.client().balances(&f.asset).get_unchecked(0), 250_000);
}

#[test]
fn governance_can_cancel_a_change_and_the_event_names_it() {
    let f = setup();
    let change = Change::SetAllocation(next_allocation(&f));
    let execute_after = f.client().propose_change(&change);

    f.client().cancel_change(&f.governance);
    assert_eq!(
        f.env.events().all().filter_by_contract(&f.pool),
        std::vec![ChangeCancelled {
            change,
            execute_after,
            by: f.governance.clone(),
        }
        .to_xdr(&f.env, &f.pool)]
    );
    assert_eq!(f.client().pending_change(), None);

    f.past_timelock(execute_after);
    assert_eq!(
        f.client().try_execute_change(),
        Err(Ok(Error::NoPendingChange))
    );
    assert_eq!(
        PolicyClient::new(&f.env, &f.policy).allocation(),
        allocation(&f.env, &PLACEHOLDER)
    );
}

#[test]
fn the_approval_multisig_cancels_a_stolen_governance_quorums_takeover() {
    // Governance keys are stolen, and a new approval account is queued by the
    // thief. It can be cancelled by the signers it would replace, and the funds
    // cannot be moved.
    let f = setup();
    f.give();
    let thief = Address::generate(&f.env);
    let change = Change::SetAuthority(thief);
    let execute_after = f.client().propose_change(&change);

    f.client().cancel_change(&f.authority);
    let (who, _) = f.env.auths().first().cloned().unwrap();
    assert_eq!(who, f.authority);
    assert_eq!(
        f.env.events().all().filter_by_contract(&f.pool),
        std::vec![ChangeCancelled {
            change,
            execute_after,
            by: f.authority.clone(),
        }
        .to_xdr(&f.env, &f.pool)]
    );

    f.past_timelock(execute_after);
    assert_eq!(
        f.client().try_execute_change(),
        Err(Ok(Error::NoPendingChange))
    );
    assert_eq!(f.client().authority(), f.authority);
}

#[test]
fn a_stranger_cannot_cancel() {
    let f = setup();
    let change = Change::SetAllocation(next_allocation(&f));
    f.client().propose_change(&change);
    let stranger = Address::generate(&f.env);
    assert_eq!(
        f.client().try_cancel_change(&stranger),
        Err(Ok(Error::Unauthorised))
    );
    // A cancel cannot be made by naming a role. It can only be made via that
    // role's authorisation.
    outsider_cannot(
        &f,
        "cancel_change",
        (f.authority.clone(),).into_val(&f.env),
        || {
            assert!(f.client().try_cancel_change(&f.authority).is_err());
        },
    );
    assert!(f.client().pending_change().is_some());
}

#[test]
fn a_cancelled_slot_can_be_used_again_with_a_fresh_delay() {
    let f = setup();
    let change = Change::SetAllocation(next_allocation(&f));
    f.client().propose_change(&change);
    f.client().cancel_change(&f.governance);
    f.env.ledger().set_sequence_number(START_LEDGER + 50);
    assert_eq!(
        f.client().propose_change(&change),
        START_LEDGER + 50 + TIMELOCK
    );
}

#[test]
fn only_one_change_can_be_queued() {
    let f = setup();
    let change = Change::SetAllocation(next_allocation(&f));
    f.client().propose_change(&change);
    assert_eq!(
        f.client().try_propose_change(&change),
        Err(Ok(Error::ChangeAlreadyPending))
    );
}

#[test]
fn execute_and_cancel_need_a_queued_change() {
    let f = setup();
    assert_eq!(
        f.client().try_execute_change(),
        Err(Ok(Error::NoPendingChange))
    );
    assert_eq!(
        f.client().try_cancel_change(&f.governance),
        Err(Ok(Error::NoPendingChange))
    );
}

#[test]
fn only_governance_can_propose_and_execute() {
    let f = setup();
    let change = Change::SetAllocation(next_allocation(&f));

    outsider_cannot(
        &f,
        "propose_change",
        (change.clone(),).into_val(&f.env),
        || {
            assert!(f.client().try_propose_change(&change).is_err());
        },
    );
    assert_eq!(f.client().pending_change(), None);

    // A change queued by governance cannot be pushed through by anyone else, the
    // approval multisig included.
    let execute_after = f.client().propose_change(&change);
    f.past_timelock(execute_after);
    outsider_cannot(&f, "execute_change", Vec::new(&f.env), || {
        assert!(f.client().try_execute_change().is_err());
    });
    f.env.mock_auths(&[MockAuth {
        address: &f.authority,
        invoke: &MockAuthInvoke {
            contract: &f.pool,
            fn_name: "execute_change",
            args: Vec::new(&f.env),
            sub_invokes: &[],
        },
    }]);
    assert!(f.client().try_execute_change().is_err());
    f.env.mock_all_auths();
    assert!(f.client().pending_change().is_some());
}

#[test]
fn governance_cannot_reach_around_the_pool_to_the_policy() {
    let f = setup();
    let policy = PolicyClient::new(&f.env, &f.policy);
    let next = next_allocation(&f);
    f.env.mock_auths(&[MockAuth {
        address: &f.governance,
        invoke: &MockAuthInvoke {
            contract: &f.policy,
            fn_name: "set_allocation",
            args: (next.clone(),).into_val(&f.env),
            sub_invokes: &[],
        },
    }]);
    assert!(policy.try_set_allocation(&next).is_err());
    f.env.mock_all_auths();
    assert_eq!(policy.allocation(), allocation(&f.env, &PLACEHOLDER));
}

#[test]
fn a_value_the_policy_refuses_is_refused_at_proposal() {
    let f = setup();
    let bad = allocation(&f.env, &[1, 1, 1, 1, 1, 1, 1, 1]);
    f.refuse_proposal(&Change::SetAllocation(bad), Error::PolicyRefused);
    let mut bad = parameters();
    bad.rate_bps = 0;
    f.refuse_proposal(&Change::SetParameters(bad), Error::PolicyRefused);
    assert_eq!(
        PolicyClient::new(&f.env, &f.policy).allocation(),
        allocation(&f.env, &PLACEHOLDER)
    );
    assert_eq!(
        PolicyClient::new(&f.env, &f.policy).parameters(),
        parameters()
    );
}

#[test]
fn parameters_change_through_the_timelock() {
    let f = setup();
    let mut next = parameters();
    next.nisab_basis = NisabBasis::Silver;
    let execute_after = f
        .client()
        .propose_change(&Change::SetParameters(next.clone()));
    assert_eq!(
        PolicyClient::new(&f.env, &f.policy).parameters(),
        parameters()
    );
    f.past_timelock(execute_after);
    f.client().execute_change();
    assert_eq!(PolicyClient::new(&f.env, &f.policy).parameters(), next);
}

fn policy_answering_to(f: &Fixture, controller: &Address) -> Address {
    f.env.register(
        Policy,
        PolicyArgs::__constructor(controller, &allocation(&f.env, &PLACEHOLDER), &parameters()),
    )
}

#[test]
fn policy_repoints_through_the_timelock() {
    let f = setup();
    let new_policy = policy_answering_to(&f, &f.pool);
    let after = f
        .client()
        .propose_change(&Change::SetPolicy(new_policy.clone()));
    assert_eq!(f.client().policy(), f.policy);
    f.past_timelock(after);
    f.client().execute_change();
    assert_eq!(f.client().policy(), new_policy);
}

#[test]
fn a_policy_answering_to_someone_else_is_refused_at_proposal() {
    let f = setup();
    let rogue = policy_answering_to(&f, &f.governance);
    f.refuse_proposal(&Change::SetPolicy(rogue), Error::PolicyControllerMismatch);
    assert_eq!(f.client().policy(), f.policy);
}

#[test]
fn a_policy_address_with_no_contract_is_refused() {
    let f = setup();
    f.refuse_proposal(
        &Change::SetPolicy(Address::generate(&f.env)),
        Error::PolicyControllerMismatch,
    );
    assert_eq!(f.client().policy(), f.policy);
}

#[test]
fn a_new_approval_multisig_takes_the_delay_and_then_alone_approves() {
    // The approval keys are lost: the account can only be replaced via governance,
    // with nobody left to cancel.
    let f = setup();
    f.give();
    let replacement = Address::generate(&f.env);
    let after = f
        .client()
        .propose_change(&Change::SetAuthority(replacement.clone()));
    assert_eq!(f.client().authority(), f.authority);
    f.past_timelock(after);
    f.client().execute_change();
    assert_eq!(f.client().authority(), replacement);

    assert!(!f.submit_with_approval(&f.authority, f.payout(10), f.payout(10)));
    assert!(f.submit_with_approval(&replacement, f.payout(10), f.payout(10)));
}

#[test]
fn governance_cannot_become_the_approval_multisig() {
    let f = setup();
    f.refuse_proposal(
        &Change::SetAuthority(f.governance.clone()),
        Error::RolesOverlap,
    );
    assert_eq!(f.client().authority(), f.authority);
}

#[test]
fn the_pool_and_the_policy_cannot_become_the_approval_multisig() {
    let f = setup();
    for address in [f.pool.clone(), f.policy.clone()] {
        f.refuse_proposal(&Change::SetAuthority(address), Error::AuthorityCannotSign);
    }
    assert_eq!(f.client().authority(), f.authority);
}

#[test]
fn approving_an_asset_goes_through_the_timelock_and_gives_it_its_own_buckets() {
    let f = setup();
    let second = f
        .env
        .register_stellar_asset_contract_v2(Address::generate(&f.env))
        .address();
    token::StellarAssetClient::new(&f.env, &second).mint(&f.donor, &1_000);
    f.give();

    let after = f
        .client()
        .propose_change(&Change::ApproveAsset(second.clone()));
    assert_eq!(
        f.client().try_contribute(&f.donor, &second, &1_000),
        Err(Ok(Error::AssetNotApproved))
    );
    f.past_timelock(after);
    f.client().execute_change();
    f.client().contribute(&f.donor, &second, &1_000);

    assert_eq!(f.client().balances(&second).get_unchecked(0), 300);
    assert_eq!(f.client().balances(&f.asset).get_unchecked(0), 300_000);
}

#[test]
fn approving_an_approved_asset_is_refused() {
    let f = setup();
    f.refuse_proposal(
        &Change::ApproveAsset(f.asset.clone()),
        Error::AssetAlreadyApproved,
    );
}

#[test]
fn a_queued_change_survives_in_instance_storage() {
    let f = setup();
    let change = Change::SetAllocation(next_allocation(&f));
    let execute_after = f.client().propose_change(&change);
    f.env.as_contract(&f.pool, || {
        assert!(f.env.storage().instance().has(&DataKey::PendingChange));
        assert!(!f.env.storage().temporary().has(&DataKey::PendingChange));
        assert!(f
            .env
            .storage()
            .persistent()
            .has(&DataKey::CategoryBalances(f.asset.clone())));
    });
    assert_eq!(
        f.client().pending_change(),
        Some(PendingChange {
            change,
            execute_after,
            execute_until: execute_after + CHANGE_WINDOW_LEDGERS,
        })
    );
}

// --- the solvency invariant ----------------------------------------------------

struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0
    }
}

fn assert_invariant(f: &Fixture, asset: &Address) {
    let claimed: i128 = f.client().balances(asset).iter().sum();
    let held = token::Client::new(&f.env, asset).balance(&f.pool);
    assert!(
        claimed <= held,
        "buckets {} above balance {}",
        claimed,
        held
    );
    assert!(f.client().balances(asset).iter().all(|bucket| bucket >= 0));
}

/// Runs a seeded mix of gifts, payouts, oversized payouts and direct transfers,
/// and checks after every step that the buckets never exceed the balance.
fn run_invariant(seed: u64, steps: u32) {
    let f = setup();
    f.env.cost_estimate().budget().reset_unlimited();
    token::StellarAssetClient::new(&f.env, &f.asset).mint(&f.donor, &(i64::MAX as i128));
    let mut rng = Rng(seed);
    let mut surplus = 0i128;
    let mut paid = 0i128;
    let mut given = 0i128;

    for _ in 0..steps {
        match rng.next() % 4 {
            0 | 1 => {
                let amount = (rng.next() % 5_000_000) as i128 + 1;
                f.client().contribute(&f.donor, &f.asset, &amount);
                given += amount;
            }
            2 => {
                let index = (rng.next() % 8) as u32;
                let bucket = f.client().balances(&f.asset).get_unchecked(index);
                let amount = (rng.next() as i128) % (bucket * 2 + 1) + 1;
                let category: Category = ALL[index as usize];
                let result = f
                    .client()
                    .try_distribute(&f.recipient, &f.asset, &amount, &category);
                if amount <= bucket {
                    assert!(result.is_ok());
                    paid += amount;
                } else {
                    assert_eq!(result, Err(Ok(Error::InsufficientBucket)));
                }
            }
            _ => {
                let extra = (rng.next() % 1_000) as i128;
                if extra > 0 {
                    f.token().transfer(&f.donor, &f.pool, &extra);
                    surplus += extra;
                }
            }
        }
        assert_invariant(&f, &f.asset);
        let claimed: i128 = f.client().balances(&f.asset).iter().sum();
        assert_eq!(claimed, given - paid);
        assert_eq!(f.token().balance(&f.pool), given - paid + surplus);
    }
}

const ALL: [Category; 8] = [
    Category::Poor,
    Category::Needy,
    Category::Administrators,
    Category::Reconciliation,
    Category::Captives,
    Category::Debtors,
    Category::CauseOfGod,
    Category::Wayfarers,
];

#[test]
fn the_buckets_never_exceed_the_balance_across_seeded_runs() {
    for seed in [1u64, 2, 3, 0xC1F4E7, 0xDEADBEEF] {
        run_invariant(seed, 60);
    }
}

#[test]
fn the_contract_refuses_when_the_balance_falls_under_the_buckets() {
    // This state can only be caused by a token misbehaving outside the pool. It
    // must be caught by the assert inside `contribute` rather than recorded.
    let f = setup();
    f.give();
    f.env.as_contract(&f.pool, || {
        let mut inflated: Vec<i128> = f
            .env
            .storage()
            .persistent()
            .get(&DataKey::CategoryBalances(f.asset.clone()))
            .unwrap();
        inflated.set(0, inflated.get_unchecked(0) + 1);
        f.env
            .storage()
            .persistent()
            .set(&DataKey::CategoryBalances(f.asset.clone()), &inflated);
    });
    assert_eq!(
        f.client().try_contribute(&f.donor, &f.asset, &CONTRIBUTION),
        Err(Ok(Error::Insolvent))
    );
}

#[test]
fn each_assets_solvency_stands_alone() {
    let f = setup();
    let second = f
        .env
        .register_stellar_asset_contract_v2(Address::generate(&f.env))
        .address();
    token::StellarAssetClient::new(&f.env, &second).mint(&f.donor, &1_000);
    let after = f
        .client()
        .propose_change(&Change::ApproveAsset(second.clone()));
    f.past_timelock(after);
    f.client().execute_change();
    f.give();
    f.client().contribute(&f.donor, &second, &1_000);
    assert_invariant(&f, &f.asset);
    assert_invariant(&f, &second);

    // The second asset cannot be spent by an approval for the first.
    let submitted = (f.recipient.clone(), second.clone(), 100, Category::Poor);
    assert!(!f.submit_with_approval(&f.authority, submitted, f.payout(100)));
    assert_eq!(token::Client::new(&f.env, &second).balance(&f.recipient), 0);
}

// --- the recipient allow-list --------------------------------------------------

impl Fixture {
    /// Submits a payout to `recipient` when the approval multisig authorised exactly
    /// it, and asserts the error and an unmoved pool when a refusal test expects
    /// `want`.
    fn refuse_approved(&self, recipient: &Address, want: Error) {
        let before = self.state();
        let paid_before = self.token().balance(recipient);
        self.env.mock_auths(&[MockAuth {
            address: &self.authority,
            invoke: &MockAuthInvoke {
                contract: &self.pool,
                fn_name: "distribute",
                args: (
                    recipient.clone(),
                    self.asset.clone(),
                    10i128,
                    Category::Poor,
                )
                    .into_val(&self.env),
                sub_invokes: &[],
            },
        }]);
        let result = self
            .client()
            .try_distribute(recipient, &self.asset, &10, &Category::Poor);
        self.env.mock_all_auths();
        assert_eq!(result, Err(Ok(want)));
        assert_eq!(self.state(), before, "a refused payout moved the pool");
        assert_eq!(self.token().balance(recipient), paid_before);
    }

    /// Proposes `change` and asserts the error `want`, an empty queue and an unmoved
    /// pool.
    fn refuse_proposal(&self, change: &Change, want: Error) {
        let before = self.state();
        assert_eq!(self.client().try_propose_change(change), Err(Ok(want)));
        assert_eq!(self.state(), before, "a refused proposal moved the pool");
        assert_eq!(self.client().pending_change(), None);
    }

    /// Queues `change`, waits out the delay and returns the result of executing it.
    fn queue_and_execute(&self, change: &Change) -> Result<(), Error> {
        let execute_after = self.client().propose_change(change);
        self.past_timelock(execute_after);
        match self.client().try_execute_change() {
            Ok(_) => Ok(()),
            Err(Ok(error)) => Err(error),
            Err(Err(error)) => panic!("execute_change failed in the host: {error:?}"),
        }
    }
}

/// The addresses that a pool is deployed with, settled before registration.
struct Roles {
    pool: Address,
    governance: Address,
    authority: Address,
    policy: Address,
    asset: Address,
}

fn roles(env: &Env) -> Roles {
    let pool = Address::generate(env);
    let policy = env.register(
        Policy,
        PolicyArgs::__constructor(&pool, &allocation(env, &PLACEHOLDER), &parameters()),
    );
    Roles {
        pool,
        governance: Address::generate(env),
        authority: Address::generate(env),
        policy,
        asset: env
            .register_stellar_asset_contract_v2(Address::generate(env))
            .address(),
    }
}

/// Returns the configuration event that a pool deployed with `roles` emits.
fn configured(env: &Env, roles: &Roles) -> soroban_sdk::xdr::ContractEvent {
    PoolConfigured {
        governance: roles.governance.clone(),
        authority: roles.authority.clone(),
        policy: roles.policy.clone(),
        asset: roles.asset.clone(),
        timelock_ledgers: TIMELOCK,
    }
    .to_xdr(env, &roles.pool)
}

/// Deploys a pool at `roles.pool` that approves `recipients` at construction.
fn deploy(env: &Env, roles: &Roles, recipients: &Vec<Address>) {
    env.register_at(
        &roles.pool,
        ZakatPool,
        ZakatPoolArgs::__constructor(
            &roles.governance,
            &roles.authority,
            &roles.policy,
            &roles.asset,
            recipients,
            &TIMELOCK,
        ),
    );
}

#[test]
fn a_payout_to_an_unapproved_recipient_fails_even_with_a_valid_approval() {
    let f = setup();
    f.give();
    let stranger = Address::generate(&f.env);
    f.refuse_approved(&stranger, Error::RecipientNotApproved);
}

#[test]
fn a_recipient_passed_to_the_constructor_can_be_paid_at_once() {
    let f = setup();
    assert!(f.client().is_approved_recipient(&f.recipient));
    f.give();
    assert_eq!(f.env.ledger().sequence(), START_LEDGER);
    assert!(f.submit_with_approval(&f.authority, f.payout(10), f.payout(10)));
    assert_eq!(f.token().balance(&f.recipient), 10);
}

#[test]
fn an_empty_list_of_recipients_is_accepted_and_approves_no_one() {
    let env = Env::default();
    env.mock_all_auths();
    let r = roles(&env);
    deploy(&env, &r, &Vec::new(&env));
    assert_eq!(
        env.events().all().filter_by_contract(&r.pool),
        std::vec![configured(&env, &r)]
    );
    let pool = ZakatPoolClient::new(&env, &r.pool);
    assert!(!pool.is_approved_recipient(&Address::generate(&env)));
}

#[test]
fn the_constructor_emits_its_configuration_then_one_approval_event_per_recipient() {
    let env = Env::default();
    env.mock_all_auths();
    let r = roles(&env);
    let (first, second) = (Address::generate(&env), Address::generate(&env));
    deploy(&env, &r, &vec![&env, first.clone(), second.clone()]);
    assert_eq!(
        env.events().all().filter_by_contract(&r.pool),
        std::vec![
            configured(&env, &r),
            RecipientApproved {
                recipient: first.clone()
            }
            .to_xdr(&env, &r.pool),
            RecipientApproved {
                recipient: second.clone()
            }
            .to_xdr(&env, &r.pool),
        ]
    );
    let pool = ZakatPoolClient::new(&env, &r.pool);
    assert!(pool.is_approved_recipient(&first));
    assert!(pool.is_approved_recipient(&second));
}

/// A function that picks one of the deployment addresses.
type Pick = fn(&Roles) -> Address;

#[test]
fn the_constructor_refuses_the_pool_the_roles_and_duplicates_as_recipients() {
    let env = Env::default();
    env.mock_all_auths();
    let repeated = Address::generate(&env);
    let cases: [(Pick, Error); 3] = [
        (|r| r.pool.clone(), Error::SelfDistribution),
        (|r| r.governance.clone(), Error::RecipientIsRole),
        (|r| r.authority.clone(), Error::RecipientIsRole),
    ];
    for (pick, want) in cases {
        let r = roles(&env);
        let message = refusal_of(|| deploy(&env, &r, &vec![&env, pick(&r)]));
        assert_refused(&message, want as u32);
    }
    let r = roles(&env);
    let message = refusal_of(|| deploy(&env, &r, &vec![&env, repeated.clone(), repeated]));
    assert_refused(&message, Error::RecipientAlreadyApproved as u32);
}

#[test]
fn approving_a_recipient_takes_effect_only_after_the_timelock() {
    let f = setup();
    f.give();
    let partner = Address::generate(&f.env);
    let change = Change::ApproveRecipient(partner.clone());
    let execute_after = f.client().propose_change(&change);

    f.env.ledger().set_sequence_number(execute_after);
    assert_eq!(
        f.client().try_execute_change(),
        Err(Ok(Error::TimelockNotElapsed))
    );
    assert!(!f.client().is_approved_recipient(&partner));
    f.refuse_approved(&partner, Error::RecipientNotApproved);

    f.past_timelock(execute_after);
    f.client().execute_change();
    assert_eq!(
        f.env.events().all().filter_by_contract(&f.pool),
        std::vec![
            RecipientApproved {
                recipient: partner.clone()
            }
            .to_xdr(&f.env, &f.pool),
            ChangeExecuted {
                change,
                execute_after,
            }
            .to_xdr(&f.env, &f.pool),
        ]
    );
    assert!(f.client().is_approved_recipient(&partner));
    let payout = (partner.clone(), f.asset.clone(), 10, Category::Poor);
    assert!(f.submit_with_approval(&f.authority, payout.clone(), payout));
    assert_eq!(f.token().balance(&partner), 10);
}

#[test]
fn either_role_can_cancel_an_approval_of_a_recipient() {
    for by_authority in [false, true] {
        let f = setup();
        let partner = Address::generate(&f.env);
        let execute_after = f
            .client()
            .propose_change(&Change::ApproveRecipient(partner.clone()));
        let canceller = if by_authority {
            f.authority.clone()
        } else {
            f.governance.clone()
        };
        f.client().cancel_change(&canceller);
        f.past_timelock(execute_after);
        assert_eq!(
            f.client().try_execute_change(),
            Err(Ok(Error::NoPendingChange))
        );
        assert!(!f.client().is_approved_recipient(&partner));
    }
}

#[test]
fn approving_the_pool_the_roles_or_an_approved_recipient_is_refused_at_proposal() {
    let f = setup();
    let cases = [
        (f.pool.clone(), Error::SelfDistribution),
        (f.governance.clone(), Error::RecipientIsRole),
        (f.authority.clone(), Error::RecipientIsRole),
        (f.recipient.clone(), Error::RecipientAlreadyApproved),
    ];
    for (address, want) in cases {
        f.refuse_proposal(&Change::ApproveRecipient(address), want);
    }
    assert!(!f.client().is_approved_recipient(&f.pool));
    assert!(!f.client().is_approved_recipient(&f.governance));
    assert!(!f.client().is_approved_recipient(&f.authority));
}

#[test]
fn removing_a_recipient_through_the_timelock_blocks_the_next_payout() {
    let f = setup();
    f.give();
    let change = Change::RemoveRecipient(f.recipient.clone());
    let execute_after = f.client().propose_change(&change);
    f.past_timelock(execute_after);
    f.client().execute_change();
    assert_eq!(
        f.env.events().all().filter_by_contract(&f.pool),
        std::vec![
            RecipientRemoved {
                recipient: f.recipient.clone(),
                by: f.governance.clone(),
            }
            .to_xdr(&f.env, &f.pool),
            ChangeExecuted {
                change,
                execute_after,
            }
            .to_xdr(&f.env, &f.pool),
        ]
    );
    assert!(!f.client().is_approved_recipient(&f.recipient));
    f.refuse_approved(&f.recipient, Error::RecipientNotApproved);
}

#[test]
fn the_approval_multisig_removes_a_recipient_at_once() {
    let f = setup();
    f.give();
    f.client().remove_recipient(&f.authority, &f.recipient);
    let (who, _) = f.env.auths().first().cloned().unwrap();
    assert_eq!(who, f.authority);
    assert_eq!(
        f.env.events().all().filter_by_contract(&f.pool),
        std::vec![RecipientRemoved {
            recipient: f.recipient.clone(),
            by: f.authority.clone(),
        }
        .to_xdr(&f.env, &f.pool)]
    );
    assert!(!f.client().is_approved_recipient(&f.recipient));
    f.refuse_approved(&f.recipient, Error::RecipientNotApproved);
}

#[test]
fn an_approval_signed_before_a_removal_fails_after_it() {
    let f = setup();
    f.give();
    // The signers approve a payout, then withdraw the recipient before it is
    // submitted. The same approval is then presented unchanged.
    let approved = f.payout(10);
    f.client().remove_recipient(&f.authority, &f.recipient);
    assert!(!f.submit_with_approval(&f.authority, approved.clone(), approved));
    f.refuse_approved(&f.recipient, Error::RecipientNotApproved);
}

#[test]
fn only_the_approval_multisig_can_remove_a_recipient_at_once() {
    let f = setup();
    let stranger = Address::generate(&f.env);
    for caller in [f.governance.clone(), stranger] {
        assert_eq!(
            f.client().try_remove_recipient(&caller, &f.recipient),
            Err(Ok(Error::Unauthorised))
        );
    }
    // A removal cannot be made by naming the approval multisig. It can only be
    // made via its authorisation.
    outsider_cannot(
        &f,
        "remove_recipient",
        (f.authority.clone(), f.recipient.clone()).into_val(&f.env),
        || {
            assert!(f
                .client()
                .try_remove_recipient(&f.authority, &f.recipient)
                .is_err());
        },
    );
    assert!(f.client().is_approved_recipient(&f.recipient));
}

#[test]
fn removing_a_recipient_that_is_not_approved_is_refused() {
    let f = setup();
    let stranger = Address::generate(&f.env);
    assert_eq!(
        f.client().try_remove_recipient(&f.authority, &stranger),
        Err(Ok(Error::RecipientNotApproved))
    );
    f.refuse_proposal(
        &Change::RemoveRecipient(stranger),
        Error::RecipientNotApproved,
    );
}

#[test]
fn an_approved_recipient_cannot_become_the_approval_multisig() {
    let f = setup();
    f.refuse_proposal(
        &Change::SetAuthority(f.recipient.clone()),
        Error::RecipientIsRole,
    );
    assert_eq!(f.client().authority(), f.authority);
}

#[test]
fn a_removal_is_final_and_the_recipient_cannot_be_approved_again() {
    for by_timelock in [false, true] {
        let f = setup();
        f.give();
        // The signers approve a payout, and the recipient is removed before it is
        // submitted.
        let approved = f.payout(10);
        if by_timelock {
            assert_eq!(
                f.queue_and_execute(&Change::RemoveRecipient(f.recipient.clone())),
                Ok(())
            );
        } else {
            f.client().remove_recipient(&f.authority, &f.recipient);
        }
        assert!(f.client().is_removed_recipient(&f.recipient));
        assert!(!f.client().is_approved_recipient(&f.recipient));

        // A re-approval cannot be queued, so the old approval cannot come back.
        f.refuse_proposal(
            &Change::ApproveRecipient(f.recipient.clone()),
            Error::RecipientRemoved,
        );
        f.env
            .ledger()
            .set_sequence_number(START_LEDGER + 3 * TIMELOCK);
        assert!(!f.submit_with_approval(&f.authority, approved.clone(), approved));
        f.refuse_approved(&f.recipient, Error::RecipientNotApproved);
    }
}

#[test]
fn a_change_that_no_longer_holds_is_refused_at_execution_and_stays_queued() {
    let f = setup();
    let change = Change::RemoveRecipient(f.recipient.clone());
    let execute_after = f.client().propose_change(&change);
    // The approval multisig removes the recipient at once while the change waits.
    f.client().remove_recipient(&f.authority, &f.recipient);
    f.past_timelock(execute_after);
    assert_eq!(
        f.client().try_execute_change(),
        Err(Ok(Error::RecipientNotApproved))
    );
    assert_eq!(f.client().pending_change().map(|p| p.change), Some(change));
}

#[test]
fn a_change_executes_on_its_last_ledger_and_expires_after_it() {
    let f = setup();
    let change = Change::SetAllocation(next_allocation(&f));
    let execute_after = f.client().propose_change(&change);
    let execute_until = execute_after + CHANGE_WINDOW_LEDGERS;
    assert_eq!(
        f.client().pending_change().map(|p| p.execute_until),
        Some(execute_until)
    );

    f.env.ledger().set_sequence_number(execute_until + 1);
    let before = f.state();
    assert_eq!(
        f.client().try_execute_change(),
        Err(Ok(Error::ChangeExpired))
    );
    assert_eq!(f.state(), before);
    assert_eq!(
        PolicyClient::new(&f.env, &f.policy).allocation(),
        allocation(&f.env, &PLACEHOLDER)
    );

    // An expired change can only be cancelled, which frees the slot.
    f.client().cancel_change(&f.authority);
    let execute_after = f.client().propose_change(&change);
    f.env
        .ledger()
        .set_sequence_number(execute_after + CHANGE_WINDOW_LEDGERS);
    f.client().execute_change();
    assert_eq!(
        PolicyClient::new(&f.env, &f.policy).allocation(),
        next_allocation(&f)
    );
}
