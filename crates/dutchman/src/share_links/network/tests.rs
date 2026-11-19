#![allow(clippy::unwrap_used, clippy::indexing_slicing)]

use super::*;
use crate::share_links::{resolve, Provider};
use std::io::{Read, Write};
use std::net::TcpListener;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

fn deadline() -> Instant {
    Instant::now() + Duration::from_secs(3)
}

fn resolver(addresses: Vec<SocketAddr>) -> Arc<dyn ureq::Resolver> {
    Arc::new(move |_: &str| Ok(addresses.clone()))
}

#[test]
fn fake_resolver_rejects_every_restricted_range_before_connecting() {
    let restricted = [
        "0.0.0.0",
        "0.1.2.3",
        "10.0.0.1",
        "10.255.255.255",
        "100.64.0.0",
        "100.100.100.200",
        "100.127.255.255",
        "127.0.0.1",
        "127.255.255.255",
        "169.254.0.1",
        "169.254.169.254",
        "172.16.0.0",
        "172.31.255.255",
        "192.168.0.1",
        "192.168.255.255",
        "192.0.0.1",
        "192.0.2.1",
        "192.88.99.1",
        "198.18.0.1",
        "198.19.255.255",
        "198.51.100.1",
        "203.0.113.1",
        "224.0.0.1",
        "239.255.255.255",
        "240.0.0.1",
        "255.255.255.255",
        "168.63.129.16",
        "::",
        "::1",
        "::127.0.0.1",
        "fc00::1",
        "fd00:ec2::254",
        "fe80::1",
        "febf::1",
        "fec0::1",
        "ff02::1",
        "ff0e::1",
        "64:ff9b::a9fe:a9fe",
        "64:ff9b:1::1",
        "100::1",
        "2001::1",
        "2001:2::1",
        "2001:db8::1",
        "2002:7f00:1::",
        "3fff::1",
    ];
    let mut addresses: Vec<IpAddr> = restricted.iter().map(|ip| ip.parse().unwrap()).collect();
    // Every denied IPv4 class is also denied in IPv4-mapped IPv6 form.
    addresses.extend(restricted.iter().filter_map(|ip| {
        ip.parse::<std::net::Ipv4Addr>()
            .ok()
            .map(|ip| IpAddr::V6(ip.to_ipv6_mapped()))
    }));
    let url = Url::parse("https://drive.google.com/uc?id=synthetic").unwrap();
    for address in addresses {
        let called = Arc::new(AtomicBool::new(false));
        let connected = called.clone();
        let result = fetch_using(
            &url,
            deadline(),
            resolver(vec![SocketAddr::new(address, 443)]),
            move |_, _, _| {
                connected.store(true, Ordering::SeqCst);
                Err("Unexpected connection".into())
            },
        );
        assert_eq!(result.unwrap_err(), UNSAFE, "{address}");
        assert!(!called.load(Ordering::SeqCst), "{address}");
    }
}

#[test]
fn screened_answers_are_pinned_without_resolving_again() {
    let calls = Arc::new(AtomicUsize::new(0));
    let counted = calls.clone();
    let answers: Vec<SocketAddr> = [
        "8.8.8.8:443",
        "1.1.1.1:443",
        "100.63.255.255:443",
        "100.128.0.0:443",
        "172.15.255.255:443",
        "172.32.0.0:443",
        "192.167.255.255:443",
        "192.169.0.0:443",
        "[2606:4700:4700::1111]:443",
        "[2001:4860:4860::8888]:443",
        "[::ffff:8.8.4.4]:443",
    ]
    .iter()
    .map(|ip| ip.parse().unwrap())
    .collect();
    let expected = answers.clone();
    let result = fetch_using(
        &Url::parse("https://drive.google.com/uc?id=synthetic").unwrap(),
        deadline(),
        Arc::new(move |host: &str| {
            assert_eq!(host, "drive.google.com:443");
            assert_eq!(counted.fetch_add(1, Ordering::SeqCst), 0);
            Ok(answers.clone())
        }),
        move |url, pinned, _| {
            assert_eq!(url.host_str(), Some("drive.google.com"));
            assert_eq!(pinned, expected);
            Ok(ureq::Response::new(200, "OK", "synthetic").unwrap())
        },
    );
    assert_eq!(result.unwrap().status(), 200);
    assert_eq!(calls.load(Ordering::SeqCst), 1);
}

#[test]
fn mixed_fallback_answers_and_excessive_answers_are_refused() {
    assert_eq!(
        screen(vec![
            "8.8.8.8:443".parse().unwrap(),
            "127.0.0.1:443".parse().unwrap()
        ])
        .unwrap_err(),
        UNSAFE
    );
    assert_eq!(screen(vec![]).unwrap_err(), NETWORK);
    assert_eq!(
        screen(vec!["8.8.8.8:443".parse().unwrap(); MAX_ADDRESSES + 1]).unwrap_err(),
        NETWORK
    );
}

