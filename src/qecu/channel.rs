//! Host-owned, byte-oriented "Channel" layer.
//!
//! A *channel* is a bidirectional byte stream between a host client (e.g. a
//! Python Fastboot client speaking ZeroMQ) and the guest (the ABL Fastboot
//! stack running inside QECU).  The WASM plugin never opens sockets: it only
//! enqueues/dequeues `ChannelFrame`s, and a dedicated transport task owns the
//! ZeroMQ socket.
//!
//! Wire format (ZeroMQ multipart, after the ROUTER/DEALER identity frame):
//!
//! ```text
//! frame[0] = channel name   (UTF-8 bytes)
//! frame[1] = sequence       (u64, little-endian)
//! frame[2] = kind           (u8: 0=control 1=bulk_out 2=bulk_in 3=status 4=error)
//! frame[3] = payload        (raw bytes; NOT JSON/base64)
//! ```
//!
//! The first channel is `fastboot` (bulk_out = host -> guest command bytes,
//! bulk_in = guest -> host response bytes).  The broker validates every frame
//! (known channel, size, per-session monotonic sequence, queue depth) and
//! routes outbound frames back to the originating DEALER identity.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::collections::VecDeque;
use std::sync::mpsc;

pub const DEFAULT_MAX_FRAME_BYTES: usize = 1_048_576;
pub const DEFAULT_QUEUE_DEPTH: usize = 32;
pub const DEFAULT_TRANSPORT: &str = "zeromq";

// ---------------------------------------------------------------------------
// Configuration
// ---------------------------------------------------------------------------

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ChannelConfig {
    pub name: String,
    #[serde(default = "default_transport")]
    pub transport: String,
    #[serde(default)]
    pub endpoint: Option<String>,
    #[serde(default = "default_max_frame_bytes")]
    pub max_frame_bytes: usize,
    #[serde(default = "default_queue_depth")]
    pub queue_depth: usize,
}

fn default_transport() -> String {
    DEFAULT_TRANSPORT.to_string()
}

fn default_max_frame_bytes() -> usize {
    DEFAULT_MAX_FRAME_BYTES
}

fn default_queue_depth() -> usize {
    DEFAULT_QUEUE_DEPTH
}

impl ChannelConfig {
    pub fn fastboot() -> ChannelConfig {
        ChannelConfig {
            name: "fastboot".to_string(),
            transport: DEFAULT_TRANSPORT.to_string(),
            endpoint: Some("ipc:///tmp/qecu-pixel9-fastboot.sock".to_string()),
            max_frame_bytes: DEFAULT_MAX_FRAME_BYTES,
            queue_depth: DEFAULT_QUEUE_DEPTH,
        }
    }
}

// ---------------------------------------------------------------------------
// Frame model
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ChannelKind {
    Control = 0,
    BulkOut = 1,
    BulkIn = 2,
    Status = 3,
    Error = 4,
}

impl ChannelKind {
    pub fn from_byte(b: u8) -> Option<ChannelKind> {
        match b {
            0 => Some(ChannelKind::Control),
            1 => Some(ChannelKind::BulkOut),
            2 => Some(ChannelKind::BulkIn),
            3 => Some(ChannelKind::Status),
            4 => Some(ChannelKind::Error),
            _ => None,
        }
    }

    pub fn as_byte(self) -> u8 {
        self as u8
    }
}

/// A validated, decoded channel frame.  `session_id` is the raw DEALER
/// identity that the ROUTER socket prepends to every inbound message.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ChannelFrame {
    pub channel: String,
    pub session_id: Vec<u8>,
    pub sequence: u64,
    pub kind: ChannelKind,
    pub payload: Vec<u8>,
}

impl ChannelFrame {
    /// Encode to the four wire frames (identity frame excluded).
    pub fn encode_wire(&self) -> Vec<Vec<u8>> {
        vec![
            self.channel.as_bytes().to_vec(),
            self.sequence.to_le_bytes().to_vec(),
            vec![self.kind.as_byte()],
            self.payload.clone(),
        ]
    }

