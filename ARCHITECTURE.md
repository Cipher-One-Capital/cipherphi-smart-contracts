# Architecture

CipherPhi is the layer that moves an operating, audited Swiss zakat foundation onchain. The rules of zakat, pooled custody, allocation across the 8 zakat categories, and distribution only to eligible recipients, are a set that 2 smart contracts encode and 2 Foundation multisigs enforce.

This file is the architecture record that sets out what is built, why each decision was settled the way it was, and what is deliberately not built. A component a reader might expect can only be absent if it is named under "Exclusions", with the trigger that would change it.

## Moat

Onchain zakat is an idea that predates this project. HAQQ is a chain that routes a tenth of every ISLM issuance into the Evergreen DAO endowment for Islamic charity,[^1] Zakat Coin is one of several tokens that have proposed donor-facing zakat platforms,[^2] and the academic literature is a body of work that carries several blockchain zakat collection and distribution models.[^3] Almost all of them are designs where the chain came first and the institution second: a new token, a new DAO, or a paper design, looking for a zakat body to adopt it.

CipherPhi is a design that runs the other way. The Swiss Zakat Foundation is an institution that already collects and redistributes zakat under federal supervision, with audited accounts and a working eligibility process, and this project is the step that puts its rail onchain.

The contract is the part that cannot be defended. It is a few thousand lines of Rust that any competent team could write. This publishes the design rather than guarding it to show that the moat lies elsewhere. The moat is the institution that already exists: the collection, the donor base, the vetting process, the partner network, and the audit relationship can only be moved, not invented. A competitor starting today is a team that writes the contract in a month and then spends years becoming a supervised foundation with an unqualified audit opinion.

## Stellar

The 4 pillars below are features that already sit in the protocol rather than being built for this project.

**USDC.** USDC is a stablecoin that Circle issues on Stellar directly, and CCTP is the protocol that moves it in from other chains by burn and mint rather than through a wrapped asset or a third-party bridge.[^4] Bridge counterparty risk cannot be carried by a pool holding donated zakat, and on Stellar it never has to be.

**Token interface.** The Stellar Asset Contract is a contract that every Stellar asset has at a deterministic address, implementing SEP-41, so custody and transfer are a standard interface call. No token contract has to be written, audited, or got right in CipherPhi's codebase.

**Cheap settlement suited to per-category accounting.** A fee is a cost that stays a fraction of a cent.[^5] Splitting every contribution across 8 zakat categories, with checked arithmetic and an event recording the split, can be afforded per contribution. Batching is the practice that a chain with meaningful gas would push the project toward, and the one that destroys the audit trail.

**A built-in disbursement rail.** The Stellar Disbursement Platform is a rail that SDF maintains for exactly the job this project reaches in a later phase: paying individual recipients and off-ramping to fiat.[^6] The roadmap can be delivered without CipherPhi building a payments company.

Soroban is the layer that supplies what a payment network alone cannot: the 8-category allocation and the payout approval, run as enforced code rather than as application logic sitting above the rails.

## Structure

The onchain structure is a modular set that holds 2 Soroban contracts and 2 Foundation multisigs on separate keys. Personal data can only be held offchain.

## Boundary

The boundary is the line that keeps funds and rules on Stellar, and identity and human judgment off it.

**Erasure capability:** An immutable chain and a right to erasure are only in tension if personal data reaches the ledger. Personal data cannot reach the ledger, so the obligation can only attach to the off-chain registry, where it is met.

**Onchain state stays bounded:** Recipient records are the only data that grows without limit, and they are the one set kept off the ledger. The remaining state can be held inside Soroban's storage and read limits at any donor count.

**The ledger stays readable by outsiders:** The money can be followed by a regulator, an auditor, or a donor without any of them being handed a list of aid recipients.

**Cost:** Eligibility becomes a decision that is made off-chain. That decision can only take effect as a signed payout approval, and the signatures are verified by the network before any money moves. The eligibility decision does not have to be trusted for a reviewer to verify that one was made, by known keys, before funds left the pool.

<img alt="What crosses the on-chain boundary" src="static/the-boundary.png" width="720">

> Figure 1: *What crosses the on-chain boundary*

*Only a signed transaction, a signed approval, and reads of events can cross the line.*

## Contracts

Custody and policy are separate deployments that exist from the first day, and payout approval is a separate account that the pool requires for each payout.

| Component | Responsibility |
| --- | --- |
| `zakat_pool` | The custody contract that holds the 8 category balances and the only 2 entry points that move value |
| `policy` | The contract that holds the Shariah parameters and computes the allocation over a contribution |
| Approval multisig | A Stellar account that authorises each payout. A payout can only be authorised by 2 of its 3 Foundation signers |

<img alt="Custody separated from policy and payout approval" src="static/the-contracts-structure.png" width="720">

> Figure 2: *Custody separated from policy and payout approval*

**The separation:** One contract is the custodian that holds assets. The other is consulted by it and can never be used to move anything. This separates them to put the component most likely to change, a refinement to the Shariah parameters, in the one contract that holds no money.

**The upgrade is granular:** A refinement to the amil cap is a parameter change that `policy` takes, or a redeployment if the allocation logic itself changes. A change of approval scheme is a replacement of the approval account: it is a multisig today, and the pool's authorisation check can stay unchanged if it later becomes a contract account with checks of its own. Custody cannot be touched by either, so donor funds can never be migrated for a reason unrelated to custody. A single contract is the design that would have forced the opposite: every change to a Shariah parameter redeploying the thing holding the money.

**The cost is a trust boundary:** A cross-contract call is a place where a wrong answer can enter. The address of `policy` is pinned by `zakat_pool` in governance-controlled storage, and can only be treated as an untrusted input rather than a trusted collaborator: an allocation can only be accepted if the returned vector has exactly 8 elements, every element is zero or positive, and they sum exactly to the amount received. A sum-only check is a check that would not be enough: a split of the shape `[amount + x, -x, 0, ...]` sums correctly, passes checked arithmetic, and leaves total custody intact while silently crediting one category from another's beneficiaries. Payout approval is a check that crosses no contract boundary at all: the approval account's authorisation is required by the pool itself, and the account's threshold is enforced by the network. The audit scope is a set of 2 Wasm binaries, which independent review can still take on well before mainnet rather than alongside it.

> **Not in this build.** Custody code cannot be upgraded in place. Soroban contracts can only be made mutable by compiling in an upgrade entry point; a contract that omits `update_current_contract_wasm` is a contract that stays immutable by construction.[^7] This omits the entry point from `zakat_pool` to keep code that holds donor funds from becoming an admin key by another name. `policy` can only be repointed, and the approval account only replaced, via governance under the timelock, and neither can hold anything. Custody can only be changed via a new deployment with a published address and a migration a donor can watch.

## Storage

The storage is a set that can only grow within bounds set by the design.

