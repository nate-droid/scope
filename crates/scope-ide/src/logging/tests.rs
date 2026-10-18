use super::*;
use cell_capability_log::LogError;
use std::{
    panic::{AssertUnwindSafe, catch_unwind},
    sync::mpsc,
    thread,
    time::Duration,
};

fn recording_logger() -> (Logger, Arc<Mutex<Vec<String>>>) {
    let messages = Arc::new(Mutex::new(Vec::new()));
    let captured = messages.clone();
    let logger = Logger::new(move |message| {
        captured.lock().unwrap().push(message.to_owned());
        Ok(())
    });
    (logger, messages)
}

fn assert_receipt(receipt: &LoggingReceipt, metadata: &LoggingMetadata) {
    assert_eq!(receipt.interface(), LOG_INTERFACE);
    assert_eq!(receipt.operation(), "log_info");
    assert_eq!(receipt.context(), metadata.context);
    assert_eq!(receipt.revision(), metadata.revision);
    assert_eq!(
        Some(receipt.implementation()),
        metadata.implementation.as_deref()
    );
}

#[test]
fn new_and_cloned_bindings_are_denied_and_share_context() {
    let (binding, _controller) = LoggingBinding::new().unwrap();
    let cloned = binding.clone();
    let metadata = binding.inspect().unwrap();
    assert_eq!(metadata.revision, 0);
    assert_eq!(metadata.implementation, None);
    assert_eq!(cloned.inspect().unwrap(), metadata);
    for handle in [&binding, &cloned] {
        assert_eq!(
            handle.log_info("not authorized"),
            Err(LoggingError::Denied {
                context: metadata.context,
                revision: 0,
            })
        );
    }
    let (independent, _) = LoggingBinding::new().unwrap();
    assert_ne!(independent.inspect().unwrap().context, metadata.context);
}

#[test]
fn forwards_exact_messages_and_returns_admission_receipts() {
    let (binding, controller) = LoggingBinding::new().unwrap();
    let cloned = binding.clone();
    let (logger, messages) = recording_logger();
    controller.authorize(logger, "memory").unwrap();
    let metadata = binding.inspect().unwrap();
    assert_eq!(metadata.revision, 1);
    assert_eq!(metadata.implementation.as_deref(), Some("memory"));
    let inputs = [
        "",
        "  leading and trailing  ",
        "line one\nline two\r\n",
        "Unicode: λ 🦀\0end",
    ];
    for message in inputs {
        assert_receipt(&cloned.log_info(message).unwrap(), &metadata);
    }
    assert_eq!(*messages.lock().unwrap(), inputs.map(str::to_owned));
    assert_eq!(binding.inspect().unwrap(), metadata);
    let payload = "sensitive message payload";
    let receipt = binding.log_info(payload).unwrap();
    let safe = format!("{binding:?} {controller:?} {metadata:?} {receipt:?}");
    assert!(!safe.contains(payload));
}

#[test]
fn revoke_and_replacement_publish_shared_monotonic_revisions() {
    let (binding, controller) = LoggingBinding::new().unwrap();
    let cloned = binding.clone();
    let context = binding.inspect().unwrap().context;
    let (old, old_messages) = recording_logger();
    let (new, new_messages) = recording_logger();
    controller.authorize(old, "old").unwrap();
    let old_receipt = binding.log_info("old call").unwrap();
    controller.authorize(new, "new").unwrap();
    let replacement = cloned.inspect().unwrap();
    assert_eq!(replacement.revision, 2);
    assert_eq!(replacement.implementation.as_deref(), Some("new"));
    assert_receipt(&cloned.log_info("new call").unwrap(), &replacement);
    assert_eq!(old_receipt.revision(), 1);
    assert_eq!(old_receipt.implementation(), "old");
    for revision in [3, 4] {
        controller.revoke().unwrap();
        assert_eq!(
            cloned.inspect().unwrap(),
            LoggingMetadata {
                context,
                revision,
                implementation: None
            }
        );
        assert_eq!(
            binding.log_info("denied"),
            Err(LoggingError::Denied { context, revision })
        );
    }
    assert_eq!(*old_messages.lock().unwrap(), ["old call"]);
    assert_eq!(*new_messages.lock().unwrap(), ["new call"]);
}

