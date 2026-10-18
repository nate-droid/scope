//! Run with: cargo run -p scope-ide --features logging --example native_logging
use cell_capability_log::Logger;
use scope_ide::logging::{LoggingBinding, LoggingError};
use std::sync::{Arc, Mutex};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let messages = Arc::new(Mutex::new(Vec::<String>::new()));
    let captured = messages.clone();
    let logger = Logger::new(move |message| {
        captured.lock().unwrap().push(message.to_owned());
        Ok(())
    });
    let (binding, controller) = LoggingBinding::new()?;
    let consumer = binding.clone();
    assert!(matches!(
        consumer.log_info("42"),
        Err(LoggingError::Denied { .. })
    ));
    controller.authorize(logger, "host-memory")?;
    let receipt = consumer.log_info("42")?;
    assert_eq!(receipt.interface(), "cell:log/log@0.1.0");
    assert_eq!(receipt.implementation(), "host-memory");
    controller.revoke()?;
    assert!(matches!(
        consumer.log_info("42"),
        Err(LoggingError::Denied { .. })
    ));
    assert_eq!(*messages.lock().unwrap(), vec!["42"]);
    Ok(())
}