A time to live is a lifetime that every Soroban entry carries. Since Protocol 23, a persistent entry whose TTL lapses is evicted to the archive and can be restored automatically when it next appears in a transaction footprint, with the rent and write fees of that restoration paid by whoever sends the transaction.[^8] A temporary entry is a different kind, one that is deleted permanently at expiry and cannot be restored.[^9] Unbounded per-contribution storage is therefore a failure mode where the contract keeps working but grows steadily more expensive to touch, with restoration costs landing on donor transactions at exactly the moment the platform is succeeding.

| Contract | Entry | Durability | Content |
| --- | --- | --- | --- |
| `zakat_pool` | `governance` | Instance | The Foundation's 2-of-3 multisig that sets the rules. It can only be set at construction |
| `zakat_pool` | `authority` | Instance | The 2-of-3 approval multisig that authorises each payout. It can only be replaced via a timelocked `SetAuthority` |
| `zakat_pool` | `policy_addr` | Instance | The `policy` contract that computes each allocation. It can only be repointed via the timelock |
| `zakat_pool` | `approved_assets` | Instance | The set of SAC addresses that the pool accepts: USDC, and later EURC |
| `zakat_pool` | `paused` | Instance | A flag that pauses contributions when set. It can only be set and cleared by governance, instantly |
| `zakat_pool` | `timelock_ledgers` | Instance | The delay in ledgers that every timelocked change waits out. It can only be set at construction, and cannot be under 120,960: 7 days at the 5-second ledger close Stellar targets |
| `zakat_pool` | `pending_change` | Instance | The one queued governance change that waits out the timelock, with its new value and the ledger after which it can execute |
| `zakat_pool` | `category_balances` | Persistent, keyed by asset | The 8 buckets per approved asset that make up the whole of the accounting state |
| `policy` | `controller` | Instance | The pool, the only address that `policy`'s setters accept. A change can only reach `policy` via the timelock |
| `policy` | `allocation` | Instance | The 8-category split and the category that takes the rounding remainder |
| `policy` | `parameters` | Instance | The Shariah parameters that sit beside the split: the nisab basis, the zakat rate, the amil cap and the discharge timing |

The table is the complete stored set across both contracts. The set can only grow when governance approves an additional asset, which adds 8 buckets, and cannot grow with donor count, contribution count, or recipient count. Configuration is held in instance storage, a store that shares one lifetime with the contract itself, so all of it can be covered by a single extension. Only the per-asset balances are keyed separately. This keys them apart to isolate the one thing that grows when an asset is approved. Nothing can be held in temporary storage, which is deleted rather than archived at expiry. This avoids temporary storage to stop a queued governance change from vanishing mid-timelock, which would be a silent failure of the delay itself.

A single-use nonce is an entry that each payout approval also leaves on the ledger. That entry is created by the network rather than the contract, is temporary, and is deleted once the approval's expiry passes. Its count can only grow with the number of approvals live at once, not with the number ever made.

A TTL-bump job is a scheduled task that extends every entry above, together with each contract's instance and its Wasm code. This keeps every entry live to guarantee nothing load-bearing is archived through disuse and no donor transaction pays surprise restoration fees. The pool cannot be changed by the job: a TTL can be extended without any role's authorisation, by any account that pays the fees.

## Events

The previous section is the list of everything the contracts store. Everything else a donor or an auditor might want to see can only be emitted as a Stellar ledger event and read back by an off-chain indexer.[^10]

A contribution is a call that emits one event carrying the donor address, the asset, the amount, and the resulting 8-category split. A distribution is a call that emits one carrying the recipient address, the category, the asset and the amount, in a transaction that also carries the signed approval that authorised it. Those 2 event types are the complete record of what the pool credits and pays, and every category balance can be reconstructed from them at any point in its history. The pool's actual token balance, including any surplus sent straight to its address, can only be read from the asset's own standardised transfer events (Building blocks). Governance actions, asset approval included, are events of their own that are covered under monitoring below.

**Emission is cheap where storage is not.** A ledger event is a record that is written once and can never be read by a contract again, so it costs no ongoing rent and counts against no read limit. A persisted copy of the same record is storage that would have to be maintained for as long as anyone might want the trail, which for zakat is indefinitely.

**The indexer is infrastructure and can fail.** It is a service that reads the ledger and shapes it for a donor. The donor flow cannot be served while it is down.

**It cannot fail silently or dishonestly.** Every figure it renders is a value that resolves to a ledger entry with a transaction hash, shown to the donor next to the figure. The record cannot be lost with the indexer. A rebuild is a job that cannot be run casually, though: events are only served by RPC over a bounded retention window, so a rebuild covering the full history can only read from Stellar's history archives. That ingestion path is a part that ships with the indexer, open source in the same repository as the contracts once it is published there, before security review (Provenance). Continuous ingestion and the TTL-bump job are jobs that the delivery team owns through the pilot and hands to the Foundation with the runbook at mainnet launch. This gives both jobs an owner to stop an unowned schedule from quietly ending the guarantee that nothing load-bearing is archived through disuse.

<img alt="From ledger event to donor proof view" src="static/the-event-trail.png" width="720">

> Figure 3: *From ledger event to donor proof view*

*Every state the donor sees is one that resolves to a transaction hash they can check themselves. The indexer is a convenience that holds no authority.*

## Functions

Value can only be moved by 2 functions on `zakat_pool`. Everything else across both contracts is configuration, and these 2 are the functions that the audit is really about.

`contribute(donor, asset, amount)`

Transfers `amount` of `asset` from `donor` into the pool and credits the 8 category buckets when the asset is approved.

1. Requires auth from the donor, and rejects unless `amount` is positive
2. Rejects if contributions are paused, and unless `asset` is in `approved_assets`
3. Pulls the asset from the donor via the Stellar Asset Contract, which authorises the transfer against the donor's own signature, and rejects unless the pool's balance rose by exactly `amount`. A donor's funds cannot be moved into the pool by a third party, since no allowance or `transfer_from` path exists, and cannot be refunded once in
4. Calls `policy` for the split, and rejects unless it returns exactly 8 amounts, each of them zero or positive, summing exactly to the amount received
5. Increments the category buckets with checked `i128` arithmetic
6. Asserts that the buckets do not claim more than the pool holds
7. Emits a Contribution event carrying donor address, asset, amount, and the resulting split

Errors:

- The amount is not positive.
- The pool is paused.
- The asset is not approved.
- The pool's balance does not rise by exactly the amount.
- The allocation does not hold exactly 8 amounts, each zero or positive, summing to the amount received, or `policy` does not answer with one.
- The arithmetic overflows.
- The buckets claim more than the pool holds.

**No authorisation gate.** Contributions cannot be gated. A gate is a check that would require an onchain allowlist of donors, which means donor identity onchain. This leaves `contribute` open to anyone to keep donor identity off the ledger, as the boundary requires.

`distribute(recipient, asset, amount, category)`