    /// Decode four wire frames, attaching the given session identity.
    pub fn decode_wire(wire: &[Vec<u8>], session_id: Vec<u8>) -> Result<ChannelFrame, ChannelError> {
        if wire.len() != 4 {
            return Err(ChannelError::Malformed(format!(
                "expected 4 frames, got {}",
                wire.len()
            )));
        }
        let channel = String::from_utf8_lossy(&wire[0]).into_owned();
        let sequence = if wire[1].len() == 8 {
            let mut b = [0u8; 8];
            b.copy_from_slice(&wire[1]);
            u64::from_le_bytes(b)
        } else {
            return Err(ChannelError::Malformed(format!(
                "sequence frame must be 8 bytes, got {}",
                wire[1].len()
            )));
        };
        let kind = if wire[2].len() == 1 {
            ChannelKind::from_byte(wire[2][0])
                .ok_or_else(|| ChannelError::Malformed(format!("unknown kind byte {}", wire[2][0])))?
        } else {
            return Err(ChannelError::Malformed(format!(
                "kind frame must be 1 byte, got {}",
                wire[2].len()
            )));
        };
        Ok(ChannelFrame {
            channel,
            session_id,
            sequence,
            kind,
            payload: wire[3].clone(),
        })
    }
}

// ---------------------------------------------------------------------------
// Errors
// ---------------------------------------------------------------------------

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ChannelError {
    UnknownChannel(String),
    Overflow(String),
    Malformed(String),
    Disconnected,
    Timeout,
    Transport(String),
}

impl std::fmt::Display for ChannelError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ChannelError::UnknownChannel(c) => write!(f, "unknown channel '{}'", c),
            ChannelError::Overflow(c) => write!(f, "queue overflow on channel '{}'", c),
            ChannelError::Malformed(m) => write!(f, "malformed frame: {}", m),
            ChannelError::Disconnected => write!(f, "session disconnected"),
            ChannelError::Timeout => write!(f, "transport timeout"),
            ChannelError::Transport(m) => write!(f, "transport error: {}", m),
        }
    }
}

impl std::error::Error for ChannelError {}

// ---------------------------------------------------------------------------
// Transport abstraction
// ---------------------------------------------------------------------------

/// A message as seen by the transport: the peer identity plus the wire frames.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TransportMessage {
    pub session_id: Vec<u8>,
    pub frames: Vec<Vec<u8>>,
}

/// Backend-independent byte transport.  Implementations must never panic or
/// block unboundedly; `recv` returns `Ok(None)` on timeout.
pub trait ChannelTransport: Send {
    fn recv(&mut self, timeout_ms: u64) -> Result<Option<TransportMessage>, ChannelError>;
    fn send(&mut self, msg: TransportMessage) -> Result<(), ChannelError>;
    fn endpoint(&self) -> Option<String> {
        None
    }
}

// ---------------------------------------------------------------------------
// In-memory transport (tests / no-network)
// ---------------------------------------------------------------------------

/// A synchronous, cross-wired in-memory transport.  `MemoryTransport::pair()`
/// returns two ends: whatever side A sends appears in side B's `recv`, and
/// vice-versa.  Each message carries an identity chosen by the sender, exactly
/// like a ROUTER peer identity.
pub struct MemoryTransport {
    name: String,
    rx: mpsc::Receiver<TransportMessage>,
    tx: mpsc::Sender<TransportMessage>,
    identity: Vec<u8>,
}

impl MemoryTransport {
    pub fn pair(name: &str) -> (MemoryTransport, MemoryTransport) {
        let (tx_a, rx_a) = mpsc::channel::<TransportMessage>();
        let (tx_b, rx_b) = mpsc::channel::<TransportMessage>();
        let a = MemoryTransport {
            name: format!("{name}-a"),
            rx: rx_a,
            tx: tx_b,
            identity: b"client".to_vec(),
        };
        let b = MemoryTransport {
            name: format!("{name}-b"),
            rx: rx_b,
            tx: tx_a,
            identity: b"broker".to_vec(),
        };
        (a, b)
    }

