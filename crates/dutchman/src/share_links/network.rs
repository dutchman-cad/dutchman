//! Resolve once, screen the complete answer, and give ureq only those socket addresses.
//! A timed wait also bounds platform DNS and TLS implementations that can block internally.

use super::{NETWORK, PRIVATE, UNSAFE};
use std::io::{self, Read};
use std::net::{IpAddr, SocketAddr, ToSocketAddrs};
use std::sync::{mpsc, Arc, Condvar, Mutex};
use std::time::{Duration, Instant};
use url::Url;

// Resource ceilings, including workers left behind by an uninterruptible system resolver.
const MAX_WORKERS: usize = 8;
const MAX_ADDRESSES: usize = 64;
static WORKERS: (Mutex<usize>, Condvar) = (Mutex::new(0), Condvar::new());

struct Permit;
impl Permit {
    fn acquire(deadline: Instant) -> Result<Self, String> {
        let mut count = WORKERS.0.lock().unwrap_or_else(|error| error.into_inner());
        while *count >= MAX_WORKERS {
            let (next, _) = WORKERS
                .1
                .wait_timeout(count, remaining(deadline)?)
                .unwrap_or_else(|error| error.into_inner());
            count = next;
        }
        remaining(deadline)?;
        *count += 1;
        Ok(Self)
    }
}
impl Drop for Permit {
    fn drop(&mut self) {
        let mut count = WORKERS.0.lock().unwrap_or_else(|error| error.into_inner());
        *count = count.saturating_sub(1);
        WORKERS.1.notify_one();
    }
}

fn spawn(deadline: Instant, work: impl FnOnce() + Send + 'static) -> Result<(), String> {
    let permit = Permit::acquire(deadline)?;
    std::thread::Builder::new()
        .name("share-download".into())
        .spawn(move || {
            let _permit = permit;
            work();
        })
        .map_err(|_| NETWORK.to_owned())?;
    Ok(())
}

fn remaining(deadline: Instant) -> Result<Duration, String> {
    deadline
        .checked_duration_since(Instant::now())
        .filter(|duration| !duration.is_zero())
        .ok_or_else(|| NETWORK.to_owned())
}

fn bounded<T: Send + 'static>(
    deadline: Instant,
    work: impl FnOnce() -> Result<T, String> + Send + 'static,
) -> Result<T, String> {
    let (send, receive) = mpsc::sync_channel(1);
    spawn(deadline, move || {
        // Queued/scheduled work must not contact a host after the caller timed out.
        let result = remaining(deadline).and_then(|_| work());
        let _ = send.send(result);
    })?;
    receive
        .recv_timeout(remaining(deadline)?)
        .map_err(|_| NETWORK.to_owned())?
}

/// A network worker owns only the response stream. The caller owns all scratch files,
/// so timing out a body read closes the output before its folder is removed, on every OS.
/// The channel queues at most one chunk; reads use fixed-size buffers.
pub(super) fn reader(
    mut source: Box<dyn Read + Send + Sync>,
    deadline: Instant,
) -> Result<impl Read, String> {
    let (send, receive) = mpsc::sync_channel(1);
    spawn(deadline, move || loop {
        if remaining(deadline).is_err() {
            break;
        }
        let mut chunk = vec![0; 64 << 10];
        let result = source.read(&mut chunk).map(|count| {
            chunk.truncate(count);
            chunk
        });
        let done = !matches!(&result, Ok(chunk) if !chunk.is_empty());
        if send.send(result).is_err() || done {
            break;
        }
    })?;
    Ok(Body {
        ended: false,
        receive,
        chunk: io::Cursor::new(Vec::new()),
        deadline,
    })
}

struct Body {
    ended: bool,
    receive: mpsc::Receiver<io::Result<Vec<u8>>>,
    chunk: io::Cursor<Vec<u8>>,
    deadline: Instant,
}
impl Read for Body {
    fn read(&mut self, output: &mut [u8]) -> io::Result<usize> {
        if output.is_empty() || self.ended {
            return Ok(0);
        }
        let budget = remaining(self.deadline)
            .map_err(|message| io::Error::new(io::ErrorKind::TimedOut, message))?;
        if self.chunk.position() >= self.chunk.get_ref().len() as u64 {
            self.chunk = io::Cursor::new(
                self.receive
                    .recv_timeout(budget)
                    .map_err(|_| io::Error::new(io::ErrorKind::TimedOut, NETWORK))??,
            );
            self.ended = self.chunk.get_ref().is_empty();
        }
        self.chunk.read(output)
    }
}