#[test]
fn allowed_redirect_host_with_private_dns_never_opens_a_socket() {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    listener.set_nonblocking(true).unwrap();
    let address = listener.local_addr().unwrap();
    let mut requests = 0;
    let error = resolve(
        Provider::Fusion,
        Url::parse("https://a360.co/synthetic").unwrap(),
        100,
        Duration::from_secs(3),
        |url, _| {
            requests += 1;
            if requests == 1 {
                // Parse a synthetic response with a real redirect header.
                Ok("HTTP/1.1 302 Found\r\nLocation: https://download.autodesk.com/cube.f3d\r\nContent-Length: 0\r\n\r\n".parse().unwrap())
            } else {
                assert_eq!(url.host_str(), Some("download.autodesk.com"));
                fetch_using(url, deadline(), resolver(vec![address]), request)
            }
        },
    ).unwrap_err();
    assert_eq!(requests, 2);
    assert_eq!(error, UNSAFE);
    assert_eq!(
        listener.accept().unwrap_err().kind(),
        io::ErrorKind::WouldBlock
    );
}

#[test]
fn stalled_dns_is_bounded_and_late_resolution_cannot_connect() {
    let (started, waiting) = mpsc::channel();
    let (release, blocked) = mpsc::channel();
    let blocked = std::sync::Mutex::new(blocked);
    let called = Arc::new(AtomicBool::new(false));
    let connected = called.clone();
    let began = Instant::now();
    let error = fetch_using(
        &Url::parse("https://drive.google.com/uc?id=synthetic").unwrap(),
        began + Duration::from_millis(100),
        Arc::new(move |_: &str| {
            started.send(()).unwrap();
            blocked.lock().unwrap().recv().unwrap();
            Ok(vec!["8.8.8.8:443".parse().unwrap()])
        }),
        move |_, _, _| {
            connected.store(true, Ordering::SeqCst);
            Err("Unexpected connection".into())
        },
    )
    .unwrap_err();
    waiting.recv_timeout(Duration::from_secs(1)).unwrap();
    assert_eq!(error, NETWORK);
    assert!(began.elapsed() < Duration::from_secs(1));
    release.send(()).unwrap();
    assert!(!called.load(Ordering::SeqCst));
}

#[test]
fn connect_receives_only_the_budget_left_after_dns() {
    let began = Instant::now();
    let end = began + Duration::from_millis(300);
    let (record, recorded) = mpsc::channel();
    let result = fetch_using(
        &Url::parse("https://drive.google.com/uc?id=synthetic").unwrap(),
        end,
        Arc::new(|_: &str| {
            std::thread::sleep(Duration::from_millis(180));
            Ok(vec!["8.8.8.8:443".parse().unwrap()])
        }),
        move |_, _, deadline| {
            record.send((deadline, remaining(deadline))).unwrap();
            // A stalled connector/TLS implementation still cannot hold up its caller.
            std::thread::sleep(Duration::from_millis(300));
            Err(NETWORK.into())
        },
    );
    assert_eq!(result.unwrap_err(), NETWORK);
    let (deadline, budget) = recorded.recv_timeout(Duration::from_secs(1)).unwrap();
    assert_eq!(deadline, end);
    assert!(budget.unwrap() <= Duration::from_millis(120));
    assert!(began.elapsed() < Duration::from_millis(700));
}

#[test]
fn actual_request_bounds_tls_and_slow_body_with_remaining_budget() {
    for tls in [true, false] {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        listener.set_nonblocking(true).unwrap();
        let server = std::thread::spawn(move || {
            let end = Instant::now() + Duration::from_secs(2);
            let mut socket = loop {
                match listener.accept() {
                    Ok((socket, _)) => break socket,
                    Err(error)
                        if error.kind() == io::ErrorKind::WouldBlock && Instant::now() < end =>
                    {
                        std::thread::sleep(Duration::from_millis(5));
                    }
                    _ => return,
                }
            };
            // macOS/BSD: an accepted socket inherits the listener's non-blocking mode.
            socket.set_nonblocking(false).unwrap();
            socket
                .set_read_timeout(Some(Duration::from_secs(2)))
                .unwrap();
            let _ = socket.read(&mut [0; 4096]);
            if !tls {
                let _ = socket.write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 20\r\n\r\nx");
            }
            std::thread::sleep(Duration::from_millis(350));
            let _ = socket.write_all(b"x");
        });
        let began = Instant::now();
        let url = Url::parse(if tls {
            "https://drive.google.com/"
        } else {
            "http://drive.google.com/"
        })
        .unwrap();
        // Direct transport injection tests deadlines locally; production fetch always screens first.
        let response = bounded(began + Duration::from_millis(150), move || {
            request(url, vec![address], began + Duration::from_millis(150))
        });
        if tls {
            assert_eq!(response.unwrap_err(), NETWORK);
        } else {
            let mut body = String::new();
            assert!(response
                .unwrap()
                .into_reader()
                .read_to_string(&mut body)
                .is_err());
        }
        assert!(began.elapsed() < Duration::from_millis(600));
        server.join().unwrap();
    }
}
