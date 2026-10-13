//! Exercises single-instance forwarding and its start-up and shutdown races on synthetic profiles.

use std::{
    path::PathBuf,
    sync::mpsc,
    thread,
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use futures::{FutureExt, StreamExt, channel::mpsc::unbounded};

use super::*;

/// A profile path no other test or real Syrinx uses; it is only named, never created.
fn synthetic_profile(label: &str) -> PathBuf {
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("synthetic clock follows epoch")
        .as_nanos();
    std::env::temp_dir().join(format!(
        "syrinx-instance-test-{label}-{}-{nonce}",
        std::process::id()
    ))
}

fn link(text: &str) -> LinkRequest {
    LinkRequest::open(text).expect("synthetic web address")
}

fn primary(directory: &Path) -> PrimaryInstance {
    match claim(directory, &LinkRequest::Activate, Duration::from_secs(5)) {
        Ok(Claim::Primary(instance)) => instance,
        Ok(Claim::Forwarded { .. }) => panic!("a fresh profile has no running Syrinx"),
        Err(error) => panic!("claim failed: {error}"),
    }
}

/// Claims on another thread, as a second launch would; that thread drops what it claims.
fn launch(
    directory: &Path,
    request: LinkRequest,
    patience: Duration,
) -> Result<bool, InstanceError> {
    let directory = directory.to_owned();
    thread::spawn(move || {
        claim(&directory, &request, patience).map(|claim| matches!(claim, Claim::Forwarded { .. }))
    })
    .join()
    .expect("launch thread")
}

fn queued(feed: &mut LinkFeed) -> Vec<LinkRequest> {
    let mut requests = Vec::new();
    while let Some(Some(request)) = feed.requests.next().now_or_never() {
        requests.push(request);
    }
    requests
}

/// The owner-only descriptor and object names a launch on `directory` would use.
fn identity(directory: &Path) -> (OwnerOnly, InstanceNames) {
    let owner = OwnerOnly::current_user().expect("owner");
    let names = InstanceNames::new(directory, owner.sid(), owner.session()).expect("names");
    (owner, names)
}

/// Sends a raw message, retrying while the listener is between clients, as `claim` does.
fn answer(owner: &OwnerOnly, pipe_name: &str, message: &[u8]) -> Delivery {
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        let delivery = pipe::send(pipe_name, message, owner).expect("pipe works");
        if !matches!(delivery, Delivery::Unavailable) || Instant::now() >= deadline {
            return delivery;
        }
        thread::sleep(RETRY_INTERVAL);
    }
}

#[test]
fn a_second_launch_forwards_its_link_and_exits() {
    let directory = synthetic_profile("forward");
    let mut instance = primary(&directory);
    let mut feed = instance
        .take_feed(Some(link("https://own.example.test/")))
        .expect("feed");

    let forwarded = launch(
        &directory,
        link("HTTPS://Forwarded.Example.TEST/page"),
        Duration::from_secs(5),
    );
    assert_eq!(forwarded, Ok(true));
    assert_eq!(
        launch(&directory, LinkRequest::Activate, Duration::from_secs(5)),
        Ok(true)
    );

    assert_eq!(
        queued(&mut feed),
        [
            link("https://own.example.test/"),
            link("https://forwarded.example.test/page"),
            LinkRequest::Activate,
        ]
    );
    assert!(
        instance.take_feed(None).is_err(),
        "the feed is handed out once"
    );
}

#[test]
fn the_running_syrinx_rechecks_every_request() {
    let directory = synthetic_profile("forged");
    let mut instance = primary(&directory);
    let mut feed = instance.take_feed(None).expect("feed");
    let (owner, names) = identity(&directory);

    // A client that skips `LinkRequest::open` must still be refused by the listener.
    for forged in [
        &b"Ojavascript:alert(1)"[..],
        &b"Ofile:///C:/Windows/System32/cmd.exe"[..],
        &b"Ohttps://name:secret@example.test/"[..],
        &b"Xhttps://example.test/"[..],
    ] {
        assert!(matches!(
            answer(&owner, &names.pipe, forged),
            Delivery::Refused
        ));
    }
    // An empty message is never queued, whether or not the pipe delivers it at all.
    assert!(!matches!(
        answer(&owner, &names.pipe, &[]),
        Delivery::Accepted { .. }
    ));
    let mut overlong = vec![b'O'];
    overlong.extend_from_slice(b"https://example.test/");
    overlong.resize(protocol::MAX_MESSAGE_BYTES + 64, b'a');
    assert!(matches!(
        answer(&owner, &names.pipe, &overlong),
        Delivery::Refused
    ));
    assert!(queued(&mut feed).is_empty());
}

