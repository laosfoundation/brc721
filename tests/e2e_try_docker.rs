use testcontainers::{core::WaitFor, runners::SyncRunner, Image};

#[derive(Debug, Default)]
pub struct HelloWorld;

impl Image for HelloWorld {
    fn name(&self) -> &str {
        "hello-world"
    }

    fn tag(&self) -> &str {
        "latest"
    }

    fn ready_conditions(&self) -> Vec<WaitFor> {
        vec![WaitFor::message_on_stdout("Hello from Docker!")]
    }
}

#[test]
fn sync_can_run_hello_world() {
    let _container = HelloWorld.start();
}
