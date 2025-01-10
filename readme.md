# bob.fun

## build

```bash
docker build -t rust-dev .
docker run -v $(pwd):/app -it rust-dev bash /app/build.sh
sha256sum target/wasm32-unknown-unknown/release/bob-minter-v2.wasm.gz
sha256sum target/wasm32-unknown-unknown/release/bob_miner_v2.wasm
```

bob Ledger forged from the source of truth, the DFINITY https://github.com/dfinity/ic, commit 2190613d3b5bcd9b74c382b22d151580b8ac271a.

https://download.dfinity.systems/ic/2190613d3b5bcd9b74c382b22d151580b8ac271a/canisters/ic-icrc1-ledger.wasm.gz