Transfers `amount` of `asset` from a category bucket to `recipient` when the approval multisig has authorised exactly that payout.

1. Requires auth from the approval multisig over the 4 arguments. The auth is refused by the network unless 2 of the 3 signers signed, for this pool, on this network, unexpired, and not used before
2. Rejects unless `amount` is positive, `asset` is in `approved_assets`, and the recipient is not the pool itself
3. Checks the category bucket holds `amount`
4. Decrements the bucket, then transfers, in that order, and rejects unless the pool's balance fell by exactly `amount`
5. Asserts that the buckets do not claim more than the pool holds
6. Emits a Disbursement event carrying the recipient address, the category, the asset and the amount

Errors:

- The amount is not positive.
- The asset is not approved.
- The recipient is the pool itself.
- The category bucket does not hold the amount.
- The pool's balance does not fall by exactly the amount.
- The arithmetic overflows.
- The buckets claim more than the pool holds.

A missing or insufficient approval can only fail as a host authorisation error, not a pool error: it is refused by the network before any of the pool's checks run. `distribute` cannot be stopped by the contribution pause. This keeps payouts open to guarantee a pause never strands what the pool already holds.

<img alt="Assertions before effects on the distribute path" src="static/the-distribute-path.png" width="720">

> Figure 4: *Assertions before effects on the distribute path*

**Ordering.** The bucket can only be decremented before the transfer. Any contract already on the call stack cannot be re-entered under Soroban's host rules, so this ordering is a backstop rather than the primary control. This follows checks-effects-interactions to keep the path safe if the platform guarantee ever changes. A contract whose safety depends on a platform guarantee is one that reads as safe for the wrong reason.

**Binding.** An approval is a signature that covers the exact recipient, asset, amount and category. None of them can be changed by whoever submits the transaction, the delivery team in practice: a changed argument is a call that nobody signed, and it cannot pass the network. The full scope of the signature is set out under replay in the security model.

## Roles

| Role | Holds | Can | Cannot |
| --- | --- | --- | --- |
| Governance | Foundation 2-of-3 multisig | Pause and resume contributions. Under the timelock: repoint `policy`, change the split and parameters, approve an asset, replace the approval multisig. Cancel a pending change | Move funds. Approve a payout |
| Approval multisig | Foundation 2-of-3 multisig, on keys separate from governance's | Authorise each payout. Cancel a pending change | Change policy. Pause or resume. Propose a change |
| Delivery team | No key on mainnet | Build transactions, run the indexer and the TTL job, monitor and alert the signers | Sign anything on mainnet |

The separation is a control that assumes one key will eventually be compromised.

**Stolen governance keys:** produce a cancelled proposal rather than a payout. The funds can only be reached by governance via replacing the approval multisig, a change that waits out the timelock in public and can be cancelled by the approval multisig.

**One stolen approval key:** produces nothing on its own: a payout can only be approved by 2 signers. The stolen signer can only be replaced on the account itself, by the Foundation.

**The approval multisig's veto:** a power that reaches beyond `SetAuthority`. Any pending change can be cancelled by 2 approval signers, and every rule change, their own replacement included, can be held for as long as they keep cancelling. The veto is the lesser of their powers: any payout can be approved by the same 2 signers. This accepts the veto to keep a single cancel path for every change. A dispute between the 2 Foundation accounts cannot be settled by the chain, only by the Foundation's board.

**Lost approval keys:** if 2 of the 3 are lost, no payout can be approved. The approval account can then only be replaced by governance via `SetAuthority`, after the timelock, and with nobody left to cancel, the replacement cannot be stopped.

**When each multisig arrives:** Both are multisigs that exist from deployment. On testnet, seats on either can be held by the delivery team: testnet holds no value. This lets the delivery team exercise the approval flow to avoid waiting on Foundation signers during development. Those seats cannot carry over: the mainnet pool can only be constructed with Foundation-only accounts, checked before contributions are resumed.

## Signers

**Both multisigs belong to the Foundation:** The approval multisig's 3 seats are the seats that the Foundation's Executive Director, the Shariah authority, and an independent signer appointed by the Foundation hold. A payout to any address can be approved by any 2 of the 3, the same rule as joint signature by two (*Kollektivunterschrift zu zweien*) for a Swiss foundation. The chain adds a public event that names every payout's recipient, amount and category. Governance can only be held by the Foundation, on separate keys, and no key on either can be held by the delivery team on mainnet.

The Shariah authority is a signer that holds one of the seats. Eligibility is partly a category question: whether a recipient falls inside one of the 8. This seats the Shariah authority to cover that question with the same judgment the parameter gate already relies on.

<img alt="What each key holder can and cannot do" src="static/the-key-topology.png" width="960">

> Figure 5: *What each key holder can and cannot do*

**Seat governance.** Governance is the onchain counterpart of the foundation board that sets the Shariah parameters, can pause contributions at once, and can replace the approval multisig if its keys are lost. Every change other than pausing and resuming can only take effect after the timelock, in public. On mainnet it is a 2-of-3 multisig that the Foundation holds, with a high threshold of 2 and the master key at weight 0, and its keys can never be shared with the approval multisig. The funds cannot be reached by governance alone: any pending change can be cancelled by the approval multisig. The delivery team is the operator that builds transactions, monitors the pool, and alerts the signers, and cannot hold a seat on mainnet. Holder identities, seat allocation, and custody method are the details that the mainnet deployment publishes.

**Rotation happens on the accounts:** The signers on each multisig are account configuration rather than contract state, and can only be changed on the account itself. The high threshold is a setting of 2 on both accounts that lets 2 signers replace a lost third and gives them nothing beyond the medium threshold they already reach. This sets the high threshold at 2 to keep a lost governance seat refillable: the governance account cannot be replaced in the pool, and a seat that could never be refilled would be a slow path to a frozen configuration. A signer change is an account operation rather than a contract event. This extends the monitoring described below to both accounts' options changes to catch it alongside the pool's events. The approval account as a whole can only be replaced via `SetAuthority`, under the same delay and the same events as every other governance change, and cannot be the governance account.

**What sits behind the timelock.** Repointing `policy`, changing the split or a Shariah parameter, replacing the approval multisig and approving an asset are the changes that wait out the delay: it is set at construction and cannot be shorter than 120,960 ledgers, which is 7 days at the 5-second ledger close Stellar targets. The delay can only be counted in ledgers, not seconds, and would be shorter in wall-clock time if ledgers closed faster. Each change is a proposal that emits an event when proposed and when executed. This announces the change at proposal to make it visible before it takes effect. Only one change can be queued at a time, and it can be cancelled by either governance or the approval multisig, which also emits an event. Pausing and resuming contributions cannot be delayed: a pause only stops money coming in, and lifting it puts nothing at risk. Transactions within a ledger cannot be ordered by fee on Stellar: ordering across accounts is shuffled using the hash of the agreed transaction set, and a fee can only affect whether a transaction is included when the ledger is full, not the sequence in which included transactions execute.[^11] A defensive transaction therefore cannot be relied on to execute ahead of an attacking one in the same ledger. This keeps the pause instant to make the next ledger safe rather than race an attacker inside the current one.

