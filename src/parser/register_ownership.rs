use crate::storage::traits::StorageWrite;
use crate::types::{Brc721Command, Brc721Error, Brc721Payload, Brc721Tx};

pub fn digest<S: StorageWrite>(
    brc721_tx: &Brc721Tx<'_>,
    _storage: &S,
    block_height: u64,
    tx_index: u32,
) -> Result<(), Brc721Error> {
    let Brc721Payload::RegisterOwnership(payload) = brc721_tx.payload() else {
        return Err(Brc721Error::TxError(
            "expected RegisterOwnership message".to_string(),
        ));
    };

    log::error!(
        "register-ownership not supported yet (block {} tx {}, collection {}:{}, groups={})",
        block_height,
        tx_index,
        payload.collection_height,
        payload.collection_tx_index,
        payload.groups.len()
    );
    Err(Brc721Error::UnsupportedCommand {
        cmd: Brc721Command::RegisterOwnership,
    })
}
