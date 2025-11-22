use crate::parser::BlockParser;
use crate::scanner::{BitcoinRpc, Scanner};
use anyhow::Result;
use bitcoin::Block;

pub struct Core<C: BitcoinRpc, P: BlockParser> {
    scanner: Scanner<C>,
    parser: P,
}

impl<C: BitcoinRpc, P: BlockParser> Core<C, P> {
    pub fn new(scanner: Scanner<C>, parser: P) -> Self {
        Self { scanner, parser }
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
                for (height, block) in blocks {
                    self.process_block(height, &block)?;
                }
            }
            Err(e) => {
                log::error!("scanner error: {}", e);
                return Err(e.into());
            }
        }
        Ok(())
    }

    fn process_block(&self, height: u64, block: &Block) -> Result<()> {
        let hash = block.block_hash();
        log::info!("🧱 block={} 🧾 hash={}", height, hash);

        if let Err(e) = self.parser.parse_block(block, height) {
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
    use crate::types::Brc721Error;
    use bitcoin::blockdata::constants::genesis_block;
    use bitcoin::Network;
    use bitcoincore_rpc::Error as RpcError;

    #[derive(Clone)]
    struct DummyRpc;

    impl crate::scanner::BitcoinRpc for DummyRpc {
        fn get_block_count(&self) -> Result<u64, RpcError> {
            unimplemented!()
        }

        fn get_block_hash(&self, _height: u64) -> Result<bitcoin::BlockHash, RpcError> {
            unimplemented!()
        }

        fn get_block(&self, _hash: &bitcoin::BlockHash) -> Result<bitcoin::Block, RpcError> {
            unimplemented!()
        }

        fn wait_for_new_block(&self, _timeout: u64) -> Result<(), RpcError> {
            unimplemented!()
        }
    }

    fn empty_block() -> Block {
        genesis_block(Network::Regtest)
    }

    fn make_core_with_parser<P: BlockParser>(parser: P) -> Core<DummyRpc, P> {
        let rpc = DummyRpc;
        let scanner = Scanner::new(rpc);
        Core::new(scanner, parser)
    }

    struct FailingParser;

    impl BlockParser for FailingParser {
        fn parse_block(&self, _block: &Block, _height: u64) -> Result<(), Brc721Error> {
            Err(Brc721Error::InvalidPayload)
        }
    }

    #[test]
    fn process_block_parser_error_returns_error() {
        let core = make_core_with_parser(FailingParser);

        let block = empty_block();
        let height = 7;
        assert!(core.process_block(height, &block).is_err());
    }
}
