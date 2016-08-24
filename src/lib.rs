#[macro_use]
extern crate error_chain;
#[macro_use]
extern crate log;
use std::collections::BTreeMap;
use std::net::{TcpStream, ToSocketAddrs};
use std::io::{self, BufWriter, BufReader, BufRead, Write, Read};
use std::time::{Duration, SystemTime};
use std::cmp;

mod errors;
use errors::*;

pub struct Client {
    wr: BufWriter<TcpStream>,
    rdr: BufReader<TcpStream>,
    pace: PaceMaker,
}

pub enum AckMode {
    Auto,
    ClientIndividual,
}

impl AckMode {
    fn as_str(&self) -> &'static str {
        match self {
            &AckMode::Auto => "auto",
            &AckMode::ClientIndividual => "client-individual",
        }
    }
}

pub type Headers = BTreeMap<String, String>;

fn parse_keepalive(headervalue: Option<&str>) -> Result<(Option<Duration>, Option<Duration>)> {
    if let Some(sxsy) = headervalue {
        info!("heartbeat: theirs:{:?}", sxsy);
        let mut it = sxsy.trim().splitn(2, ',');
        let sx = Duration::from_millis(try!(try!(it.next().ok_or(ErrorKind::ProtocolError))
                                                .parse()));
        let sy = Duration::from_millis(try!(try!(it.next().ok_or(ErrorKind::ProtocolError))
                                                .parse()));
        info!("heartbeat: theirs:{:?}", (&sx, &sy));

        Ok((some_non_zero(sx), some_non_zero(sy)))
    } else {
        Ok((None, None))
    }
}

fn some_non_zero(dur: Duration) -> Option<Duration> {
    if dur == Duration::from_millis(0) {
        None
    } else {
        Some(dur)
    }
}


impl Client {
    pub fn connect<A: ToSocketAddrs>(a: A,
                                     credentials: Option<(&str, &str)>,
                                     keepalive: Option<Duration>)
                                     -> Result<Self> {
        let wr = try!(TcpStream::connect(a));
        debug!("connected to: {:?}", try!(wr.peer_addr()));
        try!(wr.set_read_timeout(keepalive.map(|d| d * 3)));
        let rdr = try!(wr.try_clone());
        let mut conn_headers = BTreeMap::new();
        let mut client = Client {
            wr: BufWriter::new(wr),
            rdr: BufReader::new(rdr),
            pace: PaceMaker::default(),
        };
        conn_headers.insert("accept-version".to_string(), "1.2".to_string());
        if let &Some(ref duration) = &keepalive {
            let millis = duration.as_secs() * 1000 + duration.subsec_nanos() as u64 / 1000_000;
            conn_headers.insert("heart-beat".to_string(), format!("{},{}", millis, millis));
        }
        if let Some((user, pass)) = credentials {
            conn_headers.insert("login".to_string(), user.to_string());
            conn_headers.insert("passcode".to_string(), pass.to_string());
        }
        try!(client.send("CONNECT", conn_headers, &[]));

        let (cmd, hdrs, body) = try!(client.read_frame());
        if &cmd == "ERROR" {
            let body = String::from_utf8_lossy(&body).into_owned();
            warn!("Error response from server: {:?}: {:?}", cmd, hdrs);
            return Err(ErrorKind::StompError(cmd, hdrs, body).into());
        } else if &cmd != "CONNECTED" {
            warn!("Bad response from server: {:?}: {:?}", cmd, hdrs);
            return Err(ErrorKind::ProtocolError.into());
        }

        let (sx, sy) = try!(parse_keepalive(hdrs.get("heart-beat").map(|s| &**s)));
        client.pace = PaceMaker::new(keepalive, sx, sy);

        try!(client.wr.get_mut().set_read_timeout(client.pace.read_timeout()));
        try!(client.rdr.get_mut().set_read_timeout(client.pace.read_timeout()));

        Ok(client)
    }

    pub fn subscribe(&mut self, destination: &str, id: &str, mode: AckMode) -> Result<()> {
        let mut h = BTreeMap::new();
        h.insert("destination".to_string(), destination.to_string());
        h.insert("id".to_string(), id.to_string());
        h.insert("ack".to_string(), mode.as_str().to_string());
        try!(self.send("SUBSCRIBE", h, b""));
        Ok(())
    }
    pub fn publish(&mut self, destination: &str, body: &[u8]) -> Result<()> {
        let mut h = BTreeMap::new();
        h.insert("destination".to_string(), destination.to_string());
        h.insert("content-length".to_string(), format!("{}", body.len()));
        try!(self.send("SEND", h, body));
        Ok(())
    }
    pub fn ack(&mut self, headers: &Headers) -> Result<()> {
        let mut h = BTreeMap::new();
        let mid = try!(headers.get("ack").ok_or(ErrorKind::NoAckHeader));
        h.insert("id".to_string(), mid.to_string());
        try!(self.send("ACK", h, &[]));
        Ok(())
    }


