use crate::bitcoin_rpc::BitcoinRpc;
use crate::parser::BlockParser;
use crate::scanner::Scanner;
use crate::storage::traits::{Storage, StorageTx};
use anyhow::Result;
use bitcoin::Block;

pub struct Core<C: BitcoinRpc, S: Storage, P: BlockParser<S::Tx>> {
    scanner: Scanner<C>,
    storage: S,
    parser: P,
}

impl<C: BitcoinRpc, S: Storage, P: BlockParser<S::Tx>> Core<C, S, P> {
    pub fn new(scanner: Scanner<C>, storage: S, parser: P) -> Self {
        Self {
            scanner,
            storage,
            parser,
        }
    }

    /// Main loop: keep stepping until shutdown is requested.
    pub fn run(&mut self, shutdown: tokio_util::sync::CancellationToken) -> Result<()> {
        while !shutdown.is_cancelled() {
            self.step(&shutdown)?;
        }
        log::info!("👋 Core loop exited");
        Ok(())
    }

    /// One iteration: ask the scanner for blocks, process them, or back off on error.
    pub fn step(&mut self, shutdown: &tokio_util::sync::CancellationToken) -> Result<()> {
        match self.scanner.next_blocks_with_shutdown(shutdown) {
            Ok(blocks) => {
                if blocks.is_empty() {
                    return Ok(());
                }
                let tx = self.storage.begin_tx()?;
                for (height, block) in blocks {
                    self.process_block(&tx, height, &block)?;
                }
                tx.commit()?;
            }
            Err(e) => {
                log::error!("scanner error: {}", e);
                return Err(e.into());
            }
        }
        Ok(())
    }

    fn process_block(&self, tx: &S::Tx, height: u64, block: &Block) -> Result<()> {
        let hash = block.block_hash();
        log::info!("🧱 block={} 🧾 hash={}", height, hash);

        if let Err(e) = self
            .parser
            .parse_block(tx, block, height, self.scanner.rpc())
        {
            log::error!(
                "parsing error of block {} at height {}: {}",
                hash,
                height,
                e
            );
            return Err(e.into());
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::storage::traits::{
        Collection, CollectionKey, OwnershipRange, OwnershipUtxo, OwnershipUtxoSave, StorageRead,
        StorageWrite,
    };
    use crate::types::Brc721Error;
    use bitcoin::blockdata::constants::genesis_block;
    use bitcoin::Network;
    use bitcoincore_rpc::Error as RpcError;
    use ethereum_types::H160;

    #[derive(Clone)]
    struct DummyRpc;

    impl crate::bitcoin_rpc::BitcoinRpc for DummyRpc {
        fn get_block_count(&self) -> Result<u64, RpcError> {
            unimplemented!()
        }

        fn get_block_hash(&self, _height: u64) -> Result<bitcoin::BlockHash, RpcError> {
            unimplemented!()
        }

        fn get_block(&self, _hash: &bitcoin::BlockHash) -> Result<bitcoin::Block, RpcError> {
            unimplemented!()
        }

        fn get_raw_transaction(
            &self,
            _txid: &bitcoin::Txid,
        ) -> Result<bitcoin::Transaction, RpcError> {
            unimplemented!()
        }

        fn wait_for_new_block(&self, _timeout: u64) -> Result<(), RpcError> {
            unimplemented!()
        }
    }

    fn empty_block() -> Block {
        genesis_block(Network::Regtest)
    }

    #[derive(Clone)]
    struct DummyStorage;

    impl StorageRead for DummyStorage {
        fn load_last(&self) -> Result<Option<crate::storage::Block>> {
            Ok(None)
        }
        fn load_collection(&self, _id: &CollectionKey) -> Result<Option<Collection>> {
            Ok(None)
        }
        fn list_collections(&self) -> Result<Vec<Collection>> {
            Ok(vec![])
        }

        fn list_unspent_ownership_utxos_by_outpoint(
            &self,
            _reg_txid: &str,
            _reg_vout: u32,
        ) -> Result<Vec<OwnershipUtxo>> {
            Ok(vec![])
        }

        fn list_ownership_ranges(&self, _utxo: &OwnershipUtxo) -> Result<Vec<OwnershipRange>> {
            Ok(vec![])
        }

        fn find_unspent_ownership_utxo_for_slot(
            &self,
            _collection_id: &CollectionKey,
            _base_h160: H160,
            _slot: u128,
        ) -> Result<Option<OwnershipUtxo>> {
            Ok(None)
        }

        fn list_unspent_ownership_utxos_by_owner(
            &self,
            _owner_h160: H160,
        ) -> Result<Vec<OwnershipUtxo>> {
            Ok(vec![])
        }
    }

    impl StorageWrite for DummyStorage {
        fn save_last(&self, _height: u64, _hash: &str) -> Result<()> {
            Ok(())
        }
        fn save_collection(
            &self,
            _key: CollectionKey,
            _evm_collection_address: H160,
            _rebaseable: bool,
        ) -> Result<()> {
            Ok(())
        }

        fn save_ownership_utxo(&self, _utxo: OwnershipUtxoSave<'_>) -> Result<()> {
            Ok(())
        }

        fn save_ownership_range(
            &self,
            _reg_txid: &str,
            _reg_vout: u32,
            _collection_id: &CollectionKey,
            _base_h160: H160,
            _slot_start: u128,
            _slot_end: u128,
        ) -> Result<()> {
            Ok(())
        }

        fn mark_ownership_utxo_spent(
            &self,
            _reg_txid: &str,
            _reg_vout: u32,
            _spent_txid: &str,
            _spent_height: u64,
            _spent_tx_index: u32,
        ) -> Result<()> {
            Ok(())
        }
    }

    impl StorageTx for DummyStorage {
        fn commit(self) -> Result<()> {
            Ok(())
        }
    }

    impl Storage for DummyStorage {
        type Tx = DummyStorage;
        fn begin_tx(&self) -> Result<Self::Tx> {
            Ok(DummyStorage)
        }
    }

    fn make_core_with_parser<P: BlockParser<DummyStorage>>(
        parser: P,
    ) -> Core<DummyRpc, DummyStorage, P> {
        let rpc = DummyRpc;
        let scanner = Scanner::new(rpc);
        let storage = DummyStorage;
        Core::new(scanner, storage, parser)
    }

    struct FailingParser;

    impl BlockParser<DummyStorage> for FailingParser {
        fn parse_block<R: crate::bitcoin_rpc::BitcoinRpc>(
            &self,
            _tx: &DummyStorage,
            _block: &Block,
            _height: u64,
            _rpc: &R,
        ) -> Result<(), Brc721Error> {
            Err(Brc721Error::InvalidPayload)
        }
    }

    #[test]
    fn process_block_parser_error_returns_error() {
        let core = make_core_with_parser(FailingParser);

        let block = empty_block();
        let height = 7;
        let tx = DummyStorage;
        assert!(core.process_block(&tx, height, &block).is_err());
    }
}
