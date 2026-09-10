// Run each environment in a fresh process: no unsafe environment mutation and
// no contamination of parallel tests or reqwest's process-wide proxy cache.
use std::{
    io::{Read, Write},
    net::{TcpListener, TcpStream},
    process::Command,
    thread,
    time::{Duration, Instant},
};

#[test]
fn proxy_environment_contract() {
    run_proxy_contract("proxy_child");
}

fn run_proxy_contract(child_test: &str) {
    for mode in ["http", "socks5", "socks5h", "bypass"] {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        listener.set_nonblocking(true).unwrap();
        let server = thread::spawn(move || {
            let deadline = Instant::now() + Duration::from_secs(15);
            let mut socket = loop {
                match listener.accept() {
                    Ok((socket, _)) => break socket,
                    Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                        assert!(Instant::now() < deadline, "proxy was not contacted: {mode}");
                        thread::sleep(Duration::from_millis(10));
                    }
                    Err(error) => panic!("accept: {error}"),
                }
            };
            socket.set_nonblocking(false).unwrap();
            socket
                .set_read_timeout(Some(Duration::from_secs(5)))
                .unwrap();
            socket
                .set_write_timeout(Some(Duration::from_secs(5)))
                .unwrap();
            match mode {
                "http" => {
                    let mut headers = Vec::new();
                    while !headers.ends_with(b"\r\n\r\n") {
                        headers.push(read_bytes(&mut socket, 1)[0]);
                        assert!(headers.len() < 8192);
                    }
                    let headers = String::from_utf8(headers).unwrap().to_lowercase();
                    assert!(headers.starts_with("connect proxy-target.invalid:443 http/1.1\r\n"));
                    assert!(headers.contains("proxy-authorization: basic dxnlcjpwyxnz\r\n"));
                    socket
                        .write_all(b"HTTP/1.1 502 Bad Gateway\r\nContent-Length: 0\r\n\r\n")
                        .unwrap();
                }
                "socks5" | "socks5h" => {
                    assert_eq!(read_bytes(&mut socket, 1), [5]);
                    let count = read_bytes(&mut socket, 1)[0];
                    assert!(read_bytes(&mut socket, count.into()).contains(&2));
                    socket.write_all(&[5, 2]).unwrap();
                    assert_eq!(read_bytes(&mut socket, 2), [1, 4]);
                    assert_eq!(read_bytes(&mut socket, 4), b"user");
                    assert_eq!(read_bytes(&mut socket, 1), [4]);
                    assert_eq!(read_bytes(&mut socket, 4), b"pass");
                    socket.write_all(&[1, 0]).unwrap();
                    assert_eq!(read_bytes(&mut socket, 3), [5, 1, 0]);
                    let kind = read_bytes(&mut socket, 1)[0];
                    if mode == "socks5h" {
                        assert_eq!(kind, 3);
                        let len = read_bytes(&mut socket, 1)[0];
                        assert_eq!(read_bytes(&mut socket, len.into()), b"proxy-target.invalid");
                    } else {
                        match kind {
                            1 => assert_eq!(read_bytes(&mut socket, 4), [127, 0, 0, 1]),
                            4 => assert_eq!(
                                read_bytes(&mut socket, 16),
                                std::net::Ipv6Addr::LOCALHOST.octets()
                            ),
                            3 => {
                                let len = read_bytes(&mut socket, 1)[0];
                                let host = read_bytes(&mut socket, len.into());
                                assert!(
                                    [b"localhost".as_slice(), b"[::1]", b"127.0.0.1"]
                                        .contains(&host.as_slice())
                                );
                            }
                            _ => panic!("unexpected SOCKS address type: {kind}"),
                        }
                    }
                    assert_eq!(read_bytes(&mut socket, 2), [1, 187]);
                    socket.write_all(&[5, 2, 0, 1, 0, 0, 0, 0, 0, 0]).unwrap();
                }
                "bypass" => {
                    // NO_PROXY must connect directly and start TLS, not CONNECT.
                    assert_eq!(read_bytes(&mut socket, 1), [22]);
                }
                _ => unreachable!(),
            }
        });
        let mut command = Command::new(std::env::current_exe().unwrap());
        command.args(["--exact", child_test, "--nocapture"]);
        for name in [
            "HTTP_PROXY",
            "HTTPS_PROXY",
            "ALL_PROXY",
            "NO_PROXY",
            "http_proxy",
            "https_proxy",
            "all_proxy",
            "no_proxy",
            "REQUEST_METHOD",
        ] {
            command.env_remove(name);
        }
        command.env("CPR_PROXY_TEST_MODE", mode);
        let target = if mode == "bypass" {
            command.env("HTTPS_PROXY", "http://127.0.0.1:1");
            command.env("NO_PROXY", "127.0.0.1");
            format!("https://127.0.0.1:{port}/")
        } else {
            // localhost exercises the xAI proxy-host DNS exemption.
            command.env("ALL_PROXY", format!("{mode}://user:pass@localhost:{port}"));
            if mode == "socks5" {
                "https://localhost/".to_owned()
            } else {
                "https://proxy-target.invalid/".to_owned()
            }
        };
        let output = command
            .env("CPR_PROXY_TEST_TARGET", target)
            .output()
            .unwrap();
        server.join().unwrap();
        assert!(
            output.status.success(),
            "{mode}: {} {}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
    }
}

fn read_bytes(socket: &mut TcpStream, count: usize) -> Vec<u8> {
    let mut bytes = vec![0; count];
    socket.read_exact(&mut bytes).unwrap();
    bytes
}

#[tokio::test]
async fn proxy_child() {
    if std::env::var("CPR_PROXY_TEST_MODE").is_err() {
        return;
    }
    let target = std::env::var("CPR_PROXY_TEST_TARGET").unwrap();
    // The mock explicitly rejects the tunnel. Never fall back to a direct route.
    assert!(
        client()
            .get(target)
            .timeout(Duration::from_secs(5))
            .send()
            .await
            .is_err()
    );
}