**Monitoring.** The delay is a cost that is only worth paying if someone is watching. The delivery team is the watcher that monitors the proposal, execution and cancellation events, pause and resume, every Disbursement, and options changes on both multisig accounts, with an off-chain escalation path to the signers. This watches every change to make the timelock a control rather than a delay nobody observes. It can be built from existing Stellar tooling, either self-hosted against RPC event streams or through a third-party alerting service, and the choice between them is an operational decision rather than an architectural one.

*A distribution cannot be completed by a single key: every payout can only be approved by 2 of the 3 approval signers, and it is checked by the network before the pool runs.*

## Assets

USDC is the first asset, held through the Stellar Asset Contract. EURC is the asset that joins it through the same interface once the donor base asks for it.

A contract address is an address that every Stellar asset has reserved for it, and the Stellar Asset Contract deployed there implements the SEP-41 token interface.[^12] Regulated stablecoins can only be held and moved by the pool through that interface, so no custom token logic exists anywhere in the codebase, and the same code path can serve any asset that implements the standard.[^13]

**Transfers are measured, not trusted.** A failed SAC transfer, on a missing trustline or a frozen balance, is a failure that aborts the whole transaction, so a half-recorded payment cannot be left behind. The other failure is the one the pool checks: a transfer that succeeds but moves a different amount. A transfer can only be accepted if the pool's own balance moved by exactly the amount asked for. This reads the balance before and after every transfer to catch an asset that moves less than requested.

**EURC is a second denomination:** Category balances can only be keyed by asset, so each approved asset is a denomination that carries its own 8 buckets and its own solvency invariant. The alternative, one set of buckets across 2 denominations, is a design that would be an accounting error rather than a shortcut: zakat owed in euros cannot be discharged by dollars at an implied rate the contract never agreed.

`approved_assets` is a set that only a governance transaction can add an address to. Denominations cannot be converted by that transaction, and no asset is priced against another anywhere in the contracts. USDC is the asset that comes first, for donor demand and depth. This adds EURC next to serve a donor base and partner organisations that transact in euros and francs.

**The addresses are fixed before the pool is deployed:** A Stellar Asset Contract address is an address that is derived deterministically from the asset and the network passphrase, so the USDC entry in `approved_assets` is a known constant on testnet and a different known constant on mainnet. Both can only be taken from Circle's published references, not from a lookup at runtime, and are written into the repository next to the deployment that uses them.[^14]

> Circle's CCTP is a live protocol on Stellar that moves native USDC in from other chains by burning at the source and minting at the destination, with no wrapped asset and no third-party bridge.[^4] A donor holding USDC elsewhere is a donor who arrives on Stellar with real USDC, and the transfer can only be seen by the pool as an ordinary SAC transfer. No custom token contract, wrapping, or bridging logic can be found anywhere inside the pool: the cross-chain path is one that terminates before it reaches CipherPhi's code.

## Pilot

The pilot is a first phase that disburses directly from `zakat_pool` to a vetted partner-organisation address.

<img alt="Eligibility decided off-chain, approved before funds move" src="static/the-pilot-flow.png" width="720">

> Figure 6: *Eligibility decided off-chain, approved before funds move*

*The recipient of a pilot distribution is an organisation that the Foundation has already vetted through its existing eligibility framework. A payout to that organisation's address, in its category, can only be signed by the approval multisig.*

**The recipient address is verified before it is used.** An asset can only be received by a Stellar account via a trustline for it, so a distribution to a partner organisation that has not established one cannot arrive and fails instead. The transfer is atomic: nothing can be consumed by a failure, and the approval can still be used until it expires. This confirms the trustline as part of confirming the address to avoid discovering it on the first real distribution.

**The scope is a deliberately narrow one for a first mainnet distribution.** Every additional component between the pool and the recipient is a component that must be live, correct, and audited before a single real zakat payment can be made. This pays a vetted partner organisation directly to remove all of them. Individual beneficiaries can then only be reached by the organisation through the process it already runs, under annual audit, which is a process with a track record rather than a system built for this grant.

**The Stellar Disbursement Platform is the planned rail for individual-recipient payouts and fiat off-ramp in a later phase.**[^6] It is an existing platform that SDF maintains for exactly this job, and one that is on SCF's own integration list. This chooses it to make reaching individual recipients later an integration rather than a build. The funded scope can be delivered without it.

## Donors

A contribution is shown to its donor in 4 states, and each one can only be read from a ledger event that carries the transaction hash which produced it.

<img alt="A contribution from received to delivered" src="static/the-donor-flow.png" width="720">

> Figure 7: *A contribution from received to delivered*

*A donor who wants to check the platform rather than trust it is one who opens a Stellar explorer, finds their transaction, and reads the same events the interface read.*

*The interface is a rendering that shows only public data.*

**Known limitations.** A distribution can only be shown to a donor at category level: their category received a distribution to a verified recipient, not their individual coins reaching a named person. Zakat is pooled by construction, and the 8-category split is the unit that the zakat methodology operates on, so per-donor tracing to an individual recipient would be a fiction dressed as cryptography. Category-level delivery, with a verifiable approval on every disbursement, is the strongest true claim available. This makes the category-level claim rather than a stronger false one to keep every claim in this document checkable.

## Funding

A contribution can only be made from a Stellar account that holds a USDC trustline and USDC. Privy is the wallet layer that supplies only keys, and none of those 3.

**The account and the trustline are sponsored. This pays the reserves to keep the donor from ever holding XLM.** Sponsored reserves are the Stellar feature that lets CipherPhi pay the base reserve for a new donor account and its USDC trustline, and fee-bump transactions are the feature that covers the transaction fee on the same principle.[^15] A donor arriving with an email address and no prior wallet is one who ends up USDC-ready without ever acquiring the native asset. Both are protocol features rather than components to build, and the sponsorship cost at pilot volume is a sum of a few XLM.

**Acquiring the USDC is the real boundary, and the pilot is scoped inside it.** Pilot donors are donors who arrive already holding USDC, whether from an exchange, a wallet, or another chain via CCTP. Privy is still the tool that does the work that matters for them: it removes seed-phrase custody, the barrier for a donor who holds stablecoins but has never managed keys.

**How many such donors exist is measured.** A quantified population of stablecoin-holding donors inside the Foundation's base cannot be claimed: no survey has been run. A survey is an early delivery activity that produces a written report of how many donors were contacted, how many hold or can acquire USDC, and how many committed to the pilot. If that number comes back too small to run a donor-side pilot, the pilot can only demonstrate the distribution path with Foundation-originated contributions, and the donor-acquisition question moves to the phase that funds an on-ramp.