/// Only globally routed unicast destinations. Reject a mixed answer in its entirety so
/// fallback can never dial a restricted address. IPv4-mapped IPv6 uses the IPv4 policy.
fn public(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(ip) => {
            let bits = u32::from(ip);
            ![
                (0x0000_0000, 8),  // This network / unspecified
                (0x0a00_0000, 8),  // Private
                (0x6440_0000, 10), // CGNAT (also cloud metadata at 100.100.100.200)
                (0x7f00_0000, 8),  // Loopback
                (0xa9fe_0000, 16), // Link-local / cloud metadata
                (0xac10_0000, 12), // Private
                (0xc000_0000, 24), // IETF special-purpose
                (0xc000_0200, 24), // Documentation
                (0xc058_6300, 24), // Deprecated relay
                (0xc0a8_0000, 16), // Private
                (0xc612_0000, 15), // Benchmarking
                (0xc633_6400, 24), // Documentation
                (0xcb00_7100, 24), // Documentation
                (0xe000_0000, 3),  // Multicast, reserved and broadcast
            ]
            .into_iter()
            .any(|(network, prefix)| bits & (u32::MAX << (32 - prefix)) == network)
            // Azure's platform virtual IP is public-looking but serves host metadata.
            && bits != 0xa83f_8110
        }
        IpAddr::V6(ip) => {
            if let Some(mapped) = ip.to_ipv4_mapped() {
                return public(IpAddr::V4(mapped));
            }
            let bits = u128::from(ip);
            // Allow global unicast only, excluding special-purpose / transition ranges.
            bits >> 125 == 1
                && bits >> 105 != (0x2001 << 7) // 2001::/23 (includes Teredo and benchmarks)
                && bits >> 96 != 0x2001_0db8 // Documentation
                && bits >> 112 != 0x2002 // 6to4 can embed a private IPv4 destination
                && bits >> 108 != 0x3fff0 // Documentation (3fff::/20)
        }
    }
}

fn screen(addresses: Vec<SocketAddr>) -> Result<Vec<SocketAddr>, String> {
    if addresses.is_empty() || addresses.len() > MAX_ADDRESSES {
        return Err(NETWORK.into());
    }
    if addresses.iter().any(|address| !public(address.ip())) {
        return Err(UNSAFE.into());
    }
    Ok(addresses)
}

pub(super) fn fetch(url: &Url, deadline: Instant) -> Result<ureq::Response, String> {
    fetch_using(
        url,
        deadline,
        Arc::new(|netloc: &str| {
            netloc
                .to_socket_addrs()
                .map(|addresses| addresses.take(MAX_ADDRESSES + 1).collect())
        }),
        request,
    )
}

fn fetch_using(
    url: &Url,
    deadline: Instant,
    resolver: Arc<dyn ureq::Resolver>,
    connect: impl FnOnce(Url, Vec<SocketAddr>, Instant) -> Result<ureq::Response, String>
        + Send
        + 'static,
) -> Result<ureq::Response, String> {
    let netloc = format!(
        "{}:{}",
        url.host_str().ok_or_else(|| UNSAFE.to_owned())?,
        url.port_or_known_default()
            .ok_or_else(|| UNSAFE.to_owned())?
    );
    let addresses = bounded(deadline, move || {
        screen(resolver.resolve(&netloc).map_err(|_| NETWORK.to_owned())?)
    })?;
    let url = url.clone();
    bounded(deadline, move || connect(url, addresses, deadline))
}

fn request(
    url: Url,
    addresses: Vec<SocketAddr>,
    deadline: Instant,
) -> Result<ureq::Response, String> {
    let budget = remaining(deadline)?;
    // Fresh agent per hop: neither a pooled socket nor a second DNS lookup can bypass
    // screening. The URL hostname is retained for Host, SNI and certificate validation.
    let agent = ureq::AgentBuilder::new()
        .redirects(0)
        .try_proxy_from_env(false)
        .timeout_connect(budget)
        .timeout_read(budget)
        .timeout_write(budget)
        .resolver(move |_: &str| -> io::Result<Vec<SocketAddr>> { Ok(addresses.clone()) })
        .build();
    agent
        .get(url.as_str())
        .timeout(remaining(deadline)?)
        .call()
        .map_err(|error| match error {
            ureq::Error::Status(_, _) => PRIVATE.to_owned(),
            ureq::Error::Transport(_) => NETWORK.to_owned(),
        })
}

#[cfg(test)]
mod tests;
