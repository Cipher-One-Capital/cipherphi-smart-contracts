# Reproducible build

Builds `zakat_pool` and `policy` from this repository's source in a digest-pinned container. This builds from source in a pinned container to let anyone check that a deployed contract runs exactly this code. The contracts can only be used on testnet, are unaudited, and cannot be used with real funds.

```bash
bash ops/scripts/build.sh                # Writes ops/out/*.wasm and ops/out/hashes.json
node ops/scripts/check-no-upgrade.mjs    # Checks the Wasm to confirm that zakat_pool cannot replace its code
```

The image is pinned by digest in `build.conf`, and SEP-58 metadata is recorded in each Wasm. This packs the whole workspace into one archive to give both Wasm files the same `source_sha256`. The archive is not committed and can only be rebuilt from git, and the fuzz crate is left out of it.

A deployed contract can be checked by fetching its Wasm from the network and comparing its hash with the matching one in `ops/out/hashes.json`:

```bash
stellar contract fetch --id <contract id> --network testnet --out-file deployed.wasm
shasum -a 256 deployed.wasm
```

The fuzz targets that CI runs on Linux are in `contracts/fuzz`:

```bash
cd contracts/fuzz && cargo +nightly fuzz run --sanitizer none contribute
```

The fuzz targets cannot be linked on macOS because of the contracts' `cdylib` crate type, and can only be run there in a Linux container.