**Fiat on-ramp is a later phase:** A donor who holds only Swiss francs can only be reached via a SEP-24 anchor that converts fiat to USDC on Stellar, which is exactly the interface anchors exist to provide. It is listed as planned in the building blocks table, and the trigger for commissioning it is demonstrated pilot demand from donors who cannot self-fund.

## Onboarding

Privy is the embedded email and social wallet that lets a donor who holds stablecoins but has never managed a seed phrase contribute without one. It is one of the wallet integrations that Stellar's own developer tooling documentation lists.[^16] The integration is a working path, not an estimate: email login through to a funded testnet contribution can be run as a published working example.[^17]

Seed-phrase custody is the barrier that stops a mainstream donor base from ever reaching the contract, and it cannot be compensated for by any amount of contract quality. Privy is the shortest path that crosses that barrier at pilot scale.

## Privacy

Recipient personal data is held by the platform and can never be written to the Stellar ledger. The ledger is the record that holds addresses, amounts, and the signed approvals that authorised each payout.

**On-chain:**

- Pooled custody and category balances
- Configuration and allocation policy
- Contribution and disbursement events
- One signed approval per disbursement
- Addresses only

**Off-chain:**

- Donor identity and KYC
- Recipient identity and eligibility evidence
- The human Shariah sign-off
- The approval-signing tooling
- The indexer and the recipient registry

**Erasure:** An immutable ledger and a right to erasure are a pair that is usually presented as an unresolved tension. They are only in tension if personal data goes onto the ledger. This keeps personal data off the ledger to remove the tension. Erasure can only operate against the off-chain registry, which is where every piece of personal data lives. What remains onchain afterwards is a record that holds an address, an amount, and a signature.

**Open Contribution:** A contribution can be made by anyone. This leaves the contract ungated to keep donor identity off the ledger. The donor cannot be anonymous to the institution, though: the Foundation carries Swiss anti-money-laundering duties on the funds it receives, and the donor records, screening, and source-of-funds handling that satisfy them are kept in the same off-chain registry as everything else on this page, applied to onchain contributions exactly as to bank transfers. The contract's openness is a privacy decision that concerns the ledger, not a decision to stop looking.

**What the contract cannot do:** Screening is a check that the application runs before a contribution transaction is built, which covers every donor who arrives through CipherPhi. A contributor who bypasses the application and calls the contract directly cannot be refused: the entry point is open by design. The contribution cannot be refunded either: `contribute` and `distribute` remain the only 2 functions that move value. For that case the position is accept and report rather than return, a position that is agreed in writing with the Foundation's compliance adviser and produced before mainnet. This names the limit to avoid documenting a return path the bytecode does not have.

The Swiss Zakat Foundation is an institution that already runs recipient files this way under Swiss federal supervision, with document verification and confidential beneficiary communication. Recipient identities are protected by that existing process, and it cannot be weakened by CipherPhi, since no part of it is put onchain.

## Shariah

The rules of zakat are parameters that governed contract state holds, each validated before mainnet distribution.

| Parameter | What it sets | Enforced by the contract? |
| --- | --- | --- |
| Category split | The shares, in basis points summing to 10,000, that split each contribution across the 8 categories | Yes. Every contribution is split by it |
| Remainder category | The category that receives the rounding remainder | Yes. It must hold a share above zero, and it cannot be the administrators, whose credit the remainder could otherwise carry past the amil cap |
| Amil cap | The ceiling that applies to the administrators' share | Yes. The split's administrators share cannot exceed it, and the cap itself cannot exceed 25% |
| Zakat rate | The rate that the donor calculator applies, above zero and at most 10% | No. Published for the application to read; the bounds are checked |
| Nisab basis | The standard, gold or silver, that the nisab threshold is measured in | No. Published for the application; the price and its timestamp come from the application and are shown beside the figure they produce |
| Discharge timing | The point where the donor's obligation is treated as discharged: received, allocated or delivered | No. A published statement of the methodology |

The rules are a methodology that is not ours to invent. A constant in Rust is an encoding that would make any refinement a redeployment, and would put a technical team in the position of having settled a methodological question by writing it down. This holds them as governed parameters to put the methodology with the person qualified to set it. The parameters that act on money, the category split, its remainder and the amil cap, are enforced by the contract on every contribution. The rate, nisab basis and discharge timing are the parameters that act on the donor's calculation rather than on the pool, and can only be changed under the same timelock as the rest. This publishes them onchain to let the application read them from the chain rather than hold its own copy. The Swiss Zakat Foundation's methodology is a document published in full, so the parameter set has a documented starting point rather than a blank one.[^18]

**The parameter set is held through validation by our Shariah authority**, Muhammad Emamally, halal investment advisor. That validation is a gate: no distribution can occur on mainnet until it is given, recorded, and matched against the parameter set actually loaded on the contract. That match is an item that can be checked before the first mainnet distribution.

> **On the scope of that authority**
>
> The authority is one qualified advisor who validates a parameter set for a controlled pilot with one partner organisation. A standing Shariah board cannot be claimed from it, and this document does not claim one. An independent board with formal scrutiny is a body that is planned as the platform moves beyond pilot scale, and can only be named as planned rather than present. This states the difference to keep a reviewer who later discovered it from being right to discount everything else in this document. The trigger for standing it up is the first distribution beyond the pilot partner.

## Security

Every control below is a control that maps to a named category in the OWASP Smart Contract Top 10 for 2026, which is compiled from 2025 incident data.[^19] This maps them to show that none of these are exotic: the categories that cost the most are mundane, and they can be closed by construction rather than by vigilance.

**Access control, the costliest 2025 category at USD 953 million in losses.** Access is split into 2 roles that sit behind `require_auth`, each a Foundation multisig on separate keys: governance is the role that sets the rules under a public timelock, and the approval multisig is the role that authorises payouts. Funds cannot be moved by governance, and the rules cannot be changed by the approval multisig. The funds cannot be reached by governance alone: any pending change can be cancelled by the approval multisig. The 2 roles cannot be held by the same account.

**Input validation, 34.6% of direct contract exploits, the class behind the Aftermath, Singularity, and Ekubo losses.** The class is one of setters that never enforce the value range the rest of the contract assumes. Every parameter `policy` accepts is bounds-checked at the setter: the category split must sum to unity, the remainder category must hold a share and cannot be the administrators, the amil cap sits in an allowed band and the administrators share cannot exceed it, and the rate sits in its own band, above zero. A value the contract cannot honour can only be rejected on the way in, not discovered on the way out.

**Arithmetic and integer overflow, the Aftermath pattern where a fee read at the wrong signedness became a rebate.** Signed-integer and fixed-point operations are checked throughout. This checks every operation to make an overflow, an underflow, or a rounding remainder a failed transaction rather than a wrong balance. The 8-way split is a call that returns amounts summing exactly to the amount received, with any remainder assigned deterministically rather than dropped. The receiving category is a configured parameter that the Shariah authority validates alongside the others, not a choice the implementation makes on its own. No value can cross between signed and unsigned representations without an explicit checked conversion.