#[test]
fn backend_failure_exposes_only_safe_admission_metadata() {
    let (binding, controller) = LoggingBinding::new().unwrap();
    let (logger, messages) = recording_logger();
    let secret = "private backend credential and path";
    controller
        .authorize(
            Logger::new(move |message| {
                logger.log(message)?;
                Err(LogError::new(secret))
            }),
            "failing-memory",
        )
        .unwrap();
    let metadata = binding.inspect().unwrap();
    let error = binding.log_info("exact attempted message").unwrap_err();
    match &error {
        LoggingError::Backend { admission } => assert_receipt(admission, &metadata),
        other => panic!("expected backend failure, got {other:?}"),
    }
    assert_eq!(error.to_string(), "authorized logging backend failed");
    assert!(!format!("{error:?}").contains(secret));
    assert!(!format!("{error:?} {error}").contains("exact attempted message"));
    assert!(std::error::Error::source(&error).is_none());
    assert_eq!(*messages.lock().unwrap(), ["exact attempted message"]);
    assert_eq!(binding.inspect().unwrap(), metadata);
}

#[test]
fn inspection_and_debug_have_no_sink_or_authorization_effects() {
    let (binding, controller) = LoggingBinding::new().unwrap();
    let denied = binding.inspect().unwrap();
    assert_eq!(binding.clone().inspect().unwrap(), denied);
    assert_eq!(
        binding.log_info("still denied"),
        Err(LoggingError::Denied {
            context: denied.context,
            revision: denied.revision,
        })
    );
    let (logger, messages) = recording_logger();
    controller.authorize(logger, "inspection").unwrap();
    let authorized = binding.inspect().unwrap();
    for _ in 0..3 {
        assert_eq!(binding.clone().inspect().unwrap(), authorized);
        let _ = format!("{binding:?} {controller:?} {authorized:?}");
    }
    assert!(messages.lock().unwrap().is_empty());
    controller.revoke().unwrap();
    assert_eq!(binding.inspect().unwrap().implementation, None);
    assert!(messages.lock().unwrap().is_empty());
}

#[test]
fn invalid_labels_leave_revision_and_existing_grant_unchanged() {
    let (binding, controller) = LoggingBinding::new().unwrap();
    let (logger, messages) = recording_logger();
    let (rejected, rejected_messages) = recording_logger();
    let invalid = [
        "".to_owned(),
        "   ".to_owned(),
        "x".repeat(129),
        "tab\t".to_owned(),
        "newline\n".to_owned(),
        "nul\0".to_owned(),
        "delete\u{7f}".to_owned(),
        "é".to_owned(),
    ];
    for authorized in [false, true] {
        if authorized {
            controller.authorize(logger.clone(), "existing").unwrap();
        }
        let before = binding.inspect().unwrap();
        for label in &invalid {
            assert_eq!(
                controller.authorize(rejected.clone(), label.clone()),
                Err(LoggingError::InvalidLabel)
            );
            assert_eq!(binding.inspect().unwrap(), before);
        }
    }
    assert_receipt(
        &binding.log_info("existing still works").unwrap(),
        &binding.inspect().unwrap(),
    );
    assert_eq!(*messages.lock().unwrap(), ["existing still works"]);
    assert!(rejected_messages.lock().unwrap().is_empty());
    for label in ["x".repeat(128), " ! printable ~ ".to_owned()] {
        controller.authorize(logger.clone(), label.clone()).unwrap();
        assert_eq!(binding.inspect().unwrap().implementation, Some(label));
    }
}

#[test]
fn revision_exhaustion_preserves_authorized_and_denied_state() {
    for authorized in [false, true] {
        let (binding, controller) = LoggingBinding::new().unwrap();
        let (logger, messages) = recording_logger();
        let (replacement, replacement_messages) = recording_logger();
        if authorized {
            controller.authorize(logger.clone(), "last grant").unwrap();
        }
        binding.shared.state.lock().unwrap().revision = u64::MAX - 1;
        if authorized {
            controller.authorize(logger, "final grant").unwrap();
        } else {
            controller.revoke().unwrap();
        }
        let before = binding.inspect().unwrap();
        assert_eq!(before.revision, u64::MAX);
        assert_eq!(
            controller.authorize(replacement, "replacement"),
            Err(LoggingError::RevisionExhausted)
        );
        assert_eq!(controller.revoke(), Err(LoggingError::RevisionExhausted));
        assert_eq!(binding.inspect().unwrap(), before);
        if authorized {
            assert_receipt(&binding.log_info("final admission").unwrap(), &before);
            assert_eq!(*messages.lock().unwrap(), ["final admission"]);
        } else {
            assert_eq!(
                binding.log_info("denied"),
                Err(LoggingError::Denied {
                    context: before.context,
                    revision: u64::MAX,
                })
            );
        }
        if !authorized {
            assert!(messages.lock().unwrap().is_empty());
        }
        assert!(replacement_messages.lock().unwrap().is_empty());
    }
}