#[test]
fn simultaneous_launches_elect_exactly_one_running_syrinx() {
    let directory = synthetic_profile("race");
    let launches = 6;
    let (outcomes, results) = mpsc::channel();
    let mut workers = Vec::new();
    for index in 0..launches {
        let directory = directory.clone();
        let outcomes = outcomes.clone();
        workers.push(thread::spawn(move || {
            let request = link(&format!("https://race-{index}.example.test/"));
            match claim(&directory, &request, Duration::from_secs(10)) {
                Ok(Claim::Primary(mut instance)) => {
                    let feed = instance.take_feed(None).expect("feed");
                    let (release, released) = mpsc::channel::<()>();
                    outcomes
                        .send(Some((feed, release)))
                        .expect("report primary");
                    // Keep the profile until every other launch has been answered.
                    released.recv().expect("release primary");
                }
                Ok(Claim::Forwarded { .. }) => outcomes.send(None).expect("report forward"),
                Err(error) => panic!("launch {index} failed: {error}"),
            }
        }));
    }
    drop(outcomes);

    let mut primaries = Vec::new();
    let mut forwarded = 0;
    for _ in 0..launches {
        match results.recv().expect("every launch reports") {
            Some(primary) => primaries.push(primary),
            None => forwarded += 1,
        }
    }
    assert_eq!(primaries.len(), 1, "exactly one launch owns the profile");
    assert_eq!(forwarded, launches - 1);
    let (mut feed, release) = primaries.pop().expect("one primary");
    assert_eq!(queued(&mut feed).len(), launches - 1);
    release.send(()).expect("release the primary");
    for worker in workers {
        worker.join().expect("launch thread");
    }
}

#[test]
fn a_launch_during_start_up_waits_for_the_listener() {
    let directory = synthetic_profile("startup");
    let held = directory.clone();
    let (locked, lock_held) = mpsc::channel();
    let (sender, mut requests) = unbounded();
    let starting = thread::spawn(move || {
        let (owner, names) = identity(&held);
        // Hold the mutex but listen only later, like a Syrinx still starting up.
        let mut lock = NamedMutex::open(&names.mutex, &owner).expect("mutex");
        assert_eq!(lock.try_acquire(), Ok(true));
        locked.send(()).expect("report lock");
        thread::sleep(Duration::from_millis(400));
        let server = PipeServer::start(&names.pipe, &owner, sender).expect("listener");
        thread::sleep(Duration::from_secs(2));
        drop(server);
        lock.release();
    });
    lock_held.recv().expect("lock taken");

    assert_eq!(
        launch(
            &directory,
            link("https://early.example.test/"),
            Duration::from_secs(5)
        ),
        Ok(true)
    );
    assert_eq!(
        requests.next().now_or_never(),
        Some(Some(link("https://early.example.test/")))
    );
    starting.join().expect("starting instance");
}

#[test]
fn a_launch_during_shutdown_becomes_the_next_running_syrinx() {
    let directory = synthetic_profile("shutdown");
    let (stopped, stop_raised) = mpsc::channel();
    let closing_directory = directory.clone();
    let closing = thread::spawn(move || {
        let mut instance = primary(&closing_directory);
        let feed = instance.take_feed(None).expect("feed");
        // The window stops listening as it starts closing, then exits a moment later.
        feed.stop.raise().expect("stop listening");
        stopped.send(()).expect("report stop");
        thread::sleep(Duration::from_millis(500));
        drop(instance);
    });
    stop_raised.recv().expect("stop raised");

    assert_eq!(
        launch(
            &directory,
            link("https://late.example.test/"),
            Duration::from_secs(10)
        ),
        Ok(false),
        "the launch must wait and own the profile itself, not lose its link"
    );
    closing.join().expect("closing instance");
}

