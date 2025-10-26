#[cfg(test)]
mod tests {
    use corepc_node::{Conf, Node};

    #[test]
    fn test_corepc_node() {
        let conf = Conf::default();
        let bitcoind = Node::with_conf(corepc_node::downloaded_exe_path().unwrap(), &conf).unwrap();
        let client = &bitcoind.client;
        let info = client.get_blockchain_info().unwrap();
        assert_eq!(info.chain, "regtest");
    }
}
