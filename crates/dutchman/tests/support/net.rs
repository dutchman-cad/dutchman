//! Loopback server support shared by unit and integration tests.
use std::io;
use std::net::{TcpListener, TcpStream};
use std::time::{Duration, Instant};

/// Accept by the deadline, then restore blocking reads on every platform.
/// The listener must be owned by this mock server: this sets it non-blocking.
pub fn accept_blocking(listener: &TcpListener, deadline: Instant) -> io::Result<TcpStream> {
    listener.set_nonblocking(true)?;
    loop {
        match listener.accept() {
            Ok((stream, _)) => {
                stream.set_nonblocking(false)?;
                return Ok(stream);
            }
            Err(error) if error.kind() == io::ErrorKind::WouldBlock => {
                let remaining = deadline.saturating_duration_since(Instant::now());
                if remaining.is_zero() {
                    return Err(io::Error::new(
                        io::ErrorKind::TimedOut,
                        "Mock connection did not arrive before the deadline",
                    ));
                }
                std::thread::sleep(remaining.min(Duration::from_millis(5)));
            }
            Err(error) if error.kind() == io::ErrorKind::Interrupted => {
                if Instant::now() >= deadline {
                    return Err(error);
                }
            }
            Err(error) => return Err(error),
        }
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;
    use std::io::{Read, Write};

    #[test]
    fn accepted_stream_waits_for_delayed_bytes() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let mut client = TcpStream::connect(listener.local_addr().unwrap()).unwrap();
        let mut stream =
            accept_blocking(&listener, Instant::now() + Duration::from_secs(2)).unwrap();
        stream
            .set_read_timeout(Some(Duration::from_secs(2)))
            .unwrap();
        let writer = std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(30));
            client.write_all(b"x").unwrap();
        });
        let mut byte = [0];
        stream.read_exact(&mut byte).unwrap();
        assert_eq!(byte, *b"x");
        writer.join().unwrap();
    }

    #[test]
    fn absent_connection_has_a_deadline() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let start = Instant::now();
        assert_eq!(
            accept_blocking(&listener, start + Duration::from_millis(20))
                .unwrap_err()
                .kind(),
            io::ErrorKind::TimedOut
        );
        assert!(start.elapsed() < Duration::from_secs(2));
    }
}