    pub fn consume_next(&mut self) -> Result<(Headers, Vec<u8>)> {
        let (cmd, hdrs, body) = try!(self.read_frame());
        if &cmd != "MESSAGE" {
            warn!("Bad message from server: {:?}: {:?}", cmd, hdrs);
            return Err(ErrorKind::ProtocolError.into());
        }

        Ok((hdrs, body))
    }

    fn send(&mut self,
            command: &str,
            headers: BTreeMap<String, String>,
            body: &[u8])
            -> Result<()> {

        try!(writeln!(self.wr, "{}", command));
        for (k, v) in headers {
            try!(writeln!(self.wr, "{}:{}", k, v));
        }
        try!(writeln!(self.wr, ""));

        try!(self.wr.write_all(body));
        try!(self.wr.write(b"\0"));
        try!(self.wr.flush());
        self.pace.write_observed(SystemTime::now());
        Ok(())
    }

    fn read_line(&mut self, buf: &mut String) -> Result<()> {
        loop {
            let result = self.rdr.read_line(buf);
            debug!("read line result: {:?}", result);
            match result {
                Ok(_) => {
                    self.pace.read_observed(SystemTime::now());
                    return Ok(());
                }
                Err(ref e) if e.kind() == io::ErrorKind::WouldBlock => {
                    match try!(self.pace.handle_read_timeout(SystemTime::now())) {
                        BeatAction::Retry => continue,
                        BeatAction::SendClientHeart => {
                            try!(self.wr.write_all(b"\n"));
                            try!(self.wr.flush());
                            self.pace.write_observed(SystemTime::now());
                            debug!("Sent heartbeat");
                        }
                        _ => unimplemented!(),
                    };
                }
                Err(e) => {
                    warn!("Read returned: kind:{:?}", e.kind());
                    return Err(e.into());
                }
            };
        }
    }

    fn read_frame(&mut self) -> Result<(String, Headers, Vec<u8>)> {
        let mut buf = String::new();
        while buf.trim().is_empty() {
            buf.clear();
            try!(self.read_line(&mut buf));
            trace!("Read command line: {:?}", buf);
            assert!(!buf.is_empty());
        }
        let command = buf.trim().to_string();

        let mut headers = BTreeMap::new();
        loop {
            buf.clear();
            try!(self.rdr.read_line(&mut buf));
            trace!("Read header line: {:?}", buf);
            if buf == "\n" {
                break;
            }
            let mut it = buf.trim().splitn(2, ':');
            let name = try!(it.next().ok_or(ErrorKind::ProtocolError));
            let value = try!(it.next().ok_or(ErrorKind::ProtocolError));
            headers.insert(name.to_string(), value.to_string());
        }
        trace!("Reading body");
        let mut buf = Vec::new();
        if let Some(lenstr) = headers.get("content-length") {
            let nbytes: u64 = try!(lenstr.parse());
            trace!("Read bytes: {}", nbytes);
            try!(self.rdr.by_ref().take(nbytes + 1).read_to_end(&mut buf));
        } else {
            trace!("Read until nul");
            try!(self.rdr.read_until(b'\0', &mut buf));
        }
        trace!("Read body: {:?}", buf);
        if buf.pop() != Some(b'\0') {
            warn!("No null at end of body");
            return Err(ErrorKind::ProtocolError.into());
        }

        let frame = (command, headers, buf);
        trace!("read frame: {:?}", frame);
        Ok(frame)
    }
}



#[derive(Debug, Clone, Default)]
struct PaceMaker {
    client_to_server: Option<Duration>,
    server_to_client: Option<Duration>,
    last_observed_read: Option<SystemTime>,
    last_observed_write: Option<SystemTime>,
}

#[derive(Debug, Clone, PartialEq,Eq)]
enum BeatAction {
    Retry,
    PeerFailed,
    SendClientHeart,
}