#[test]
fn poisoned_state_returns_synchronization_errors_without_sink_effects() {
    let (binding, controller) = LoggingBinding::new().unwrap();
    let (logger, messages) = recording_logger();
    controller.authorize(logger.clone(), "memory").unwrap();
    let cloned = binding.clone();
    assert!(
        catch_unwind(AssertUnwindSafe(|| {
            let _guard = binding.shared.state.lock().unwrap();
            panic!("deliberately poison logging state");
        }))
        .is_err()
    );
    assert_eq!(binding.inspect(), Err(LoggingError::Synchronization));
    assert_eq!(
        cloned.log_info("not forwarded"),
        Err(LoggingError::Synchronization)
    );
    assert_eq!(
        controller.authorize(logger, "replacement"),
        Err(LoggingError::Synchronization)
    );
    assert_eq!(controller.revoke(), Err(LoggingError::Synchronization));
    assert!(messages.lock().unwrap().is_empty());
    let state = binding.shared.state.lock().err().unwrap().into_inner();
    assert_eq!(state.revision, 1);
    assert_eq!(state.grant.as_ref().unwrap().label, "memory");
}

#[test]
fn admitted_old_call_survives_revoke_and_replacement() {
    let (binding, controller) = LoggingBinding::new().unwrap();
    let (entered_tx, entered_rx) = mpsc::channel();
    let (release_tx, release_rx) = mpsc::channel();
    let release_rx = Mutex::new(release_rx);
    let (old_logger, old_messages) = recording_logger();
    controller
        .authorize(
            Logger::new(move |message| {
                old_logger.log(message)?;
                entered_tx.send(()).unwrap();
                release_rx.lock().unwrap().recv().unwrap();
                Ok(())
            }),
            "old",
        )
        .unwrap();
    let old_metadata = binding.inspect().unwrap();
    let old_binding = binding.clone();
    let old_call = thread::spawn(move || old_binding.log_info("blocked old call"));
    entered_rx.recv_timeout(Duration::from_secs(5)).unwrap();

    let (new_logger, new_messages) = recording_logger();
    let (published_tx, published_rx) = mpsc::channel();
    let new_binding = binding.clone();
    let publisher = thread::spawn(move || {
        let revoked = controller.revoke();
        let denial = new_binding.log_info("must be denied");
        let replaced = controller.authorize(new_logger, "new");
        let metadata = new_binding.inspect();
        let receipt = new_binding.log_info("new call");
        published_tx
            .send((revoked, denial, replaced, metadata, receipt))
            .unwrap();
    });
    // The timeout is only a deadlock watchdog; ordering comes from channels.
    let published = published_rx.recv_timeout(Duration::from_secs(5));
    release_tx.send(()).unwrap();
    let old_result = old_call.join().unwrap();
    publisher.join().unwrap();
    let (revoked, denial, replaced, metadata, receipt) =
        published.expect("publication must not wait for the admitted sink");
    assert_eq!(revoked, Ok(()));
    assert_eq!(
        denial,
        Err(LoggingError::Denied {
            context: old_metadata.context,
            revision: 2
        })
    );
    assert_eq!(replaced, Ok(()));
    let metadata = metadata.unwrap();
    assert_eq!(metadata.context, old_metadata.context);
    assert_eq!(metadata.revision, 3);
    assert_eq!(metadata.implementation.as_deref(), Some("new"));
    assert_receipt(&receipt.unwrap(), &metadata);
    assert_receipt(&old_result.unwrap(), &old_metadata);
    assert_eq!(*old_messages.lock().unwrap(), ["blocked old call"]);
    assert_eq!(*new_messages.lock().unwrap(), ["new call"]);
    assert_eq!(binding.inspect().unwrap(), metadata);
}

#[test]
fn public_logging_types_are_send_and_sync() {
    fn assert_send_sync<T: Send + Sync>() {}
    assert_send_sync::<LoggingBinding>();
    assert_send_sync::<LoggingController>();
    assert_send_sync::<LoggingMetadata>();
    assert_send_sync::<LoggingReceipt>();
    assert_send_sync::<LoggingError>();
    assert_send_sync::<ContextId>();
}
