// SPDX-License-Identifier: Apache-2.0

//! The harness both fuzz targets share: the real three contracts, and the
//! solvency invariant they assert after every call.

use attestation::{Attestation as AttestationContract, AttestationArgs};
use policy::{Allocation, DischargeTiming, NisabBasis, Parameters, Policy, PolicyArgs};
use soroban_sdk::{
    testutils::{Address as _, Ledger, MockAuth, MockAuthInvoke},
    token, Address, Env, IntoVal, InvokeError, Vec,
};
use zakat_pool::{
    Attestation, Category, Error, ZakatPool, ZakatPoolArgs, ZakatPoolClient, MIN_TIMELOCK_LEDGERS,
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
    pub donor: Address,
    pub recipient: Address,
    pub distributor: Address,
    pub authority: Address,
}

impl Harness {
    pub fn new(split: [u32; 8]) -> Self {
        let env = Env::default();
        env.mock_all_auths();
        env.cost_estimate().budget().reset_unlimited();
        env.ledger().set_sequence_number(START_LEDGER);

        let donor = Address::generate(&env);
        let recipient = Address::generate(&env);
        let asset = env
            .register_stellar_asset_contract_v2(Address::generate(&env))
            .address();
        token::StellarAssetClient::new(&env, &asset).mint(&donor, &(i64::MAX as i128));

        let distributor = Address::generate(&env);
        // The approval account. Its 2-of-3 threshold is Stellar's to enforce
        // and is not modelled here; the harness models whether it approved.
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
        let attestation = env.register(
            AttestationContract,
            AttestationArgs::__constructor(&authority),
        );
        env.register_at(
            &pool,
            ZakatPool,
            ZakatPoolArgs::__constructor(
                &Address::generate(&env),
                &distributor,
                &policy,
                &attestation,
                &asset,
                &MIN_TIMELOCK_LEDGERS,
            ),
        );

        Self {
            env,
            pool,
            asset,
            donor,
            recipient,
            distributor,
            authority,
        }
    }

    pub fn client(&self) -> ZakatPoolClient<'_> {
        ZakatPoolClient::new(&self.env, &self.pool)
    }

    pub fn claimed(&self) -> i128 {
        self.client().balances(&self.asset).iter().sum()
    }

    pub fn held(&self) -> i128 {
        token::Client::new(&self.env, &self.asset).balance(&self.pool)
    }

    /// The buckets never exceed the balance, and no bucket is negative.
    pub fn assert_invariant(&self) {
        let claimed = self.claimed();
        let held = self.held();
        assert!(claimed <= held, "buckets {claimed} above balance {held}");
        assert!(self.client().balances(&self.asset).iter().all(|b| b >= 0));
    }

    /// Attempt a payout authorised by the distributor and, if `approved`, by the
    /// approval account over `payload`. Without its approval the call fails.
    pub fn try_distribute(
        &self,
        category: Category,
        amount: i128,
        payload: &Attestation,
        approved: bool,
    ) -> Result<Result<(), soroban_sdk::ConversionError>, Result<Error, InvokeError>> {
        let full = MockAuthInvoke {
            contract: &self.pool,
            fn_name: "distribute",
            args: (
                self.recipient.clone(),
                self.asset.clone(),
                amount,
                category,
                payload.clone(),
            )
                .into_val(&self.env),
            sub_invokes: &[],
        };
        let approval = MockAuthInvoke {
            contract: &self.pool,
            fn_name: "distribute",
            args: (payload.clone(),).into_val(&self.env),
            sub_invokes: &[],
        };
        let mut mocks = std::vec![MockAuth {
            address: &self.distributor,
            invoke: &full,
        }];
        if approved {
            mocks.push(MockAuth {
                address: &self.authority,
                invoke: &approval,
            });
        }
        self.env.mock_auths(&mocks);
        let result =
            self.client()
                .try_distribute(&self.recipient, &self.asset, &amount, &category, payload);
        self.env.mock_all_auths();
        result
    }
}