**Business logic, the solvency invariant.** The solvency invariant is the rule that, after every state change, the sum of the 8 category balances cannot exceed the pool's actual token balance, asserted inside `contribute` and `distribute` rather than off-chain. Buckets can only grow through `contribute`, so the accounting cannot be corrupted by a direct token transfer to the contract address; such a transfer can only leave a surplus that is visible and, in this build, unspendable. A surplus cannot be recovered, so no third function that moves value exists: `contribute` and `distribute` remain the only 2.

**Reentrancy and unchecked external calls, GMX V1 lost USD 42 million to reentrancy in 2025.** The control is a structural one: the pool cannot be called by `policy`, though `policy` stores the pool's address as its controller; its return values can only be treated by the pool as untrusted input; and every fund-moving path is written checks-effects-interactions. Any contract already on the call stack cannot be re-entered under Soroban's host rules either, and that platform rule is treated as a backstop rather than as the control. Every SAC transfer can only be measured against the pool's balance, never assumed.

**Replay, closed by the network.** An approval is a Soroban authorisation entry for `distribute` that the approval multisig signs. That entry is bound by the host to the pool's address, its 4 arguments (recipient, asset, amount and category), the network passphrase, a single-use nonce, and an expiration ledger. A valid approval can therefore only execute once, cannot be used against a different deployment or on another network, and stops being live at its expiry if never used. Approvals are independent of each other: they can execute in any order, and one that is abandoned blocks nothing. An approval for an amount in USDC cannot be presented for EURC once EURC is approved: the asset is bound too. All of this holds for an approval signed as an authorisation entry, the form that the signing tooling builds for every approval. If the approval account were itself the transaction's source account, the approval would be bound through the transaction signature and the account's sequence number instead, with no separate nonce or expiry; that form is one the tooling never builds. This builds every approval as an authorisation entry to keep the nonce and expiry binding.

**Cross-contract trust boundaries.** The address of `policy` is pinned by `zakat_pool` in governance-controlled storage and can only be treated as untrusted. An allocation can only be accepted if the returned vector is exactly 8 elements, each zero or positive, summing exactly to the amount received; a sum-only check is a check that would admit a split with a negative element that corrupts the category accounting while leaving total custody correct. A `policy` can only be accepted, at construction or in a repoint, if it names the pool as its controller. This checks the controller to keep parameter changes behind the timelock. Funds cannot be moved by any defect in `policy`: it makes no call to the pool. Payout approval is a check that involves no cross-contract call: the approval account's authorisation is required by the pool directly.

**Pause, without reintroducing mutable code.** `contribute` can be halted by governance, instantly, and is halted at deployment until the preconditions are verified. `distribute`, the balances, the funds, and the custody Wasm cannot be touched by the pause. Immutable code and an emergency stop are separable properties, and only one of them has to be given up. This separates the emergency stop from mutable code to stop new contributions while remediation runs, rather than leave the pool filling up because the only alternative was an upgrade key. Payouts cannot be paused: a thief holding 2 approval keys pays out in one transaction that a pause could not reliably precede. This omits a payout pause to deny a stolen governance quorum a way to freeze the funds.

**Storage bounds.** Only configuration and the per-asset balance buckets can be persistent across both contracts, with a TTL-bump job on every entry, no unbounded maps, and no per-transaction iteration over anything that grows with usage: the only list read on every call is the set of approved assets, which can only grow by governance transaction. The only other ledger entries the design causes are the network's temporary approval nonces, which expire with their approvals. Nothing that any contract reads can grow with usage, so read limits stay clear as the pool fills.

## Assurance

Independent security review is a step that is requested through the SCF Soroban Audit Bank, the programme that schedules audits for SCF-funded projects with vetted firms.[^20] It can only be entered well before the mainnet deployment, not alongside launch.

**Audit findings remediation.** The programme's rules are a set that puts a twenty business day window on resolving critical, high and medium findings, and publishes the final report once resolution is verified.[^21] This enters review early rather than late to leave at least that much room before the mainnet deployment.

**Fuzzing.** `cargo-fuzz` is the fuzzer that runs on the fund-moving paths, targeting `contribute` and `distribute` specifically: those are the 2 functions where a malformed input becomes a balance error. Fuzzing is a first-class practice in the Stellar toolchain: `arbitrary` support for contract types ships with the SDK, and cargo-fuzz, property tests, and mutation testing are covered by the official guide.[^22]

**Deployment preconditions (verified from the ledger before the pool accepts anything):** Both contracts can only be initialised through `__constructor`, with their configuration passed at deploy, so no separately callable initialiser exists in either binary and an uninitialised contract cannot be captured in any window. The pool's constructor is the one call that requires governance's own authorisation, refuses a delay under 120,960 ledgers, refuses an approval account that is the governance account, and refuses a `policy` that does not name the pool as its controller. The pool is deployed with contributions paused, and contributions can only be resumed by governance after the checks below pass. This keeps the pool closed to guarantee nothing reaches it before the checks do. The multisig properties are account configuration rather than contract code, and cannot be seen by a contract. This verifies them from the ledger to avoid assuming them: the governance account and the approval account are each an account that carries 3 signers, a medium threshold of 2 and a high threshold of 2, with the master key weighted zero. A default-configured Stellar account is one that authorises on a single signature regardless of how many signers it lists, and its high threshold of 0 lets any one signer rewrite the rest, which would make every multisig claim in this document quietly false. Both accounts are read from the ledger by the verify script and must be asserted before governance resumes contributions, and on mainnet each signer key is checked against the holder list published with the deployment. Whose key a signer is cannot be proven from the ledger. This publishes the holder list to make it the commitment.

**Toolchain.** The contracts can only be built with `stellar contract build --locked` inside one pinned image: `stellar/stellar-cli` 27.1.0 with Rust 1.97.1 and the `wasm32v1-none` target, the only Wasm target the Soroban runtime supports, named by digest in `ops/build.conf`.[^23] The digest is the pin that fixes the compiler as well as the CLI, where a version range would not. This records the digest in each Wasm under SEP-58 to let a reviewer rebuild with the same image from the same tag. Dependencies are fixed by `Cargo.lock`. soroban-sdk 27 is the SDK that the contracts use, matching Protocol 27 on mainnet; the SDK's major version tracks the protocol version, so the pin follows the network rather than a release calendar. The pin can only be changed after review, on entering security review and again before mainnet deployment. This reviews it twice to keep pace with Stellar's protocol upgrades, roughly every 2 months. Signing is held to the same discipline. CAP-71 is the second authorisation credential format that Protocol 27 added alongside the original, which it names as a candidate for later removal.[^24] This builds every approval from the preimage each authorisation entry carries, rather than from a hardcoded format, to make a future removal an SDK upgrade rather than a signing rewrite.