#[test]
fn a_syrinx_that_died_holding_the_profile_is_replaced() {
    let directory = synthetic_profile("abandoned");
    let held = directory.clone();
    thread::spawn(move || {
        let (owner, names) = identity(&held);
        let mut lock = NamedMutex::open(&names.mutex, &owner).expect("mutex");
        assert_eq!(lock.try_acquire(), Ok(true));
        // Simulate a crash: the owning thread ends without releasing the mutex.
        std::mem::forget(lock);
    })
    .join()
    .expect("crashed instance");

    assert_eq!(
        launch(&directory, LinkRequest::Activate, Duration::from_secs(5)),
        Ok(false)
    );
}

#[test]
fn a_silent_running_syrinx_ends_the_wait_with_an_error() {
    let directory = synthetic_profile("silent");
    let held = directory.clone();
    let (locked, lock_held) = mpsc::channel();
    let (finished, finish) = mpsc::channel::<()>();
    let silent = thread::spawn(move || {
        let (owner, names) = identity(&held);
        let mut lock = NamedMutex::open(&names.mutex, &owner).expect("mutex");
        assert_eq!(lock.try_acquire(), Ok(true));
        locked.send(()).expect("report lock");
        finish.recv().expect("test finished");
        lock.release();
    });
    lock_held.recv().expect("lock taken");

    assert_eq!(
        launch(
            &directory,
            LinkRequest::Activate,
            Duration::from_millis(300)
        ),
        Err(InstanceError::NoResponse)
    );
    finished.send(()).expect("finish silent instance");
    silent.join().expect("silent instance");
}

#[test]
fn separate_profiles_never_forward_to_each_other() {
    let owner_profile = synthetic_profile("owner");
    let test_profile = synthetic_profile("test");
    let mut first = primary(&owner_profile);
    let mut first_feed = first.take_feed(None).expect("feed");

    assert_eq!(
        launch(
            &test_profile,
            link("https://isolated.example.test/"),
            Duration::from_secs(5)
        ),
        Ok(false)
    );
    assert!(queued(&mut first_feed).is_empty());
}

#[test]
fn a_pipe_name_held_by_someone_else_stops_the_launch() {
    let directory = synthetic_profile("squat");
    let (owner, names) = identity(&directory);
    let (sender, _requests) = unbounded();
    let _squatter = PipeServer::start(&names.pipe, &owner, sender.clone()).expect("first pipe");

    assert!(matches!(
        PipeServer::start(&names.pipe, &owner, sender),
        Err(InstanceError::PipeTaken)
    ));
}

#[test]
fn a_squatted_name_fails_after_the_handover_wait() {
    let directory = synthetic_profile("squat-claim");
    let (owner, names) = identity(&directory);
    let (sender, _requests) = unbounded();
    let _squatter = PipeServer::start(&names.pipe, &owner, sender).expect("first pipe");

    let started = Instant::now();
    assert_eq!(
        launch(&directory, LinkRequest::Activate, Duration::from_secs(5)),
        Err(InstanceError::PipeTaken)
    );
    assert!(
        started.elapsed() >= PIPE_HANDOVER,
        "waited for a predecessor first"
    );
}

#[test]
fn a_silent_client_does_not_delay_shutdown() {
    let directory = synthetic_profile("stall");
    let names = identity(&directory).1;
    let mut instance = primary(&directory);
    let feed = instance.take_feed(None).expect("feed");

    // Connect and send nothing, as a stuck same-user client would.
    let deadline = Instant::now() + Duration::from_secs(5);
    let _stalled = loop {
        match std::fs::OpenOptions::new()
            .read(true)
            .write(true)
            .open(&names.pipe)
        {
            Ok(client) => break client,
            Err(error) if Instant::now() < deadline => {
                assert_ne!(error.kind(), std::io::ErrorKind::PermissionDenied);
                thread::sleep(RETRY_INTERVAL);
            }
            Err(error) => panic!("cannot reach the listener: {error}"),
        }
    };
    thread::sleep(Duration::from_millis(200));

    let stopping = Instant::now();
    feed.stop.raise().expect("stop listening");
    drop(instance);
    assert!(
        stopping.elapsed() < Duration::from_secs(1),
        "teardown waited {:?}",
        stopping.elapsed()
    );
}
