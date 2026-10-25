#[path = "../examples/downstream_shell.rs"]
mod example;
use example::{DENIED, DownstreamShell, ECHO, fixture, shell};

fn assert_snapshot(left: &scope_script::runner::Snapshot, right: &scope_script::runner::Snapshot) {
    assert_eq!(left.identity, right.identity);
    assert_eq!(left.revision, right.revision);
    assert_eq!(left.text, right.text);
}

#[test]
fn aliases_share_script_authorization_and_live_source_lifecycle() {
    let service = fixture::MultiLanguage::default();
    let mut host = DownstreamShell::new(&service).unwrap();
    let input = format!(
        ":scope downstream\n\
         :scope downstream demo echo α $literal\n\
         :scope scratch new example.multi\n:scope line 1 \"unsaved α\"\n\
         :scope semantic-target 0\n\
         :scope scratch new scope-script\n:scope line 1 {echo:?}\n\
         :scope downstream demo echo α $literal\n:scope script run\n\
         :scope line 1 {denied:?}\n\
         :scope downstream demo denied\n:scope script run\n\
         :scope scratch select 0\n:scope line 1 \"edited β\"\n\
         :scope scratch select 1\n:scope downstream demo echo fresh\n\
         :scope scratch select 0\n:scope discard\n\
         :scope downstream demo echo closed\n:scope script run\n\
         :scope semantic-target none\n:scope downstream demo echo cleared\n\
         :scope downstream demo unknown\n:scope session\n:scope quit\n",
        echo = ECHO,
        denied = DENIED,
    );
    let output = shell(&mut host, &input).unwrap();
    assert_eq!(host.selections, 5, "{output}");
    assert_eq!(host.arguments[1], ["α", "$literal"]);
    assert_eq!(host.arguments[3], ["fresh"]);
    assert!(host.targets[0].is_none());
    assert!(host.targets[4].is_none());
    let initial = host.targets[1].as_ref().unwrap();
    assert_eq!(initial.id, 0);
    assert_eq!(initial.snapshot.text, "unsaved α");
    assert!(initial.dirty);
    assert!(initial.destination.is_none());
    assert_eq!(
        format!("{:?}", host.targets[1]),
        format!("{:?}", host.targets[2])
    );
    let edited = host.targets[3].as_ref().unwrap();
    assert_eq!(edited.snapshot.identity, initial.snapshot.identity);
    assert_ne!(edited.snapshot.revision, initial.snapshot.revision);
    assert_eq!(edited.snapshot.text, "edited β");
    assert!(edited.dirty);
    let records = &host.runner.records;
    assert_eq!(records.len(), 5, "{output}");
    for (alias, script) in [(0, 1), (2, 3)] {
        assert_snapshot(&records[alias].target, &initial.snapshot);
        assert_snapshot(&records[alias].target, &records[script].target);
        assert_eq!(records[alias].source.text, records[script].source.text);
        assert_ne!(
            records[alias].source.identity,
            records[script].source.identity
        );
        assert_eq!(
            format!("{:?}", records[alias].outcome),
            format!("{:?}", records[script].outcome)
        );
        assert_eq!(records[alias].results, records[script].results);
    }
    assert!(records[0].outcome.is_ok());
    assert!(records[2].outcome.is_err());
    assert!(records[2].results.is_empty());
    assert_eq!(records[1].source.identity, records[3].source.identity);
    assert_ne!(records[1].source.revision, records[3].source.revision);
    assert_snapshot(&records[4].target, &edited.snapshot);
    assert_eq!(*service.calls.lock().unwrap(), ["echo", "echo", "echo"]);
    for expected in [
        "target_required",
        "target_unavailable",
        "unknown_command",
        "Active language: none",
    ] {
        assert!(output.contains(expected), "{output}");
    }
}
