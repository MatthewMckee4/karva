//! Controller integration tests: handshake, event intake, and disconnect behavior.

use std::io::{BufRead as _, BufReader, ErrorKind, Read as _, Write as _};
use std::net::{Shutdown, TcpStream};
use std::sync::mpsc;
use std::thread;
use std::time::{Duration, Instant};

use karva_python_semantic::TestCacheKey;
use rstest::rstest;

use crate::protocol::{WireMessage, WorkerEvent, WorkerSelection};
use crate::worker::WorkerClient;

use super::{ControllerServer, is_clean_disconnect};

// Receipt: two seconds is 200 background flush intervals and remains a
// practical deadlock tripwire on loaded CI runners.
const BUFFERED_EVENT_TIMEOUT: Duration = Duration::from_secs(2);

fn selection(test_paths: Vec<String>) -> WorkerSelection {
    WorkerSelection {
        test_paths,
        resume_skip: Vec::new(),
    }
}

fn accept_connections(server: &mut ControllerServer, count: usize) {
    while server.readers.len() < count {
        server.accept_pending().expect("accept worker");
        thread::yield_now();
    }
}

#[test]
fn streams_attributed_worker_events() {
    let mut server = ControllerServer::bind("run-id").expect("bind controller");
    server
        .register_worker_selection(7, selection(vec!["mod::test".to_string()]))
        .expect("register worker selection");
    let address = server.address().expect("address");
    let worker = thread::spawn(move || {
        let (client, selection) =
            WorkerClient::connect(address, "run-id", 7).expect("connect worker");
        assert_eq!(selection.test_paths, ["mod::test"]);
        client
            .send_event(WorkerEvent::TestStarted {
                name: "mod::test".to_string(),
                cache_key: TestCacheKey::function_name("mod::test"),
            })
            .expect("send event");
        client.complete().expect("complete worker");
    });

    accept_connections(&mut server, 1);
    worker.join().expect("join worker");
    server.finish().expect("finish readers");
    let event = server
        .try_recv()
        .expect("receive event")
        .expect("queued event");

    assert_eq!(event.worker_id, 7);
    assert!(matches!(
        *event.event,
        WorkerEvent::TestStarted { name, cache_key }
            if name == "mod::test"
                && cache_key == TestCacheKey::function_name("mod::test")
    ));
}

#[cfg(unix)]
#[test]
fn controller_can_close_stream_retained_after_worker_exit() {
    let mut server = ControllerServer::bind("run-id").expect("bind controller");
    server
        .register_worker_selection(7, selection(vec!["mod::test".to_string()]))
        .expect("register worker selection");
    let address = server.address().expect("address");
    let worker = thread::spawn(move || {
        WorkerClient::connect(address, "run-id", 7)
            .expect("connect worker")
            .0
    });

    accept_connections(&mut server, 1);
    let retained_connection = worker.join().expect("join worker");
    while !server.worker_started(7).expect("read worker state") {
        server.try_recv().expect("receive handshake");
        thread::yield_now();
    }

    server.disconnect_worker(7).expect("disconnect worker");
    server.finish().expect("finish readers");
    drop(retained_connection);
}

#[test]
fn rejects_wrong_run_id() {
    let mut server = ControllerServer::bind("expected").expect("bind controller");
    let address = server.address().expect("address");
    let worker = thread::spawn(move || {
        let error = WorkerClient::connect(address, "wrong", 0)
            .err()
            .expect("wrong run id should close connection");
        assert!(error.to_string().contains("before sending test paths"));
    });

    accept_connections(&mut server, 1);
    worker.join().expect("join worker");
    server.finish().expect("finish readers");
    let Err(error) = server.try_recv() else {
        panic!("wrong run id should be rejected");
    };

    assert!(error.to_string().contains("expected `expected`"));
}

#[test]
fn reset_connections_are_clean_disconnects() {
    for kind in [ErrorKind::ConnectionReset, ErrorKind::ConnectionAborted] {
        let error = serde_json::Error::io(std::io::Error::from(kind));
        assert!(is_clean_disconnect(&error));
    }

    let error = serde_json::Error::io(std::io::Error::from(ErrorKind::InvalidData));
    assert!(!is_clean_disconnect(&error));
}

#[test]
fn completion_closes_connection_after_terminal_event() {
    let mut server = ControllerServer::bind("run-id").expect("bind controller");
    server
        .register_worker_selection(7, selection(Vec::new()))
        .expect("register worker selection");
    let address = server.address().expect("address");
    let worker = thread::spawn(move || {
        let (client, selection) =
            WorkerClient::connect(address, "run-id", 7).expect("connect worker");
        assert!(selection.test_paths.is_empty());
        client.complete().expect("complete worker");
    });

    accept_connections(&mut server, 1);
    worker.join().expect("join worker");
    server.finish().expect("finish readers");
    let event = server
        .try_recv()
        .expect("receive event")
        .expect("queued event");

    assert_eq!(event.worker_id, 7);
    assert!(matches!(*event.event, WorkerEvent::WorkerFinished));
}

