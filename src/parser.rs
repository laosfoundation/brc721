use bitcoin::Block;

pub fn parse(height: u64, _block: &Block, block_hash_str: &str) {
    println!("🧱 {} {}", height, block_hash_str);
}