The audit report is a document that is published with the mainnet deployment, and every finding can only be closed by a fix or by an acceptance with written reasoning. Both can be read by a reviewer.

> Formal verification and a bug bounty programme can only come after launch. Formal verification of a contract this size is a method that returns less than a thorough manual audit plus fuzzing at this stage, and a bounty with no live value to protect is one that attracts noise rather than researchers. The trigger for a bounty is a pool balance large enough to be worth attacking, the same moment it starts paying for itself. The layered testing that the same sources call for cannot be deferred: unit tests, fuzz tests, and invariant tests over the solvency property above, run in CI rather than at milestones.

## Provenance

The contracts are open source under Apache 2.0, in this repository, and the indexer is the next component that joins them under the same licence, published before security review.

**What is published here:** the source of `zakat_pool` and `policy` with their tests and fuzz targets; the reproducible build and the no-upgrade check; the figure source; and the CI that runs all of it. **Published before entering security review:** the deploy and verify scripts, which read both multisig accounts from the ledger and refuse to continue unless their signers, thresholds and master weight match this document; the signer decoder; and the indexer with its history-archive ingestion. This publishes them before review to give the auditor everything that checks this document. **Published with each deployment:** the contract IDs and Wasm hashes per network.

**Where development happens:** in a private repository held by the delivery team. Published paths can only be changed via the export, which runs one way. Every change to a published path can only be exported here as its own commit, with its original author and date. This exports each commit separately to show a reviewer the history of the code rather than a release snapshot.

**What a reviewer can check without trusting the delivery team:** every deployed contract is a build that carries a tag in this repository, and its Wasm hash can be rebuilt from that tag by the SEP-58 pipeline described under Toolchain. A hash that does not match is a deployment that did not come from published source.

**What a reviewer cannot check here:** the two-person review of each contract change is enforced in the private repository, so only the exported commits and the CI results can be shown in this repository, not the review record. The review record can only be shown to the auditor, during security review.

**The applications are closed source.** The donor, partner and admin applications are interfaces that display what the contracts and the indexer produce, and build transactions for a signer to approve. The guarantees in this document cannot depend on their code: a contribution can be checked by a donor against its transaction hash, and any transaction the pool's rules do not allow is rejected, whoever built it. 2 places are where application code is touched, and they are named rather than left for a reviewer to find. Screening can only be applied via the application, before a contribution is built (Privacy); it is the Foundation's compliance control, not a donor-facing guarantee, and it can be bypassed by a direct contract call by design. A payout can only be seen by an approval signer through an application, so a compromised application could show one recipient and ask for a signature over another. What a signer approves is a standard Soroban authorisation entry for `distribute`. This publishes a decoder to let a signer check the contract, recipient, asset, amount, category and expiry before signing, independently of any application.

## Exclusions

Each line is a decision that carries a stated trigger, rather than something nobody considered. Any line here can be disputed by a reviewer. This writes them down to let a reviewer disagree.

| Not built | Why not, and what would change it |
| --- | --- |
| **In-place code upgrade on custody** | A Soroban contract can replace its own Wasm while the address persists. That entry point is omitted from `zakat_pool`; `policy` can only be repointed, and the approval account only replaced, via the timelock, and neither can hold anything. Custody can only be changed via a new deployment and a visible migration |
| **Stellar Disbursement Platform** | The pilot is a phase that pays a vetted partner organisation directly. This pays the partner directly to remove a live dependency from the first mainnet distribution. The platform is the planned rail for individual payouts and fiat off-ramp in a later phase |
| **Individual-recipient payouts** | They can only be made via SDP, recipient-level onboarding, and an off-ramp, none of which the pilot needs in order to prove the model |
| **Onchain governance voting** | Governance is a multisig that an accountable Swiss foundation holds under federal supervision. A token vote is a mechanism that would be less accountable, not more |
| **Payout pause** | A payout by a thief holding 2 approval keys is one transaction that a pause could not reliably precede, and a payout pause is a power that would let anyone holding governance keys freeze the funds. A stolen approval key can only be answered by replacing the signer on the account |
| **Removing an approved asset** | An approved asset cannot be removed, and a problem with one asset can only be answered by the global contribution pause. A per-asset pause or removal is another governance path that would cross the audit surface, unneeded with one asset. The trigger is a second approved asset |
| **Per-contribution onchain storage** | Events plus an open-source indexer are a pair that gives the same trace without an unbounded storage growth path |
| **Per-partner split** | The pilot is a phase that has one partner organisation, so the category split is the whole allocation. The trigger is a second partner organisation within the same category |
| **zk or decentralised identity** | Off-chain records plus an onchain signed approval is the design that fits this scale. The trigger is a recipient count that makes the registry itself a target |
| **Native mobile applications** | The donor path is a responsive web application. The wallet-app dependency that would otherwise force native is removed by Privy |
| **Multi-chain deployment** | Stellar is the one settlement layer, not one of several. A deployment elsewhere is a split that would fragment the pool, which defeats pooled zakat |
| **Surplus recovery** | A direct transfer to the pool address is a payment that leaves a visible, unspendable surplus. A recovery path is a third value-moving function that the audit scope does not need at pilot size. The trigger is a surplus large enough to matter |
| **Issuer recourse** | Stellar USDC is issued with authorisation revocable, so a holder's balance, a contract's included, can be frozen by Circle. Clawback is not enabled on the asset, so the solvency invariant cannot be violated from outside, but a freeze would halt distribution with no remediation in our bytecode. The risk is accepted rather than designed around at pilot size: detection is an alert that fires on the asset issuer invoking `set_authorized` on the Stellar Asset Contract with the pool's contract address as the target, the response is a contribution pause and an escalation to the issuer, and issuer flags are reviewed as part of approving any asset |
| **Commodity-backed assets** | Nisab is a threshold measured as a weight of gold, but a threshold test for whether zakat is owed does not imply payment in the same asset. A pool holding gold is one that would pass price movement between the obligation and its delivery, and would hand recipients an asset they must sell before they can spend it. Tether Gold is an asset named on the Stellar building blocks roadmap; the trigger is Stellar-native issuance, and it can already be accommodated by per-asset accounting as a governance transaction |
| **Fiat on-ramp** | Pilot donors are donors who arrive holding USDC. A franc-only donor can only be reached via a SEP-24 anchor integration, triggered by demonstrated pilot demand from donors who cannot self-fund |
| **Standing Shariah board** | The pilot parameter set is validated by one qualified advisor. A board is a body that is planned for scale beyond the pilot partner, and cannot be claimed as present |
| **Open-source applications** | The applications are code that carries none of the guarantees in this document, so publishing them would add review surface without adding anything a donor or auditor can verify (Provenance). The trigger is a guarantee here that comes to depend on application code |

## Building blocks