#[test]
fn transfers_resume_skip_cases() {
    let mut server = ControllerServer::bind("run-id").expect("bind controller");
    server
        .register_worker_selection(
            7,
            WorkerSelection {
                test_paths: vec!["mod::test".to_string()],
                resume_skip: vec![TestCacheKey::function_name("mod::test[1]")],
            },
        )
        .expect("register worker selection");
    let address = server.address().expect("address");
    let worker = thread::spawn(move || {
        let (client, selection) =
            WorkerClient::connect(address, "run-id", 7).expect("connect worker");
        assert_eq!(
            selection.resume_skip,
            [TestCacheKey::function_name("mod::test[1]")]
        );
        client.complete().expect("complete worker");
    });

    accept_connections(&mut server, 1);
    worker.join().expect("join worker");
    server.finish().expect("finish readers");
}

#[test]
fn truncated_terminal_event_is_a_worker_disconnect() {
    let mut server = ControllerServer::bind("run-id").expect("bind controller");
    server
        .register_worker_selection(7, selection(vec!["mod::test".to_string()]))
        .expect("register worker selection");
    let address = server.address().expect("address");
    let worker = thread::spawn(move || {
        let mut stream = TcpStream::connect(address).expect("connect worker");
        serde_json::to_writer(
            &mut stream,
            &WireMessage::Hello {
                run_id: "run-id".to_string(),
                worker_id: 7,
            },
        )
        .expect("write handshake");
        stream.write_all(b"\n").expect("frame handshake");
        stream.flush().expect("flush handshake");

        let mut selection = String::new();
        BufReader::new(stream.try_clone().expect("clone stream"))
            .read_line(&mut selection)
            .expect("read selection");
        assert!(!selection.is_empty());

        stream
            .write_all(br#"{"Event":{"TestStarted""#)
            .expect("write truncated event");
        stream.flush().expect("flush truncated event");
        stream
            .shutdown(Shutdown::Write)
            .expect("close worker write stream");
        let mut drain = Vec::new();
        stream
            .read_to_end(&mut drain)
            .expect("drain controller stream");
    });

    accept_connections(&mut server, 1);
    worker.join().expect("join worker");
    server.finish().expect("finish readers");
    assert!(server.try_recv().expect("drain events").is_none());
    assert!(server.worker_started(7).expect("read worker state"));
    assert!(server.worker_disconnected(7));
}

#[rstest]
fn transfers_large_worker_selection(#[values(50_000, 1_000_000)] path_count: usize) {
    // Receipt: 50,000 is the reported workload; 1,000,000 is the requested stress case.
    let mut test_paths = Vec::with_capacity(path_count);
    for index in 0..path_count {
        test_paths.push(format!("tests/test_{index}.py::test_case"));
    }

    let mut server = ControllerServer::bind("run-id").expect("bind controller");
    server
        .register_worker_selection(0, selection(test_paths))
        .expect("register worker selection");
    let address = server.address().expect("address");
    let worker = thread::spawn(move || {
        let (client, selection) =
            WorkerClient::connect(address, "run-id", 0).expect("connect worker");
        let test_paths = selection.test_paths;
        assert_eq!(test_paths.len(), path_count);
        assert_eq!(
            test_paths.first().map(String::as_str),
            Some("tests/test_0.py::test_case")
        );
        let last_path = format!("tests/test_{}.py::test_case", path_count - 1);
        assert_eq!(
            test_paths.last().map(String::as_str),
            Some(last_path.as_str())
        );
        client.complete().expect("complete worker");
    });

    accept_connections(&mut server, 1);
    worker.join().expect("join worker");
    server.finish().expect("finish readers");
}

#[test]
fn buffered_event_reaches_controller_before_worker_completes() {
    let mut server = ControllerServer::bind("run-id").expect("bind controller");
    server
        .register_worker_selection(7, selection(Vec::new()))
        .expect("register worker selection");
    let address = server.address().expect("address");
    let (event_received, wait_for_event) = mpsc::channel();
    let worker = thread::spawn(move || {
        let (client, selection) =
            WorkerClient::connect(address, "run-id", 7).expect("connect worker");
        assert!(selection.test_paths.is_empty());
        client
            .send_event(WorkerEvent::TestSlow)
            .expect("send event");
        wait_for_event
            .recv_timeout(BUFFERED_EVENT_TIMEOUT)
            .expect("controller should receive buffered event");
        client.complete().expect("complete worker");
    });

    accept_connections(&mut server, 1);
    let deadline = Instant::now() + BUFFERED_EVENT_TIMEOUT;
    let event = loop {
        if let Some(event) = server.try_recv().expect("receive event") {
            break event;
        }
        assert!(Instant::now() < deadline, "buffered event was not flushed");
        thread::yield_now();
    };

    assert_eq!(event.worker_id, 7);
    assert!(matches!(*event.event, WorkerEvent::TestSlow));
    event_received.send(()).expect("release worker");
    worker.join().expect("join worker");
    server.finish().expect("finish readers");
}