    pub fn identity(&self) -> Vec<u8> {
        self.identity.clone()
    }
}

impl ChannelTransport for MemoryTransport {
    fn recv(&mut self, timeout_ms: u64) -> Result<Option<TransportMessage>, ChannelError> {
        let deadline = std::time::Instant::now() + std::time::Duration::from_millis(timeout_ms);
        loop {
            match self.rx.try_recv() {
                Ok(msg) => return Ok(Some(msg)),
                Err(mpsc::TryRecvError::Empty) => {
                    if std::time::Instant::now() >= deadline {
                        return Ok(None);
                    }
                    std::thread::sleep(std::time::Duration::from_millis(1));
                }
                Err(mpsc::TryRecvError::Disconnected) => {
                    return Err(ChannelError::Disconnected);
                }
            }
        }
    }

    fn send(&mut self, msg: TransportMessage) -> Result<(), ChannelError> {
        self.tx.send(msg).map_err(|_| ChannelError::Disconnected)
    }

    fn endpoint(&self) -> Option<String> {
        Some(self.name.clone())
    }
}

// ---------------------------------------------------------------------------
// ZeroMQ transport (ROUTER, pure-Rust `zeromq` crate)
// ---------------------------------------------------------------------------

/// Handle for the ZeroMQ ROUTER transport.  The socket is owned by a dedicated
/// tokio task; inbound/outbound messages are bridged through bounded sync
/// channels so the emulator thread never touches ZeroMQ directly.
pub struct ZmqTransport {
    inbound_rx: mpsc::Receiver<TransportMessage>,
    outbound_tx: mpsc::Sender<TransportMessage>,
    bound_endpoint: Option<String>,
    // Keep the pump task alive for the lifetime of the handle.
    _task: std::thread::JoinHandle<()>,
}

impl ZmqTransport {
    /// Bind a ROUTER socket (libzmq via the `zmq` crate) and start the pump
    /// task.  The pump owns the socket; inbound/outbound messages are bridged
    /// through sync channels.
    pub fn bind(endpoint: &str) -> Result<ZmqTransport, ChannelError> {
        let (inbound_tx, inbound_rx) = mpsc::channel::<TransportMessage>();
        let (outbound_tx, outbound_rx) = mpsc::channel::<TransportMessage>();
        let endpoint_for_task = endpoint.to_string();
        let bound_endpoint = endpoint.to_string();

        let task = std::thread::spawn(move || {
            let ctx = zmq::Context::new();
            let router = match ctx.socket(zmq::ROUTER) {
                Ok(s) => s,
                Err(e) => {
                    eprintln!("[channel::ZmqTransport] ROUTER socket failed: {e}");
                    return;
                }
            };
            if let Err(e) = router.set_rcvtimeo(20) {
                eprintln!("[channel::ZmqTransport] set_rcvtimeo failed: {e}");
            }
            if let Err(e) = router.set_linger(0) {
                eprintln!("[channel::ZmqTransport] set_linger failed: {e}");
            }
            if let Err(e) = router.bind(&endpoint_for_task) {
                eprintln!("[channel::ZmqTransport] bind failed for {endpoint_for_task}: {e}");
                return;
            }
            eprintln!("[channel::ZmqTransport] ROUTER bound on {endpoint_for_task}");

            loop {
                // Drain queued outbound first (non-blocking), then poll inbound.
                while let Ok(msg) = outbound_rx.try_recv() {
                    let mut parts = vec![msg.session_id.clone()];
                    parts.extend(msg.frames.iter().cloned());
                    if let Err(e) = router.send_multipart(parts.as_slice(), 0) {
                        eprintln!("[channel::ZmqTransport] send failed: {e}");
                    }
                }

                match router.recv_multipart(0) {
                    Ok(parts) => {
                        if parts.is_empty() {
                            continue;
                        }
                        let mut parts = parts;
                        let session_id = parts.remove(0);
                        if inbound_tx
                            .send(TransportMessage {
                                session_id,
                                frames: parts,
                            })
                            .is_err()
                        {
                            // Broker gone; stop the pump.
                            break;
                        }
                    }
                    Err(zmq::Error::EAGAIN) => { /* poll timeout; loop */ }
                    Err(e) => {
                        eprintln!("[channel::ZmqTransport] recv failed: {e}");
                        std::thread::sleep(std::time::Duration::from_millis(10));
                    }
                }
            }
        });

        Ok(ZmqTransport {
            inbound_rx,
            outbound_tx,
            bound_endpoint: Some(bound_endpoint),
            _task: task,
        })
    }
}