CipherPhi is a composition that places vetted Stellar infrastructure around 2 custom contracts rather than rebuilding it. The status column is the column that separates what is funded from what is not, and that separation is kept everywhere in this document.

The indexer is a service that depends on one property in particular: the Stellar Asset Contract implements SEP-41, so asset movements surface the same standardised transfer events whether they originate from a payment operation or from a contract call, and the whole trail can be observed through one uniform interface.[^25]

| Building block | Role in CipherPhi | Status |
| --- | --- | --- |
| Soroban smart contracts | 2 contracts: pooled custody and allocation policy. The core IP | Committed |
| Stellar Asset Contract, SEP-41 | Holds and moves USDC, later EURC, with no custom token logic | Committed |
| Stellar authorisation and multisig | `require_auth` role separation. Governance and the approval multisig as Foundation 2-of-3 accounts on separate keys, thresholds enforced by the network | Committed |
| Stellar ledger events and RPC | The donor proof view, reconstructed from ledger events by an open-source indexer | Committed |
| Privy | The embedded email and social wallet that lets a donor holding stablecoins contribute without managing a seed phrase | Committed |
| SCF Soroban Audit Bank | Independent security review of the contracts before mainnet | Committed |
| Stellar Disbursement Platform | The rail that pays individual recipients and off-ramps to fiat once the pilot expands beyond partner organisations | Planned |
| EURC via SAC | The second approved asset that carries its own category buckets, added by governance transaction | Planned |
| Tether Gold (XAUT) | A gold-denominated asset that carries its own category buckets, contingent on Stellar-native issuance; wrapped or bridged representations cannot be approved | Planned |
| SEP-24 anchor | The fiat on-ramp that lets a donor holding only francs or euros acquire USDC on Stellar | Planned |
| Sponsored reserves | The reserves that CipherPhi pays for accounts and trustlines, so a donor never has to hold XLM | Committed |
| Circle CCTP | Native inbound USDC from other chains, terminating before CipherPhi's code | Live on Stellar |

---

## References

[^1]: HAQQ, Evergreen DAO endowment mechanism. [haqq.network/shariah-compliance-en](https://haqq.network/shariah-compliance-en)
[^2]: Zakat Coin, donor-facing zakat platform. [zktcoin.com](https://zktcoin.com)
[^3]: A blockchain based decentralized zakat collection and distribution platform. [dl.acm.org/doi/10.1145/3641067.3641071](https://dl.acm.org/doi/10.1145/3641067.3641071)
[^4]: CCTP on Stellar, maintained by Circle. [developers.stellar.org/docs/tokens/cross-chain-transfers](https://developers.stellar.org/docs/tokens/cross-chain-transfers)
[^5]: Resource limits, fees and metering. [developers.stellar.org/docs/networks/resource-limits-fees](https://developers.stellar.org/docs/networks/resource-limits-fees)
[^6]: Stellar Disbursement Platform. [developers.stellar.org/docs/platforms/stellar-disbursement-platform](https://developers.stellar.org/docs/platforms/stellar-disbursement-platform)
[^7]: Upgrading contract Wasm in place, and the system event it emits. [developers.stellar.org/docs/build/guides/conventions/upgrading-contracts](https://developers.stellar.org/docs/build/guides/conventions/upgrading-contracts)
[^8]: Soroban state archival and TTL. [developers.stellar.org/docs/learn/fundamentals/contract-development/storage/state-archival](https://developers.stellar.org/docs/learn/fundamentals/contract-development/storage/state-archival)
[^9]: Soroban storage types and expiry behaviour. [developers.stellar.org/docs/build/guides/storage/choosing-the-right-storage](https://developers.stellar.org/docs/build/guides/storage/choosing-the-right-storage)
[^10]: Contract events, emission and retention. [developers.stellar.org/docs/build/guides/events](https://developers.stellar.org/docs/build/guides/events)
[^11]: Transaction apply order is computed after consensus and shuffles the set. [developers.stellar.org/docs/learn/fundamentals/transactions/transaction-lifecycle](https://developers.stellar.org/docs/learn/fundamentals/transactions/transaction-lifecycle)
[^12]: Stellar Asset Contract and SEP-41. [developers.stellar.org/docs/tokens/stellar-asset-contract](https://developers.stellar.org/docs/tokens/stellar-asset-contract)
[^13]: SEP-41 token interface specification. [github.com/stellar/stellar-protocol/blob/master/ecosystem/sep-0041.md](https://github.com/stellar/stellar-protocol/blob/master/ecosystem/sep-0041.md)
[^14]: CCTP and USDC contract addresses on Stellar. [developers.circle.com/cctp/references/stellar](https://developers.circle.com/cctp/references/stellar)
[^15]: Sponsored reserves and fee-bump transactions. [developers.stellar.org/docs/learn/encyclopedia/transactions-specialized/sponsored-reserves](https://developers.stellar.org/docs/learn/encyclopedia/transactions-specialized/sponsored-reserves)
[^16]: Stellar wallet integrations, including Privy. [developers.stellar.org/docs/tools/developer-tools/wallets](https://developers.stellar.org/docs/tools/developer-tools/wallets)
[^17]: Privy embedded wallet to a funded testnet contribution, working example. [github.com/bits-and-blocks/stellar-examples](https://github.com/bits-and-blocks/stellar-examples/tree/main/examples/privy-stellar-onboarding)
[^18]: Swiss Zakat Foundation methodology. [zakat.ch/knowledge-base/faq](https://zakat.ch/knowledge-base/faq)
[^19]: OWASP Smart Contract Top 10 for 2026, from 2025 incident data. [owasp.org/www-project-smart-contract-top-10](https://owasp.org/www-project-smart-contract-top-10)
[^20]: Soroban Security Audit Bank. [stellar.org/grants-and-funding/soroban-audit-bank](https://stellar.org/grants-and-funding/soroban-audit-bank)
[^21]: Audit Bank rules, remediation and publication. [stellar.gitbook.io/scf-handbook/supporting-programs/audit-bank/official-rules](https://stellar.gitbook.io/scf-handbook/supporting-programs/audit-bank/official-rules)
[^22]: Fuzzing Soroban contracts, official guide. [developers.stellar.org/docs/build/guides/testing/fuzzing](https://developers.stellar.org/docs/build/guides/testing/fuzzing)
[^23]: Soroban Rust SDK, supported target and build guidance. [github.com/stellar/rs-soroban-sdk](https://github.com/stellar/rs-soroban-sdk)
[^24]: CAP-71-02, address-bound Soroban address credentials. [github.com/stellar/stellar-protocol/blob/master/core/cap-0071-02.md](https://github.com/stellar/stellar-protocol/blob/master/core/cap-0071-02.md)
[^25]: Standardised asset events under SEP-41. [developers.stellar.org/docs/tokens/anatomy-of-an-asset](https://developers.stellar.org/docs/tokens/anatomy-of-an-asset)
