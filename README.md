# CipherPhi

Onchain zakat collection and redistribution on Stellar.

Donors calculate and pay their zakat into a pooled Soroban contract, and can verify from the Stellar ledger that their category was distributed to a recipient whose payout the Foundation's signers approved before the funds moved. Zakat is pooled by construction, so the claim is category-level, and the architecture keeps it that way throughout.

CipherPhi is the onchain layer for the Swiss Zakat Foundation, a federally supervised Swiss foundation collecting and redistributing zakat since its founding deed of 6 July 2021, audited annually and entirely community funded.

## Status

SCF #46 Build Award submission, Open Track, filed 8 November 2026. Community vote 23 November to 3 December 2026.

> Bits & Blocks has published working examples covering the integrations this build depends on. Read more: [bits-and-blocks/stellar-examples](https://github.com/bits-and-blocks/stellar-examples).

## Architecture

CipherPhi is composed of two Soroban contracts and two Foundation multisigs:

- `zakat_pool` holds custody and the per-asset eight-category balances, and has the only two functions that move value.
- `policy` holds the Shariah parameters and computes the allocation.
- The approval multisig, a 2-of-3 Stellar account, authorises each payout.
- The governance multisig sets the rules under a public timelock, and cannot reach the funds.

> Read the full design: [ARCHITECTURE.md](./ARCHITECTURE.md).

## Testnet

The revision 5 contracts run on Stellar testnet with Circle's testnet USDC, built reproducibly from this repository and checked against the network. They are unaudited, handle no real funds, and every key is a test key.

| Contract | Address | Wasm hash |
| --- | --- | --- |
| `zakat_pool` | [`CBMPFDAAAYURRIMTZN76IGNLSP2JPKTPPCBXUBD7TXKFYVI2FYYF6BAW`](https://stellar.expert/explorer/testnet/contract/CBMPFDAAAYURRIMTZN76IGNLSP2JPKTPPCBXUBD7TXKFYVI2FYYF6BAW) | `100459f23aaa84435dd1c94a2c67055669021cc21a73bafb2e11d9ff16ce9786` |
| `policy` | [`CBXBPITVYR5M6QPZAHV6QCAJO2LCS547K3OVBTVJVD6Z742OUHFTSKCU`](https://stellar.expert/explorer/testnet/contract/CBXBPITVYR5M6QPZAHV6QCAJO2LCS547K3OVBTVJVD6Z742OUHFTSKCU) | `9ba5e7602e314a60b881ded49795ee342593a4cc234fe6c9680868f5cacc3f92` |

Governance [`GCL2VC…PLXX`](https://stellar.expert/explorer/testnet/account/GCL2VCSOWEF6XUCBCD6TQ5URQAE3R4NAGRPEBZ7VGKPGMMQI6VM3PLXX) and the approval multisig [`GCP4SH…2BMV`](https://stellar.expert/explorer/testnet/account/GCP4SHHS7A2JA4KH6S4V3UTEDI263Z34RHSF6TE63DUAMTVN2XYG2BMV) are 2-of-3 accounts with the master key at weight 0. The pool's one approved recipient is the test partner [`GD6O4N…3NQR`](https://stellar.expert/explorer/testnet/account/GD6O4NBIFK2PVUM2CMJIPYX5PWBM74GXF4ASXYLAQRO4C7IA5WJI3NQR). The `policy` split and parameters are placeholders, not validated by the Shariah authority.

A clickable demo of give, split, release and check still runs the revision 3 design on its own pool, [`CD3DL4GTEFZMJUTZT4GXA5ZNPLFXNFYS4LHQQYU2ZKJ3DJJOJO6XISGU`](https://stellar.expert/explorer/testnet/contract/CD3DL4GTEFZMJUTZT4GXA5ZNPLFXNFYS4LHQQYU2ZKJ3DJJOJO6XISGU), Wasm hash `f498a9c5d440ddeee3da70f37b14f95044647b586a01c816a98c24d084072677`. It has no allow-list.

## Revisions

The architecture has been through five review cycles. The first three left the structure as first submitted and changed what the design was run against, security review and platform checks, and the corrections those found. The fourth locked the architecture and simplified it: payout approval moved from a separate contract and a distributor key to a Foundation multisig the network checks directly. This moves approval to the Foundation's own signers to match how a Swiss foundation signs.

| ID | Logs | Branches |
| --- | --- | --- |
| 1 | The design as submitted with the SCF #45 Build Award application. | Merged directly to `main` |
| 2 | Findings of an internal pre-implementation security review folded in, the review's attribution and its internal scope stated, references turned into real footnotes, and the CAP-71 credential migration named in the toolchain pin. | [`docs/architecture-security-review`](https://github.com/bits-and-blocks/cipherphi-smart-contracts/tree/docs/architecture-security-review), [`docs/review-claim-accuracy`](https://github.com/bits-and-blocks/cipherphi-smart-contracts/tree/docs/review-claim-accuracy), [`docs/cap-71-credentials`](https://github.com/bits-and-blocks/cipherphi-smart-contracts/tree/docs/cap-71-credentials), [`docs/readme-and-license`](https://github.com/bits-and-blocks/cipherphi-smart-contracts/tree/docs/readme-and-license) |
| 3 | Corrupted text and inconsistencies repaired, a platform correction on Stellar transaction ordering, monitoring and issuer-freeze detection made concrete, and figures added. | [`docs/architecture-revision-3`](https://github.com/bits-and-blocks/cipherphi-smart-contracts/tree/docs/architecture-revision-3), [`rev-3/transaction-ordering-rationale`](https://github.com/bits-and-blocks/cipherphi-smart-contracts/tree/rev-3/transaction-ordering-rationale) |
| 4 | Architecture locked. Payout approval by a Foundation 2-of-3 multisig checked by the network, replacing the `attestation` contract, the attestation struct and the distributor key. Governance made Foundation-only 2-of-3, with the approval multisig able to cancel any pending change. The pool deploys with contributions paused until both multisigs are verified, and checks its `policy` controller at construction. Storage, function, parameter, toolchain and provenance sections brought in line with the code, and the figures updated from their Excalidraw source, now kept in `static/figures.excalidraw`. | [`rev-4/architecture-lock`](https://github.com/bits-and-blocks/cipherphi-smart-contracts/tree/rev-4/architecture-lock) |
| 5 | Recipient allow-list on `zakat_pool`. A payout can only reach a recipient that governance approved, at deployment or under the timelock, so stolen approval keys can no longer pay an address the thief picks. The approval multisig can remove a recipient at once, which also voids approvals signed before the removal. New errors continue from 40, leaving 30 to 39 to `policy`. Every read-modify-write now goes through `try_update`, and a missing configuration entry fails with a typed error instead of a bare panic. Needs a new deployment, as `zakat_pool` has no upgrade path. | [`rev-5/recipient-allowlist`](https://github.com/bits-and-blocks/cipherphi-smart-contracts/tree/rev-5/recipient-allowlist) |

## Stack

- Stellar Soroban, Rust to Wasm on the soroban-sdk 27
- Stellar Asset Contract custody, USDC first with EURC planned
- Stellar multisig accounts for governance and payout approval, 2 of 3 Foundation signers each, with thresholds enforced by the network
- Privy embedded wallets, with the Stellar Wallets Kit as the named fallback
- Sponsored reserves covering donor accounts and trustlines
- Circle CCTP for native inbound USDC

Planned but not committed:

- Stellar Disbursement Platform for individual-recipient payouts
- SEP-24 anchor for fiat on-ramp
- Tether Gold contingent on Stellar-native issuance

## Team

- [Swiss Zakat Foundation](https://zakat.ch), the operating institution: Saâd Dhif, Founder and Executive Director.
- [Bits & Blocks](https://www.bitsandblocks.tech), delivery: Aladdin Battikh, CTO.
- [Muhammad Emamally](https://www.linkedin.com/in/phil-emamally-/), Shariah authority for the pilot.
- [Ismail Amara](https://www.linkedin.com/in/ismail-a-667666111/), partnerships and pilot.

## License

The smart contracts in [`contracts/`](./contracts) and their reproducible build in [`ops/`](./ops) are licensed under Apache 2.0, see [`contracts/LICENSE`](./contracts/LICENSE). Nothing else in this repository can be reused under a licence.