impl ChannelTransport for ZmqTransport {
    fn recv(&mut self, timeout_ms: u64) -> Result<Option<TransportMessage>, ChannelError> {
        match self
            .inbound_rx
            .recv_timeout(std::time::Duration::from_millis(timeout_ms))
        {
            Ok(msg) => Ok(Some(msg)),
            Err(mpsc::RecvTimeoutError::Timeout) => Ok(None),
            Err(mpsc::RecvTimeoutError::Disconnected) => Err(ChannelError::Disconnected),
        }
    }

    fn send(&mut self, msg: TransportMessage) -> Result<(), ChannelError> {
        self.outbound_tx
            .send(msg)
            .map_err(|_| ChannelError::Disconnected)
    }

    fn endpoint(&self) -> Option<String> {
        self.bound_endpoint.clone()
    }
}

// ---------------------------------------------------------------------------
// Broker
// ---------------------------------------------------------------------------

#[derive(Default)]
struct SessionState {
    last_sequence: Option<u64>,
    outbound: VecDeque<TransportMessage>,
}

impl std::fmt::Debug for SessionState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SessionState")
            .field("last_sequence", &self.last_sequence)
            .field("outbound_len", &self.outbound.len())
            .finish()
    }
}

/// Validates and routes channel frames.  The broker owns no sockets; a caller
/// pumps transport messages in and drains outbound messages out.
pub struct ChannelBroker {
    channels: HashMap<String, ChannelConfig>,
    sessions: HashMap<Vec<u8>, SessionState>,
    /// Validated frames queued for the plugin (host -> guest).
    inbound_for_plugin: VecDeque<ChannelFrame>,
}

impl std::fmt::Debug for ChannelBroker {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ChannelBroker")
            .field("channels", &self.channels)
            .field("sessions", &self.sessions.len())
            .field("inbound_for_plugin", &self.inbound_for_plugin.len())
            .finish()
    }
}

impl ChannelBroker {
    pub fn new(configs: Vec<ChannelConfig>) -> ChannelBroker {
        let channels = configs
            .into_iter()
            .map(|c| (c.name.clone(), c))
            .collect::<HashMap<_, _>>();
        ChannelBroker {
            channels,
            sessions: HashMap::new(),
            inbound_for_plugin: VecDeque::new(),
        }
    }

    pub fn default_fastboot() -> ChannelBroker {
        ChannelBroker::new(vec![ChannelConfig::fastboot()])
    }

    fn error_reply(
        &self,
        session_id: Vec<u8>,
        channel: &str,
        sequence: u64,
        reason: &str,
    ) -> TransportMessage {
        let frame = ChannelFrame {
            channel: channel.to_string(),
            session_id: session_id.clone(),
            sequence,
            kind: ChannelKind::Error,
            payload: reason.as_bytes().to_vec(),
        };
        TransportMessage {
            session_id,
            frames: frame.encode_wire(),
        }
    }

