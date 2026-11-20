use std::{
    io::{BufRead, BufReader, Write},
    net::{TcpListener, TcpStream},
    sync::mpsc,
    thread,
    time::Duration,
};

use gpui_agent::{
    AgentClient, HelloAuth, HelloInfo, PROTOCOL_VERSION, PlatformKind, Response, ScreenshotMode,
    is_unsupported_op, protocol::Op,
};

fn read_request(reader: &mut BufReader<TcpStream>) -> serde_json::Value {
    let mut line = String::new();
    reader.read_line(&mut line).unwrap();
    serde_json::from_str(&line).unwrap()
}

fn respond(writer: &mut TcpStream, request: &serde_json::Value, hello: Option<HelloInfo>) {
    let mut response = Response::ok(request["id"].as_str().unwrap());
    response.hello = hello;
    serde_json::to_writer(&mut *writer, &response).unwrap();
    writer.write_all(b"\n").unwrap();
    writer.flush().unwrap();
}

fn hello(features: &[&str]) -> HelloInfo {
    HelloInfo {
        protocol: PROTOCOL_VERSION,
        app: "fake".into(),
        platform: PlatformKind::Headless,
        os: "test".into(),
        ready: true,
        deliveries: Vec::new(),
        auth: HelloAuth::None,
        bundle_id: None,
        executable: None,
        features: features.iter().map(|feature| (*feature).into()).collect(),
        framework: None,
        device: None,
    }
}

fn press() -> Op {
    Op::Press {
        target: Some("button".into()),
        x: None,
        y: None,
        expect_fingerprint: None,
    }
}

#[test]
fn screenshot_bytes_refuses_hosts_without_inline_screenshot_support() {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap();
    let server = thread::spawn(move || {
        let (mut writer, _) = listener.accept().unwrap();
        writer
            .set_read_timeout(Some(Duration::from_millis(300)))
            .unwrap();
        let mut reader = BufReader::new(writer.try_clone().unwrap());
        let request = read_request(&mut reader);
        assert_eq!(request["op"], "hello");
        respond(&mut writer, &request, Some(hello(&[])));

        let mut line = String::new();
        let error = reader.read_line(&mut line).unwrap_err();
        assert!(matches!(
            error.kind(),
            std::io::ErrorKind::TimedOut | std::io::ErrorKind::WouldBlock
        ));
    });

    let mut client = AgentClient::connect(addr).with_timeout(Duration::from_secs(2));
    let error = client
        .screenshot_bytes(ScreenshotMode::Viewport, None, None)
        .unwrap_err();
    assert!(is_unsupported_op(&error), "{error}");
    server.join().unwrap();
}

#[test]
fn gated_call_renegotiates_after_the_host_restarts() {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap();
    let (closed_tx, closed_rx) = mpsc::channel();
    let server = thread::spawn(move || {
        let (mut old_writer, _) = listener.accept().unwrap();
        let mut old_reader = BufReader::new(old_writer.try_clone().unwrap());
        let request = read_request(&mut old_reader);
        assert_eq!(request["op"], "hello");
        respond(
            &mut old_writer,
            &request,
            Some(hello(&["press", "screenshot_inline"])),
        );
        let request = read_request(&mut old_reader);
        assert_eq!(request["op"], "press");
        respond(&mut old_writer, &request, None);
        drop(old_reader);
        drop(old_writer);
        closed_tx.send(()).unwrap();

        let (mut new_writer, _) = listener.accept().unwrap();
        new_writer
            .set_read_timeout(Some(Duration::from_millis(500)))
            .unwrap();
        let mut new_reader = BufReader::new(new_writer.try_clone().unwrap());
        let request = read_request(&mut new_reader);
        assert_eq!(request["op"], "hello");
        respond(
            &mut new_writer,
            &request,
            Some(hello(&["screenshot_inline"])),
        );

        let mut line = String::new();
        let error = new_reader.read_line(&mut line).unwrap_err();
        assert!(matches!(
            error.kind(),
            std::io::ErrorKind::TimedOut | std::io::ErrorKind::WouldBlock
        ));
    });

    let mut client = AgentClient::connect(addr).with_timeout(Duration::from_secs(3));
    assert!(client.gated(press()).unwrap().ok);
    closed_rx.recv_timeout(Duration::from_secs(1)).unwrap();

    let error = client.gated(press()).unwrap_err();
    assert!(is_unsupported_op(&error), "{error}");
    server.join().unwrap();
}