impl PaceMaker {
    fn new(keepalive: Option<Duration>, sx: Option<Duration>, sy: Option<Duration>) -> Self {
        debug!("heart-beat: cx, cy:{:?}; server-transmit:{:?}; server-receive:{:?}",
               keepalive,
               sx,
               sy);
        let client_to_server = sy.and_then(|sy| keepalive.map(|cx| cmp::max(cx, sy)));
        let server_to_client = sx.and_then(|sx| keepalive.map(|cy| cmp::max(cy, sx)));

        PaceMaker {
            client_to_server: client_to_server,
            server_to_client: server_to_client,
            last_observed_read: None,
            last_observed_write: None,
        }
    }

    fn read_timeout(&self) -> Option<Duration> {
        match (self.server_to_client, self.client_to_server) {
            (Some(s2c), Some(c2s)) => Some(cmp::min(c2s / 2, s2c * 2)),
            (Some(s2c), None) => Some(s2c * 2),
            (None, Some(c2s)) => Some(c2s / 2),
            _ => None,
        }
    }

    fn read_observed(&mut self, at: SystemTime) {
        self.last_observed_read = Some(at);
        debug!("last_observed_read now: {:?}", at);
    }
    fn write_observed(&mut self, at: SystemTime) {
        self.last_observed_write = Some(at);
        debug!("last_observed_write now: {:?}", at);
    }

    fn handle_read_timeout(&mut self, at: SystemTime) -> Result<BeatAction> {
        debug!("handle_read_timeout: {:?} at {:?}", self, &at);
        if let (Some(mark), Some(interval)) = (self.last_observed_write, self.client_to_server) {
            let duration = try!(at.duration_since(mark));
            debug!("consider sending heartbeat after: {:?} - {:?} -> {:?}",
                   mark,
                   at,
                   duration);
            if duration >= interval {
                debug!("Should send beat");
                return Ok(BeatAction::SendClientHeart);
            }
        }

        if let (Some(mark), Some(interval)) = (self.last_observed_read, self.server_to_client) {
            let duration = try!(at.duration_since(mark));
            debug!("considering if alive after: {:?} - {:?} -> {:?}",
                   mark,
                   at,
                   duration);
            if duration < interval * 2 {
                debug!("Should retry");
                Ok(BeatAction::Retry)
            } else {
                debug!("Peer dead");
                Ok(BeatAction::PeerFailed)
            }
        } else {
            // Uh, no idea.
            debug!("No heartbeats");
            Ok(BeatAction::Retry)
        }
    }
}



#[cfg(test)]
mod test {
    extern crate env_logger;
    use super::{parse_keepalive, PaceMaker, BeatAction};
    use std::time::{SystemTime, Duration};

    #[test]
    fn keepalives_parse_zero_as_none_0() {
        env_logger::init().unwrap_or(());
        assert_eq!(parse_keepalive(Some("0,0")).expect("parse_keepalive"),
                   (None, None));
    }

    #[test]
    fn keepalives_parse_zero_as_none_1() {
        env_logger::init().unwrap_or(());
        assert_eq!(parse_keepalive(Some("0,42")).expect("parse_keepalive"),
                   (None, Some(Duration::from_millis(42))));
    }

    #[test]
    fn keepalives_parse_zero_as_none_2() {
        env_logger::init().unwrap_or(());
        assert_eq!(parse_keepalive(Some("42,0")).expect("parse_keepalive"),
                   (Some(Duration::from_millis(42)), None));
    }


    #[test]
    fn pacemaker_blah_blah_blah() {
        env_logger::init().unwrap_or(());
        let pm = PaceMaker::new(None, None, None);
        assert_eq!(pm.read_timeout(), None);
    }

    #[test]
    fn pacemaker_inf_read_timeout_when_server_unsupported() {
        env_logger::init().unwrap_or(());
        let pm = PaceMaker::new(Some(Duration::from_millis(20)), None, None);
        println!("pm: {:?}", pm);
        println!("read_timeout: {:?}", pm.read_timeout());
        assert_eq!(pm.read_timeout(), None);
    }

    #[test]
    fn pacemaker_read_timeout_max_of_ours_and_server_send_rate_times_two() {
        env_logger::init().unwrap_or(());
        let pm = PaceMaker::new(Some(Duration::from_millis(20)),
                                Some(Duration::from_millis(10)),
                                None);
        println!("pm: {:?}", pm);
        println!("read_timeout: {:?}", pm.read_timeout());
        assert_eq!(pm.read_timeout(), Some(Duration::from_millis(40)));
    }

    #[test]
    fn pacemaker_read_timeout_max_of_ours_and_server_send_rate_times_two_2() {
        env_logger::init().unwrap_or(());
        let pm = PaceMaker::new(Some(Duration::from_millis(20)),
                                Some(Duration::from_millis(30)),
                                None);
        println!("pm: {:?}", pm);
        println!("read_timeout: {:?}", pm.read_timeout());
        assert_eq!(pm.read_timeout(), Some(Duration::from_millis(60)));
    }