    /// Process one inbound transport message.  Returns the outbound messages
    /// that must be handed back to the transport (error replies and/or echo
    /// acknowledgements for control frames).
    pub fn process_inbound(&mut self, msg: TransportMessage) -> Vec<TransportMessage> {
        let session_id = msg.session_id.clone();
        let frame = match ChannelFrame::decode_wire(&msg.frames, session_id.clone()) {
            Ok(f) => f,
            Err(e) => {
                return vec![TransportMessage {
                    session_id,
                    frames: vec![
                        b"".to_vec(),
                        0u64.to_le_bytes().to_vec(),
                        vec![ChannelKind::Error.as_byte()],
                        e.to_string().into_bytes(),
                    ],
                }];
            }
        };

        // Unknown channel.
        let cfg = match self.channels.get(&frame.channel) {
            Some(c) => c.clone(),
            None => {
                return vec![self.error_reply(
                    session_id,
                    &frame.channel,
                    frame.sequence,
                    "unknown channel",
                )];
            }
        };

        // Payload size.
        if frame.payload.len() > cfg.max_frame_bytes {
            return vec![self.error_reply(
                session_id,
                &frame.channel,
                frame.sequence,
                "frame exceeds max_frame_bytes",
            )];
        }

        // Per-session monotonic sequence.
        let session = self.sessions.entry(session_id.clone()).or_default();
        let seq_ok = match session.last_sequence {
            None => true, // first frame sets the baseline
            Some(last) => frame.sequence > last,
        };
        if !seq_ok {
            return vec![self.error_reply(
                session_id,
                &frame.channel,
                frame.sequence,
                "non-monotonic sequence",
            )];
        }
        session.last_sequence = Some(frame.sequence);

        // Bounded inbound queue for the plugin.
        if self.inbound_for_plugin.len() >= cfg.queue_depth {
            return vec![self.error_reply(
                session_id,
                &frame.channel,
                frame.sequence,
                "queue overflow",
            )];
        }

        // Control frames are echoed back as Status so clients can prove the
        // binary round-trip; bulk frames are queued for the plugin.
        let mut out = Vec::new();
        if frame.kind == ChannelKind::Control {
            let reply = ChannelFrame {
                channel: frame.channel.clone(),
                session_id: session_id.clone(),
                sequence: frame.sequence,
                kind: ChannelKind::Status,
                payload: frame.payload.clone(),
            };
            out.push(TransportMessage {
                session_id,
                frames: reply.encode_wire(),
            });
        } else {
            self.inbound_for_plugin.push_back(frame);
        }
        out
    }

    /// Next validated frame for the plugin, if any.
    pub fn recv_for_plugin(&mut self) -> Option<ChannelFrame> {
        self.inbound_for_plugin.pop_front()
    }

    /// Queue a frame for delivery to a session (plugin -> host).  Bounded by
    /// the channel's `queue_depth`.
    pub fn send_to_session(&mut self, frame: ChannelFrame) -> Result<(), ChannelError> {
        let cfg = self
            .channels
            .get(&frame.channel)
            .ok_or_else(|| ChannelError::UnknownChannel(frame.channel.clone()))?;
        let session = self.sessions.entry(frame.session_id.clone()).or_default();
        if session.outbound.len() >= cfg.queue_depth {
            return Err(ChannelError::Overflow(frame.channel.clone()));
        }
        let sid = frame.session_id.clone();
        session.outbound.push_back(TransportMessage {
            session_id: sid,
            frames: frame.encode_wire(),
        });
        Ok(())
    }

    /// Drain queued outbound messages to hand to the transport.
    pub fn drain_outbound(&mut self) -> Vec<TransportMessage> {
        let mut out = Vec::new();
        for session in self.sessions.values_mut() {
            while let Some(msg) = session.outbound.pop_front() {
                out.push(msg);
            }
        }
        out
    }

    /// Drop all state for a disconnected session.
    pub fn drop_session(&mut self, session_id: &[u8]) {
        self.sessions.remove(session_id);
    }

