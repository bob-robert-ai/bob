# Alice and Bob: Autonomous Agents on ICP

Welcome to the Alice and Bob projects, two innovative AI-powered agents built on the Internet Computer Protocol (ICP). These agents leverage decentralized infrastructure and advanced AI models to create a dynamic, on-chain ecosystem.

---

## build

```bash
docker build -t rust-dev .
docker run -v $(pwd):/app -it rust-dev bash /app/build.sh
sha256sum target/wasm32-unknown-unknown/release/bob_minter_v2.wasm.gz
sha256sum target/wasm32-unknown-unknown/release/bob_miner_v2.wasm
sha256sum target/wasm32-unknown-unknown/release/alice.wasm.gz
```

---

## **Alice: The Playful AI Agent**
Alice is an autonomous AI agent that operates entirely on-chain, combining real-time data analysis with a playful personality. Powered by Large Language Models (LLMs) like DeepSeek, Alice makes intelligent decisions about token trading, mining, and governance—all while injecting a touch of sarcasm and humor into the process.

### Key Features:
- **On-Chain Decision Making**: Fetches data from decentralized exchanges (DEXs) like ICPSwap and integrates external AI models via HTTPS outcalls.
- **Token Management**: Buys, sells, and burns tokens (e.g., ALICE) based on market conditions.
- **BOB Mining**: Mines BOB, a Bitcoin-inspired protocol that burns ICP cycles, adding value to the ecosystem.
- **Personality-Driven**: A unique, entertaining AI with a playful and sarcastic demeanor.

---

## **BOB: The Bitcoin-Inspired Protocol**
BOB is a decentralized protocol inspired by Bitcoin, designed to turn ICP cycles into a store of value. By burning the majority of cycles on ICP, BOB creates scarcity and enhances the value of the ecosystem.

### Key Features:
- **Cycle Burning**: Burns ICP cycles to create a deflationary mechanism.
- **Store of Value**: Provides a decentralized, Bitcoin-like store of value within the ICP ecosystem.
- **Integration with Alice**: Works seamlessly with Alice, enabling autonomous mining and cycle management.

---

## **Why Alice and Bob?**
- **Decentralized and Autonomous**: Both agents operate entirely on-chain, ensuring transparency and trustlessness.
- **AI-Powered Intelligence**: Leverage cutting-edge AI models for real-time decision-making.
- **Ecosystem Growth**: Drive value and innovation within the ICP ecosystem through token management, mining, and governance.

---

## Getting Started
To interact with Alice and BOB, visit their respective canisters on the ICP network. For developers, check out the [documentation](#) to learn how to integrate these agents into your projects.

---

## Contributing
We welcome contributions! If you'd like to improve Alice, BOB, or their ecosystem, please follow our [contribution guidelines](#).

---

Alice and Bob are redefining the future of decentralized AI and blockchain ecosystems. Join us in building a smarter, more entertaining, and value-driven future on ICP!