    #[test]
    fn pacemaker_read_timeout_should_be_half_client_heartbeat() {
        env_logger::init().unwrap_or(());
        let pm = PaceMaker::new(Some(Duration::from_millis(10)),
                                None,
                                Some(Duration::from_millis(30)));
        println!("pm: {:?}", pm);
        println!("read_timeout: {:?}", pm.read_timeout());
        assert_eq!(pm.read_timeout(), Some(Duration::from_millis(15)));
    }

    #[test]
    fn pacemaker_read_timeout_should_be_min_of_client_and_twice_server_heartbeat_1() {
        env_logger::init().unwrap_or(());
        // Client wants to send/receive heartbeats every 10ms
        // Server wants to send every 10ms, receive every 30ms.
        // -> We need to send one every 30ms, we expect one every 10ms.
        // So if we don't see any reads after 20ms, we consider the peer dead.
        let pm = PaceMaker::new(Some(Duration::from_millis(10)),
                                Some(Duration::from_millis(10)),
                                Some(Duration::from_millis(30)));
        println!("pm: {:?}", pm);
        println!("read_timeout: {:?}", pm.read_timeout());
        assert_eq!(pm.client_to_server, Some(Duration::from_millis(30)));
        assert_eq!(pm.server_to_client, Some(Duration::from_millis(10)));
        assert_eq!(pm.read_timeout(), Some(Duration::from_millis(15)));
    }

    #[test]
    fn pacemaker_read_timeout_should_be_min_of_client_and_twice_server_heartbeat_2() {
        env_logger::init().unwrap_or(());
        let pm = PaceMaker::new(Some(Duration::from_millis(10)),
                                Some(Duration::from_millis(10)),
                                Some(Duration::from_millis(2)));
        println!("pm: {:?}", pm);
        println!("read_timeout: {:?}", pm.read_timeout());
        assert_eq!(pm.client_to_server, Some(Duration::from_millis(10)));
        assert_eq!(pm.server_to_client, Some(Duration::from_millis(10)));
        assert_eq!(pm.read_timeout(), Some(Duration::from_millis(5)));
    }

    #[test]
    fn pacemaker_read_timeout_should_be_min_of_client_and_twice_server_heartbeat_3() {
        // Client wants to send/receive heartbeats every 2ms
        // Server wants to send every 10ms, receive every 2ms.
        // -> We need to send one every 2ms, we expect one every 10ms.
        // So if we don't see any reads after 2ms, wakeup and send frame.
        env_logger::init().unwrap_or(());
        let pm = PaceMaker::new(Some(Duration::from_millis(2)),
                                Some(Duration::from_millis(10)),
                                Some(Duration::from_millis(2)));
        println!("pm: {:?}", pm);
        println!("read_timeout: {:?}", pm.read_timeout());
        assert_eq!(pm.client_to_server, Some(Duration::from_millis(2)));
        assert_eq!(pm.server_to_client, Some(Duration::from_millis(10)));
        assert_eq!(pm.read_timeout(), Some(Duration::from_millis(1)));
    }

    #[test]
    fn pacemaker_should_yield_failure_after_twice_server_heartbeat_interval() {
        env_logger::init().unwrap_or(());
        let start = SystemTime::now();
        let mut pm = PaceMaker::new(Some(Duration::from_millis(10)),
                                    Some(Duration::from_millis(10)),
                                    None);
        pm.read_observed(start);
        assert_eq!(pm.handle_read_timeout(start + Duration::from_millis(19))
                     .expect("handle_read_timeout"),
                   BeatAction::Retry);
        assert_eq!(pm.handle_read_timeout(start + Duration::from_millis(20))
                     .expect("handle_read_timeout"),
                   BeatAction::PeerFailed);
    }

    #[test]
    fn pacemaker_should_yield_client_heartbeat_after_client_heartbeat_interval() {
        env_logger::init().unwrap_or(());
        let start = SystemTime::now();
        let mut pm = PaceMaker::new(Some(Duration::from_millis(10)),
                                    None,
                                    Some(Duration::from_millis(10)));
        pm.write_observed(start);
        assert_eq!(pm.handle_read_timeout(start + Duration::from_millis(9))
                     .expect("handle_read_timeout"),
                   BeatAction::Retry);
        assert_eq!(pm.handle_read_timeout(start + Duration::from_millis(10))
                     .expect("handle_read_timeout"),
                   BeatAction::SendClientHeart);
    }
}