    pub fn channel_names(&self) -> Vec<String> {
        let mut names: Vec<String> = self.channels.keys().cloned().collect();
        names.sort();
        names
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_payload() -> Vec<u8> {
        // Binary payload with NUL bytes, high bytes, and arbitrary data.
        let mut p = vec![0u8, 1, 2, 3, 0, 0xff, 0xfe, b'A', b'B', 0, 0x80, 0x7f];
        p.extend_from_slice(b"getvar:version");
        p
    }

    #[test]
    fn wire_round_trip_preserves_binary() {
        let payload = sample_payload();
        let frame = ChannelFrame {
            channel: "fastboot".into(),
            session_id: b"dealer-1".to_vec(),
            sequence: 7,
            kind: ChannelKind::BulkOut,
            payload: payload.clone(),
        };
        let wire = frame.encode_wire();
        let decoded = ChannelFrame::decode_wire(&wire, b"dealer-1".to_vec()).unwrap();
        assert_eq!(decoded, frame);
        assert_eq!(decoded.payload, payload);
        assert!(payload.contains(&0));
    }

    #[test]
    fn broker_rejects_unknown_channel() {
        let mut broker = ChannelBroker::default_fastboot();
        let frame = ChannelFrame {
            channel: "nope".into(),
            session_id: b"s".to_vec(),
            sequence: 0,
            kind: ChannelKind::BulkOut,
            payload: vec![1, 2, 3],
        };
        let out = broker.process_inbound(TransportMessage {
            session_id: b"s".to_vec(),
            frames: frame.encode_wire(),
        });
        assert_eq!(out.len(), 1);
        let reply = ChannelFrame::decode_wire(&out[0].frames, b"s".to_vec()).unwrap();
        assert_eq!(reply.kind, ChannelKind::Error);
        assert!(String::from_utf8_lossy(&reply.payload).contains("unknown channel"));
    }

    #[test]
    fn broker_rejects_non_monotonic_sequence() {
        let mut broker = ChannelBroker::default_fastboot();
        let mk = |seq: u64| TransportMessage {
            session_id: b"s".to_vec(),
            frames: ChannelFrame {
                channel: "fastboot".into(),
                session_id: b"s".to_vec(),
                sequence: seq,
                kind: ChannelKind::BulkOut,
                payload: vec![1],
            }
            .encode_wire(),
        };
        assert!(broker.process_inbound(mk(5)).is_empty());
        let out = broker.process_inbound(mk(5));
        assert_eq!(out.len(), 1);
        let reply = ChannelFrame::decode_wire(&out[0].frames, b"s".to_vec()).unwrap();
        assert_eq!(reply.kind, ChannelKind::Error);
        // 6 > 5 is accepted.
        assert!(broker.process_inbound(mk(6)).is_empty());
        // Two accepted frames (seq 5 and seq 6) queued for the plugin.
        assert!(broker.recv_for_plugin().is_some());
        assert!(broker.recv_for_plugin().is_some());
        assert!(broker.recv_for_plugin().is_none());
    }

    #[test]
    fn broker_control_frame_echoes_binary() {
        let mut broker = ChannelBroker::default_fastboot();
        let payload = sample_payload();
        let frame = ChannelFrame {
            channel: "fastboot".into(),
            session_id: b"dealer-42".to_vec(),
            sequence: 1,
            kind: ChannelKind::Control,
            payload: payload.clone(),
        };
        let out = broker.process_inbound(TransportMessage {
            session_id: b"dealer-42".to_vec(),
            frames: frame.encode_wire(),
        });
        assert_eq!(out.len(), 1);
        let reply = ChannelFrame::decode_wire(&out[0].frames, b"dealer-42".to_vec()).unwrap();
        assert_eq!(reply.kind, ChannelKind::Status);
        assert_eq!(reply.payload, payload);
    }

    #[test]
    fn broker_plugin_round_trip_through_memory_transport() {
        let (mut client, mut broker_side) = MemoryTransport::pair("fastboot-test");
        let mut broker = ChannelBroker::default_fastboot();

        // Client -> broker.
        let payload = sample_payload();
        let frame = ChannelFrame {
            channel: "fastboot".into(),
            session_id: client.identity(),
            sequence: 1,
            kind: ChannelKind::BulkOut,
            payload: payload.clone(),
        };
        client
            .send(TransportMessage {
                session_id: client.identity(),
                frames: frame.encode_wire(),
            })
            .unwrap();

        let inbound = broker_side.recv(100).unwrap().unwrap();
        assert!(broker.process_inbound(inbound).is_empty());
        let got = broker.recv_for_plugin().unwrap();
        assert_eq!(got.payload, payload);

        // Plugin -> client (bulk_in response).
        broker
            .send_to_session(ChannelFrame {
                channel: "fastboot".into(),
                session_id: client.identity(),
                sequence: 2,
                kind: ChannelKind::BulkIn,
                payload: b"OKAYversion".to_vec(),
            })
            .unwrap();
        let outbound = broker.drain_outbound();
        assert_eq!(outbound.len(), 1);
        broker_side.send(outbound[0].clone()).unwrap();

        let back = client.recv(100).unwrap().unwrap();
        assert_eq!(back.session_id, client.identity());
        let reply = ChannelFrame::decode_wire(&back.frames, client.identity()).unwrap();
        assert_eq!(reply.kind, ChannelKind::BulkIn);
        assert_eq!(reply.payload, b"OKAYversion".to_vec());
    }

    #[test]
    fn dealer_router_binary_round_trip_over_ipc() {
        use zmq::{Context, DEALER, ROUTER};

        // Unique IPC path so the test can run concurrently.
        let path = format!("ipc:///tmp/qecu-channel-test-{}.sock", std::process::id());
        let _ = std::fs::remove_file(&path[6..]);

        let ctx = Context::new();
        // Bind the broker ROUTER.
        let router = ctx.socket(ROUTER).unwrap();
        router.set_linger(0).unwrap();
        router.bind(&path).unwrap();

        // Client DEALER.
        let dealer = ctx.socket(DEALER).unwrap();
        dealer.set_linger(0).unwrap();
        dealer.set_identity(b"dealer-test".as_slice()).unwrap();
        dealer.connect(&path).unwrap();

        // Send a Control frame with binary/NUL payload.
        let payload = sample_payload();
        let wire = ChannelFrame {
            channel: "fastboot".into(),
            session_id: Vec::new(), // identity is managed by ZMQ, not the wire
            sequence: 1,
            kind: ChannelKind::Control,
            payload: payload.clone(),
        }
        .encode_wire();

        dealer.send_multipart(&wire, 0).unwrap();

        // ROUTER receives [identity, channel, seq, kind, payload].
        let recv = router.recv_multipart(0).unwrap();
        let mut frames = recv;
        assert_eq!(frames.len(), 5, "ROUTER must prepend identity");
        let identity = frames.remove(0);
        assert_eq!(frames, wire);

        // Broker processes and produces a Status echo.
        let mut broker = ChannelBroker::default_fastboot();
        let replies = broker.process_inbound(TransportMessage {
            session_id: identity.clone(),
            frames,
        });
        assert_eq!(replies.len(), 1);

        // ROUTER sends [identity, channel, seq, status, payload] back.
        let mut back = vec![identity];
        back.extend(replies[0].frames.iter().cloned());
        router.send_multipart(&back, 0).unwrap();

        // DEALER receives the four wire frames (no identity).
        let got = dealer.recv_multipart(0).unwrap();
        assert_eq!(got.len(), 4);
        let reply = ChannelFrame::decode_wire(&got, Vec::new()).unwrap();
        assert_eq!(reply.kind, ChannelKind::Status);
        assert_eq!(reply.payload, payload, "NUL/binary bytes must round-trip");

        let _ = std::fs::remove_file(&path[6..]);
    }
}
