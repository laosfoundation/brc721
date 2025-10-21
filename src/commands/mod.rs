pub trait CommandRunner {
    async fn run(self) -> anyhow::Result<()>;
}

pub mod wallet;
