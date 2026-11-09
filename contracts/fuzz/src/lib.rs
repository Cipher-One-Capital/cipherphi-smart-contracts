// SPDX-License-Identifier: Apache-2.0

//! A harness that both fuzz targets share, holding the real contracts, 2 approved
//! assets and the solvency invariant they assert after every call.

use policy::{Allocation, DischargeTiming, NisabBasis, Parameters, Policy, PolicyArgs};
use soroban_sdk::{
    testutils::{Address as _, Ledger, MockAuth, MockAuthInvoke},
    token, Address, Env, IntoVal, InvokeError, Vec,
};
use zakat_pool::{
    Category, Change, Error, ZakatPool, ZakatPoolArgs, ZakatPoolClient, MIN_TIMELOCK_LEDGERS,
};

pub const START_LEDGER: u32 = 100;
pub const ALL: [Category; 8] = [
    Category::Poor,
    Category::Needy,
    Category::Administrators,
    Category::Reconciliation,
    Category::Captives,
    Category::Debtors,
    Category::CauseOfGod,
    Category::Wayfarers,
];

pub struct Harness {
    pub env: Env,
    pub pool: Address,
    pub asset: Address,
    /// A second asset, approved through the timelock after construction.
    pub asset2: Address,
    pub donor: Address,
    pub recipient: Address,
    /// An address that governance never approved as a recipient.
    pub stranger: Address,
    pub authority: Address,
    pub governance: Address,
}

impl Harness {
    pub fn new(split: [u32; 8]) -> Self {
        let env = Env::default();
        env.mock_all_auths();
        env.cost_estimate().budget().reset_unlimited();
        env.ledger().set_sequence_number(START_LEDGER);
        // This raises the TTL limits to let up to 17 timelocked changes run without any
        // entry being archived.
        env.ledger().with_mut(|l| {
            l.min_persistent_entry_ttl = 4_000_000;
            l.min_temp_entry_ttl = 4_000_000;
            l.max_entry_ttl = 6_000_000;
        });

        let donor = Address::generate(&env);
        let recipient = Address::generate(&env);
        let stranger = Address::generate(&env);
        let asset = env
            .register_stellar_asset_contract_v2(Address::generate(&env))
            .address();
        token::StellarAssetClient::new(&env, &asset).mint(&donor, &(i64::MAX as i128));
        let asset2 = env
            .register_stellar_asset_contract_v2(Address::generate(&env))
            .address();
        token::StellarAssetClient::new(&env, &asset2).mint(&donor, &(i64::MAX as i128));
        let governance = Address::generate(&env);

        // The approval account, used to model whether the account approved.
        // Its 2-of-3 threshold can only be enforced by Stellar and is not modelled here.
        let authority = Address::generate(&env);

        let pool = Address::generate(&env);
        let policy = env.register(
            Policy,
            PolicyArgs::__constructor(
                &pool,
                &Allocation {
                    split_bps: Vec::from_slice(&env, &split),
                    remainder: Category::Poor,
                },
                &Parameters {
                    nisab_basis: NisabBasis::Gold,
                    rate_bps: 250,
                    amil_cap_bps: 2_500,
                    discharge_timing: DischargeTiming::Received,
                },
            ),
        );
        env.register_at(
            &pool,
            ZakatPool,
            ZakatPoolArgs::__constructor(
                &governance,
                &authority,
                &policy,
                &asset,
                &Vec::from_slice(&env, std::slice::from_ref(&recipient)),
                &MIN_TIMELOCK_LEDGERS,
            ),
        );
        // The pool is deployed closed, and contributions cannot be accepted until it is opened.
        // This opens it as governance would to let contributions through.
        ZakatPoolClient::new(&env, &pool).resume_contributions();

        let h = Self {
            env,
            pool,
            asset,
            asset2,
            donor,
            recipient,
            stranger,
            authority,
            governance,
        };
        h.queue_and_execute(&Change::ApproveAsset(h.asset2.clone()))
            .expect("the second asset is approved");
        h
    }

    /// Returns the asset at `index`: the first when even, the second when odd.
    pub fn asset_at(&self, index: u8) -> &Address {
        if index.is_multiple_of(2) {
            &self.asset
        } else {
            &self.asset2
        }
    }

    /// Proposes `change`, moves the ledger past the delay and executes it.
    pub fn queue_and_execute(&self, change: &Change) -> Result<(), Error> {
        let client = self.client();
        let execute_after = match client.try_propose_change(change) {
            Ok(Ok(after)) => after,
            Err(Ok(error)) => return Err(error),
            other => panic!("propose_change failed in the host: {other:?}"),
        };
        self.env.ledger().set_sequence_number(execute_after + 1);
        match client.try_execute_change() {
            Ok(_) => Ok(()),
            Err(Ok(error)) => Err(error),
            Err(Err(error)) => panic!("execute_change failed in the host: {error:?}"),
        }
    }

    pub fn client(&self) -> ZakatPoolClient<'_> {
        ZakatPoolClient::new(&self.env, &self.pool)
    }

    pub fn claimed(&self) -> i128 {
        self.claimed_in(&self.asset)
    }

    pub fn held(&self) -> i128 {
        self.held_in(&self.asset)
    }

    pub fn claimed_in(&self, asset: &Address) -> i128 {
        self.client().balances(asset).iter().sum()
    }

    pub fn held_in(&self, asset: &Address) -> i128 {
        token::Client::new(&self.env, asset).balance(&self.pool)
    }

    /// Asserts, for both assets, that the buckets never exceed the balance and that no
    /// bucket is negative.
    pub fn assert_invariant(&self) {
        for asset in [&self.asset, &self.asset2] {
            let claimed = self.claimed_in(asset);
            let held = self.held_in(asset);
            assert!(claimed <= held, "buckets {claimed} above balance {held}");
            assert!(self.client().balances(asset).iter().all(|b| b >= 0));
        }
    }

    /// Attempts a payout of `amount` of `asset` from `category` to `to`, with the approval
    /// account's authorisation of the `approved` arguments when `approved` is set, and with
    /// no authorisation at all otherwise.
    /// The approved arguments can be different from the submitted ones.
    pub fn try_distribute(
        &self,
        to: &Address,
        asset: &Address,
        category: Category,
        amount: i128,
        approved: Option<(Address, Address, i128, Category)>,
    ) -> Result<Result<(), soroban_sdk::ConversionError>, Result<Error, InvokeError>> {
        let invoke = approved.map(|args| MockAuthInvoke {
            contract: &self.pool,
            fn_name: "distribute",
            args: args.into_val(&self.env),
            sub_invokes: &[],
        });
        let mocks: std::vec::Vec<MockAuth> = invoke
            .iter()
            .map(|invoke| MockAuth {
                address: &self.authority,
                invoke,
            })
            .collect();
        self.env.mock_auths(&mocks);
        let result = self.client().try_distribute(to, asset, &amount, &category);
        self.env.mock_all_auths();
        result
    }
}
