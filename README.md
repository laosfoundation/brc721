## BRC721: Scalable Non-Fungible Tokens on Bitcoin

This project implements the BRC721 protocol, also referred to as the **Bridgeless or Bitcoin ERC721** standard, which is designed for enabling the secure and scalable management of **Non-Fungible Tokens (NFTs) on Bitcoin**. For the complete cryptographic specification see the [BRC721 research paper](https://eprint.iacr.org/2025/641).

The core idea of BRC721 is to provide a scalable solution for the creation, management, and trading of NFTs on Bitcoin while maintaining a **minimal on-chain footprint**. This approach significantly improves upon methods like Inscriptions, which face inherent limitations in scalability and cost due to Bitcoin's block size constraints.

BRC721 achieves this efficiency and scalability by leveraging a **dual-consensus system** based on the **Bridgeless Minting pattern**:

1.  **Ownership on Bitcoin:** All aspects of token ownership, including trading, lending, and transfers, remain **fully on-chain** within the Bitcoin network. Ownership relies on the Bitcoin UTXO structure and requires explicit Bitcoin signatures from rightful owners. To minimize space usage on Bitcoin, ownership records utilize the **OP RETURN mechanism**.
2.  **Metadata on LAOS:** The bulk of the data, particularly asset metadata, is offloaded to a separate, verifiable consensus system: the **LAOS Network**. LAOS is an Ethereum Virtual Machine (EVM)-compatible blockchain built as a Parachain on Polkadot, which provides programmability for managing NFT metadata.

This architecture ensures that the protocol follows an **always-on-chain approach**, providing strong guarantees for Data Availability (DA) and the prevention of invalid transactions by leveraging the security of both Bitcoin and the LAOS Network (which is secured by Polkadot’s relay chain). BRC721 tokens are compatible with existing Bitcoin wallets, simplifying user adoption.

***

*A simple analogy for BRC721’s architecture is that of a secure bank vault and an archive library. Bitcoin acts as the bank vault, securely holding the title deeds (ownership) for the NFT using its robust security mechanisms. Meanwhile, the LAOS Network acts as the external, programmable archive library, storing the detailed content and rules (metadata) associated with that title deed. The system ensures that the title deed always points directly to the correct content, even though the content itself is stored elsewhere, maximizing efficiency while maintaining high security.*

For detailed operational instructions, setup steps, and protocol references, consult the [project wiki](https://github.com/laosfoundation/brc721/wiki).

**Disclaimer:** This is experimental software released under the GPLv3 License (`LICENSE`) and comes with no warranties or guarantees of any kind.
