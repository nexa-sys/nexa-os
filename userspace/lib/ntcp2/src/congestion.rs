//! QUIC Congestion Control
//!
//! This module implements congestion control algorithms for QUIC
//! according to RFC 9002.
//!
//! ## Supported Algorithms
//!
//! - **New Reno**: Basic AIMD congestion control
//! - **Cubic**: Modern TCP-friendly congestion control
//! - **BBR**: Bottleneck Bandwidth and RTT (experimental)
//! - **BBRv2**: Improved BBR with loss awareness

use crate::error::Result;
use crate::Duration;

use std::sync::atomic::{AtomicU64, Ordering};

// ============================================================================
// Constants
// ============================================================================

/// Initial congestion window in bytes (RFC 9002)
pub const INITIAL_WINDOW_PACKETS: u64 = 10;

/// Minimum congestion window in bytes
pub const MINIMUM_WINDOW_PACKETS: u64 = 2;

/// Default maximum datagram size
pub const MAX_DATAGRAM_SIZE: u64 = 1200;

/// Initial congestion window
pub const INITIAL_WINDOW: u64 = INITIAL_WINDOW_PACKETS * MAX_DATAGRAM_SIZE;

/// Minimum congestion window
pub const MINIMUM_WINDOW: u64 = MINIMUM_WINDOW_PACKETS * MAX_DATAGRAM_SIZE;

/// Loss reduction factor (multiplicative decrease)
pub const LOSS_REDUCTION_FACTOR: f64 = 0.5;

/// Persistent congestion threshold (in PTO periods)
pub const PERSISTENT_CONGESTION_THRESHOLD: u64 = 3;

/// Cubic beta (multiplicative decrease factor)
pub const CUBIC_BETA: f64 = 0.7;

/// Cubic C constant
pub const CUBIC_C: f64 = 0.4;

/// BBR startup growth rate
pub const BBR_STARTUP_GROWTH_RATE: f64 = 2.0 / 3.0 * 2.89;

/// BBR pacing gain in startup
pub const BBR_STARTUP_PACING_GAIN: f64 = 2.89;

/// BBR probe bandwidth pacing gain
pub const BBR_PROBE_BW_GAIN: f64 = 1.25;

// ============================================================================
// Congestion Control Algorithm
// ============================================================================

/// Congestion control algorithm selector
#[repr(u8)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CongestionAlgorithm {
    /// New Reno (RFC 6582)
    NewReno = 0,
    /// Cubic (RFC 8312)
    Cubic = 1,
    /// BBR (Google)
    Bbr = 2,
    /// BBRv2
    Bbr2 = 3,
}

impl Default for CongestionAlgorithm {
    fn default() -> Self {
        CongestionAlgorithm::Cubic
    }
}

// ============================================================================
// Congestion Controller Trait
// ============================================================================

/// Congestion controller interface
pub trait CongestionController: Send + Sync {
    /// Get current congestion window
    fn cwnd(&self) -> u64;

    /// Get bytes in flight
    fn bytes_in_flight(&self) -> u64;

    /// Get slow start threshold
    fn ssthresh(&self) -> u64;

    /// Check if in slow start
    fn in_slow_start(&self) -> bool;

    /// Check if in recovery
    fn in_recovery(&self) -> bool;

    /// Called when packet is sent
    fn on_packet_sent(&self, sent_bytes: u64, now: Duration);

    /// Called when packet is acknowledged
    fn on_packet_acked(&self, acked_bytes: u64, rtt: Duration, now: Duration);

    /// Called when packet is lost
    fn on_packet_lost(&self, lost_bytes: u64, now: Duration);

    /// Called on congestion event (ECN-CE or loss)
    fn on_congestion_event(&self, sent_time: Duration, now: Duration);

    /// Called when ECN-CE is received
    fn on_ecn_ce(&self, now: Duration);

    /// Reset congestion state
    fn reset(&self);

    /// Get pacing rate (bytes per second)
    fn pacing_rate(&self) -> u64;

    /// Update RTT estimate
    fn update_rtt(&self, rtt: Duration, min_rtt: Duration);

    /// Get algorithm name
    fn algorithm(&self) -> CongestionAlgorithm;
}

// ============================================================================
// New Reno Implementation
// ============================================================================

/// New Reno congestion controller
pub struct NewReno {
    /// Congestion window
    cwnd: AtomicU64,
    /// Slow start threshold
    ssthresh: AtomicU64,
    /// Bytes in flight
    bytes_in_flight: AtomicU64,
    /// Recovery start time
    recovery_start: AtomicU64,
    /// Maximum datagram size
    max_datagram_size: u64,
    /// RTT estimate
    smoothed_rtt: AtomicU64,
}

impl NewReno {
    /// Create a new New Reno controller
    pub fn new(max_datagram_size: u64) -> Self {
        Self {
            cwnd: AtomicU64::new(INITIAL_WINDOW),
            ssthresh: AtomicU64::new(u64::MAX),
            bytes_in_flight: AtomicU64::new(0),
            recovery_start: AtomicU64::new(0),
            max_datagram_size,
            smoothed_rtt: AtomicU64::new(0),
        }
    }
}

impl CongestionController for NewReno {
    fn cwnd(&self) -> u64 {
        self.cwnd.load(Ordering::Acquire)
    }

    fn bytes_in_flight(&self) -> u64 {
        self.bytes_in_flight.load(Ordering::Acquire)
    }

    fn ssthresh(&self) -> u64 {
        self.ssthresh.load(Ordering::Acquire)
    }

    fn in_slow_start(&self) -> bool {
        self.cwnd() < self.ssthresh()
    }

    fn in_recovery(&self) -> bool {
        self.recovery_start.load(Ordering::Acquire) > 0
    }

    fn on_packet_sent(&self, sent_bytes: u64, _now: Duration) {
        self.bytes_in_flight.fetch_add(sent_bytes, Ordering::AcqRel);
    }

    fn on_packet_acked(&self, acked_bytes: u64, _rtt: Duration, now: Duration) {
        // Remove from bytes in flight
        self.bytes_in_flight.fetch_sub(acked_bytes, Ordering::AcqRel);

        // Exit recovery if needed
        let recovery_start = self.recovery_start.load(Ordering::Acquire);
        if recovery_start > 0 && now > recovery_start {
            self.recovery_start.store(0, Ordering::Release);
        }

        // Don't increase cwnd during recovery
        if self.in_recovery() {
            return;
        }

        let cwnd = self.cwnd.load(Ordering::Acquire);

        if self.in_slow_start() {
            // Slow start: increase by acked_bytes
            let new_cwnd = cwnd + acked_bytes;
            self.cwnd.store(new_cwnd, Ordering::Release);
        } else {
            // Congestion avoidance: increase by MSS^2 / cwnd
            let mss = self.max_datagram_size;
            let increment = (mss * mss) / cwnd;
            let new_cwnd = cwnd + increment.max(1);
            self.cwnd.store(new_cwnd, Ordering::Release);
        }
    }

    fn on_packet_lost(&self, lost_bytes: u64, now: Duration) {
        self.bytes_in_flight.fetch_sub(lost_bytes, Ordering::AcqRel);
        self.on_congestion_event(now, now);
    }

    fn on_congestion_event(&self, _sent_time: Duration, now: Duration) {
        // Only one congestion response per RTT
        if self.in_recovery() {
            return;
        }

        // Enter recovery
        self.recovery_start.store(now, Ordering::Release);

        // Reduce cwnd
        let cwnd = self.cwnd.load(Ordering::Acquire);
        let new_cwnd = (cwnd as f64 * LOSS_REDUCTION_FACTOR) as u64;
        let new_cwnd = new_cwnd.max(MINIMUM_WINDOW);

        self.cwnd.store(new_cwnd, Ordering::Release);
        self.ssthresh.store(new_cwnd, Ordering::Release);
    }

    fn on_ecn_ce(&self, now: Duration) {
        self.on_congestion_event(now, now);
    }

    fn reset(&self) {
        self.cwnd.store(INITIAL_WINDOW, Ordering::Release);
        self.ssthresh.store(u64::MAX, Ordering::Release);
        self.bytes_in_flight.store(0, Ordering::Release);
        self.recovery_start.store(0, Ordering::Release);
    }

    fn pacing_rate(&self) -> u64 {
        let cwnd = self.cwnd();
        let rtt = self.smoothed_rtt.load(Ordering::Acquire);

        if rtt == 0 {
            return u64::MAX;
        }

        // Rate = cwnd / RTT (in bytes per nanosecond, then convert to per second)
        (cwnd as u128 * 1_000_000_000 / rtt as u128) as u64
    }

    fn update_rtt(&self, rtt: Duration, _min_rtt: Duration) {
        self.smoothed_rtt.store(rtt, Ordering::Release);
    }

    fn algorithm(&self) -> CongestionAlgorithm {
        CongestionAlgorithm::NewReno
    }
}

// ============================================================================
// Cubic Implementation
// ============================================================================

/// Cubic congestion controller (RFC 8312)
pub struct Cubic {
    /// Congestion window
    cwnd: AtomicU64,
    /// Slow start threshold
    ssthresh: AtomicU64,
    /// Bytes in flight
    bytes_in_flight: AtomicU64,
    /// Recovery start time
    recovery_start: AtomicU64,
    /// Time of last congestion event
    last_congestion_time: AtomicU64,
    /// Cwnd at last congestion event
    cwnd_prior: AtomicU64,
    /// Maximum datagram size
    max_datagram_size: u64,
    /// RTT estimate
    smoothed_rtt: AtomicU64,
    /// Minimum RTT
    min_rtt: AtomicU64,
    /// K value (time to reach W_max)
    k: AtomicU64, // Stored as fixed-point: K * 1000
    /// Origin point (W_max)
    origin_point: AtomicU64,
}

impl Cubic {
    /// Create a new Cubic controller
    pub fn new(max_datagram_size: u64) -> Self {
        Self {
            cwnd: AtomicU64::new(INITIAL_WINDOW),
            ssthresh: AtomicU64::new(u64::MAX),
            bytes_in_flight: AtomicU64::new(0),
            recovery_start: AtomicU64::new(0),
            last_congestion_time: AtomicU64::new(0),
            cwnd_prior: AtomicU64::new(0),
            max_datagram_size,
            smoothed_rtt: AtomicU64::new(0),
            min_rtt: AtomicU64::new(u64::MAX),
            k: AtomicU64::new(0),
            origin_point: AtomicU64::new(0),
        }
    }

    /// Calculate Cubic window
    fn cubic_window(&self, t: f64) -> u64 {
        let k = self.k.load(Ordering::Acquire) as f64 / 1000.0;
        let origin = self.origin_point.load(Ordering::Acquire) as f64;

        let delta_t = t - k;
        let w_cubic = CUBIC_C * delta_t.powi(3) + origin;

        w_cubic.max(MINIMUM_WINDOW as f64) as u64
    }
}

impl CongestionController for Cubic {
    fn cwnd(&self) -> u64 {
        self.cwnd.load(Ordering::Acquire)
    }

    fn bytes_in_flight(&self) -> u64 {
        self.bytes_in_flight.load(Ordering::Acquire)
    }

    fn ssthresh(&self) -> u64 {
        self.ssthresh.load(Ordering::Acquire)
    }

    fn in_slow_start(&self) -> bool {
        self.cwnd() < self.ssthresh()
    }

    fn in_recovery(&self) -> bool {
        self.recovery_start.load(Ordering::Acquire) > 0
    }

    fn on_packet_sent(&self, sent_bytes: u64, _now: Duration) {
        self.bytes_in_flight.fetch_add(sent_bytes, Ordering::AcqRel);
    }

    fn on_packet_acked(&self, acked_bytes: u64, _rtt: Duration, now: Duration) {
        self.bytes_in_flight.fetch_sub(acked_bytes, Ordering::AcqRel);

        // Exit recovery if needed
        let recovery_start = self.recovery_start.load(Ordering::Acquire);
        if recovery_start > 0 && now > recovery_start {
            self.recovery_start.store(0, Ordering::Release);
        }

        // Don't increase cwnd during recovery
        if self.in_recovery() {
            return;
        }

        let cwnd = self.cwnd.load(Ordering::Acquire);

        if self.in_slow_start() {
            // Slow start: exponential increase
            let new_cwnd = cwnd + acked_bytes;
            self.cwnd.store(new_cwnd, Ordering::Release);
        } else {
            // Cubic congestion avoidance
            let last_congestion = self.last_congestion_time.load(Ordering::Acquire);
            if last_congestion == 0 {
                // No congestion yet, use standard increase
                let mss = self.max_datagram_size;
                let increment = mss / cwnd.max(1);
                let new_cwnd = cwnd + increment.max(1);
                self.cwnd.store(new_cwnd, Ordering::Release);
            } else {
                // Time since congestion (in seconds)
                let t = (now - last_congestion) as f64 / 1_000_000_000.0;

                // Calculate Cubic target
                let w_cubic = self.cubic_window(t);

                // Calculate standard TCP window (for hybrid mode)
                let min_rtt = self.min_rtt.load(Ordering::Acquire);
                let rtt_secs = min_rtt as f64 / 1_000_000_000.0;
                let w_est = cwnd as f64 + 3.0 * CUBIC_BETA / (2.0 - CUBIC_BETA) * rtt_secs * t / cwnd as f64;

                // Use max of Cubic and TCP-friendly
                let target = (w_cubic as f64).max(w_est);
                let target = target as u64;

                if target > cwnd {
                    // Increase by (target - cwnd) / cwnd per ACK
                    let increment = ((target - cwnd) as f64 * acked_bytes as f64 / cwnd as f64) as u64;
                    let new_cwnd = cwnd + increment.max(1);
                    self.cwnd.store(new_cwnd, Ordering::Release);
                }
            }
        }
    }

    fn on_packet_lost(&self, lost_bytes: u64, now: Duration) {
        self.bytes_in_flight.fetch_sub(lost_bytes, Ordering::AcqRel);
        self.on_congestion_event(now, now);
    }

    fn on_congestion_event(&self, _sent_time: Duration, now: Duration) {
        if self.in_recovery() {
            return;
        }

        self.recovery_start.store(now, Ordering::Release);

        let cwnd = self.cwnd.load(Ordering::Acquire);

        // Save prior cwnd (for Cubic calculation)
        self.cwnd_prior.store(cwnd, Ordering::Release);
        self.origin_point.store(cwnd, Ordering::Release);
        self.last_congestion_time.store(now, Ordering::Release);

        // Calculate K = cubic_root((W_max * (1 - beta)) / C)
        let w_max = cwnd as f64;
        let k = ((w_max * (1.0 - CUBIC_BETA)) / CUBIC_C).powf(1.0 / 3.0);
        self.k.store((k * 1000.0) as u64, Ordering::Release);

        // Reduce cwnd
        let new_cwnd = (cwnd as f64 * CUBIC_BETA) as u64;
        let new_cwnd = new_cwnd.max(MINIMUM_WINDOW);

        self.cwnd.store(new_cwnd, Ordering::Release);
        self.ssthresh.store(new_cwnd, Ordering::Release);
    }

    fn on_ecn_ce(&self, now: Duration) {
        self.on_congestion_event(now, now);
    }

    fn reset(&self) {
        self.cwnd.store(INITIAL_WINDOW, Ordering::Release);
        self.ssthresh.store(u64::MAX, Ordering::Release);
        self.bytes_in_flight.store(0, Ordering::Release);
        self.recovery_start.store(0, Ordering::Release);
        self.last_congestion_time.store(0, Ordering::Release);
        self.cwnd_prior.store(0, Ordering::Release);
        self.k.store(0, Ordering::Release);
        self.origin_point.store(0, Ordering::Release);
    }

    fn pacing_rate(&self) -> u64 {
        let cwnd = self.cwnd();
        let rtt = self.smoothed_rtt.load(Ordering::Acquire);

        if rtt == 0 {
            return u64::MAX;
        }

        (cwnd as u128 * 1_000_000_000 / rtt as u128) as u64
    }

    fn update_rtt(&self, rtt: Duration, min_rtt: Duration) {
        self.smoothed_rtt.store(rtt, Ordering::Release);
        self.min_rtt.fetch_min(min_rtt, Ordering::AcqRel);
    }

    fn algorithm(&self) -> CongestionAlgorithm {
        CongestionAlgorithm::Cubic
    }
}

// ============================================================================
// BBR Implementation
// ============================================================================

/// BBR state machine states
#[repr(u8)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BbrState {
    /// Startup: exponential growth to find bandwidth
    Startup = 0,
    /// Drain: reduce queue after startup
    Drain = 1,
    /// ProbeBW: steady-state bandwidth probing
    ProbeBW = 2,
    /// ProbeRTT: periodic RTT probing
    ProbeRTT = 3,
}

/// BBR congestion controller
pub struct Bbr {
    /// Congestion window
    cwnd: AtomicU64,
    /// Bytes in flight
    bytes_in_flight: AtomicU64,
    /// Maximum datagram size
    max_datagram_size: u64,
    /// Current state
    state: AtomicU64, // BbrState as u64
    /// Bottleneck bandwidth estimate (bytes per second)
    btl_bw: AtomicU64,
    /// Minimum RTT estimate
    min_rtt: AtomicU64,
    /// RTT sample time
    min_rtt_timestamp: AtomicU64,
    /// Pacing gain
    pacing_gain: AtomicU64, // Fixed point: gain * 1000
    /// CWND gain
    cwnd_gain: AtomicU64, // Fixed point: gain * 1000
    /// Round count
    round_count: AtomicU64,
    /// Full bandwidth reached
    full_bw_reached: AtomicU64, // bool as u64
    /// Full bandwidth count
    full_bw_count: AtomicU64,
    /// Prior inflight for ProbeBW
    prior_inflight: AtomicU64,
    /// ProbeRTT round done
    probe_rtt_round_done: AtomicU64,
}

impl Bbr {
    /// Create a new BBR controller
    pub fn new(max_datagram_size: u64) -> Self {
        Self {
            cwnd: AtomicU64::new(INITIAL_WINDOW),
            bytes_in_flight: AtomicU64::new(0),
            max_datagram_size,
            state: AtomicU64::new(BbrState::Startup as u64),
            btl_bw: AtomicU64::new(0),
            min_rtt: AtomicU64::new(u64::MAX),
            min_rtt_timestamp: AtomicU64::new(0),
            pacing_gain: AtomicU64::new((BBR_STARTUP_PACING_GAIN * 1000.0) as u64),
            cwnd_gain: AtomicU64::new(2000), // 2.0
            round_count: AtomicU64::new(0),
            full_bw_reached: AtomicU64::new(0),
            full_bw_count: AtomicU64::new(0),
            prior_inflight: AtomicU64::new(0),
            probe_rtt_round_done: AtomicU64::new(0),
        }
    }

    /// Get current state
    fn state(&self) -> BbrState {
        match self.state.load(Ordering::Acquire) {
            0 => BbrState::Startup,
            1 => BbrState::Drain,
            2 => BbrState::ProbeBW,
            3 => BbrState::ProbeRTT,
            _ => BbrState::Startup,
        }
    }

    /// Set state
    fn set_state(&self, state: BbrState) {
        self.state.store(state as u64, Ordering::Release);
    }

    /// Calculate target cwnd
    fn target_cwnd(&self) -> u64 {
        let btl_bw = self.btl_bw.load(Ordering::Acquire);
        let min_rtt = self.min_rtt.load(Ordering::Acquire);
        let cwnd_gain = self.cwnd_gain.load(Ordering::Acquire) as f64 / 1000.0;

        if min_rtt == u64::MAX || btl_bw == 0 {
            return INITIAL_WINDOW;
        }

        // BDP = bandwidth * RTT
        let bdp = (btl_bw as u128 * min_rtt as u128 / 1_000_000_000) as u64;

        // Target = gain * BDP
        let target = (bdp as f64 * cwnd_gain) as u64;

        target.max(MINIMUM_WINDOW)
    }

    /// Update bandwidth estimate
    fn update_bandwidth(&self, delivered_bytes: u64, interval: Duration) {
        if interval == 0 {
            return;
        }

        // Calculate instantaneous bandwidth
        let bw = (delivered_bytes as u128 * 1_000_000_000 / interval as u128) as u64;

        // Update maximum bandwidth (windowed max)
        self.btl_bw.fetch_max(bw, Ordering::AcqRel);
    }

    /// Check for full bandwidth reached (startup exit condition)
    fn check_full_bw_reached(&self) {
        if self.full_bw_reached.load(Ordering::Acquire) != 0 {
            return;
        }

        // Check if bandwidth growth has stopped
        // Simplified: after 3 rounds without 25% growth
        let count = self.full_bw_count.fetch_add(1, Ordering::AcqRel);
        if count >= 3 {
            self.full_bw_reached.store(1, Ordering::Release);
        }
    }
}

impl CongestionController for Bbr {
    fn cwnd(&self) -> u64 {
        self.cwnd.load(Ordering::Acquire)
    }

    fn bytes_in_flight(&self) -> u64 {
        self.bytes_in_flight.load(Ordering::Acquire)
    }

    fn ssthresh(&self) -> u64 {
        u64::MAX // BBR doesn't use ssthresh
    }

    fn in_slow_start(&self) -> bool {
        self.state() == BbrState::Startup
    }

    fn in_recovery(&self) -> bool {
        false // BBR handles loss differently
    }

    fn on_packet_sent(&self, sent_bytes: u64, _now: Duration) {
        self.bytes_in_flight.fetch_add(sent_bytes, Ordering::AcqRel);
    }

    fn on_packet_acked(&self, acked_bytes: u64, rtt: Duration, now: Duration) {
        self.bytes_in_flight.fetch_sub(acked_bytes, Ordering::AcqRel);

        // Update bandwidth estimate
        self.update_bandwidth(acked_bytes, rtt);

        // Update min RTT
        let min_rtt = self.min_rtt.fetch_min(rtt, Ordering::AcqRel);
        if rtt <= min_rtt {
            self.min_rtt_timestamp.store(now, Ordering::Release);
        }

        // State machine transitions
        match self.state() {
            BbrState::Startup => {
                self.check_full_bw_reached();
                if self.full_bw_reached.load(Ordering::Acquire) != 0 {
                    // Transition to Drain
                    self.set_state(BbrState::Drain);
                    self.pacing_gain.store(500, Ordering::Release); // 0.5
                }
            }
            BbrState::Drain => {
                // Drain until bytes_in_flight <= BDP
                let target = self.target_cwnd();
                if self.bytes_in_flight() <= target {
                    self.set_state(BbrState::ProbeBW);
                    self.pacing_gain.store(1000, Ordering::Release); // 1.0
                }
            }
            BbrState::ProbeBW => {
                // Cycle through pacing gains [1.25, 0.75, 1, 1, 1, 1, 1, 1]
                let round = self.round_count.load(Ordering::Acquire) % 8;
                let gain = match round {
                    0 => 1250,
                    1 => 750,
                    _ => 1000,
                };
                self.pacing_gain.store(gain, Ordering::Release);
            }
            BbrState::ProbeRTT => {
                // Keep cwnd at minimum for one RTT
                if self.probe_rtt_round_done.load(Ordering::Acquire) != 0 {
                    self.set_state(BbrState::ProbeBW);
                }
            }
        }

        // Update cwnd
        let target = self.target_cwnd();
        self.cwnd.store(target, Ordering::Release);

        // Increment round count
        self.round_count.fetch_add(1, Ordering::AcqRel);
    }

    fn on_packet_lost(&self, lost_bytes: u64, _now: Duration) {
        self.bytes_in_flight.fetch_sub(lost_bytes, Ordering::AcqRel);
        // BBR doesn't reduce cwnd on loss directly
        // Loss is treated as a signal to probe less aggressively
    }

    fn on_congestion_event(&self, _sent_time: Duration, _now: Duration) {
        // BBR handles congestion differently - it doesn't use multiplicative decrease
    }

    fn on_ecn_ce(&self, _now: Duration) {
        // Reduce pacing rate slightly on ECN
        let gain = self.pacing_gain.load(Ordering::Acquire);
        let new_gain = (gain as f64 * 0.95) as u64;
        self.pacing_gain.store(new_gain.max(500), Ordering::Release);
    }

    fn reset(&self) {
        self.cwnd.store(INITIAL_WINDOW, Ordering::Release);
        self.bytes_in_flight.store(0, Ordering::Release);
        self.state.store(BbrState::Startup as u64, Ordering::Release);
        self.btl_bw.store(0, Ordering::Release);
        self.min_rtt.store(u64::MAX, Ordering::Release);
        self.round_count.store(0, Ordering::Release);
        self.full_bw_reached.store(0, Ordering::Release);
        self.full_bw_count.store(0, Ordering::Release);
    }

    fn pacing_rate(&self) -> u64 {
        let btl_bw = self.btl_bw.load(Ordering::Acquire);
        let gain = self.pacing_gain.load(Ordering::Acquire) as f64 / 1000.0;

        if btl_bw == 0 {
            return u64::MAX;
        }

        (btl_bw as f64 * gain) as u64
    }

    fn update_rtt(&self, _rtt: Duration, min_rtt: Duration) {
        self.min_rtt.fetch_min(min_rtt, Ordering::AcqRel);
    }

    fn algorithm(&self) -> CongestionAlgorithm {
        CongestionAlgorithm::Bbr
    }
}

// ============================================================================
// Congestion Controller Factory
// ============================================================================

/// Create a congestion controller
pub fn create_controller(
    algorithm: CongestionAlgorithm,
    max_datagram_size: u64,
) -> Box<dyn CongestionController> {
    match algorithm {
        CongestionAlgorithm::NewReno => Box::new(NewReno::new(max_datagram_size)),
        CongestionAlgorithm::Cubic => Box::new(Cubic::new(max_datagram_size)),
        CongestionAlgorithm::Bbr | CongestionAlgorithm::Bbr2 => {
            Box::new(Bbr::new(max_datagram_size))
        }
    }
}

// ============================================================================
// Pacer
// ============================================================================

/// Packet pacer for smooth sending
pub struct Pacer {
    /// Tokens available (bytes * time unit)
    tokens: AtomicU64,
    /// Last update time
    last_update: AtomicU64,
    /// Pacing rate (bytes per second)
    rate: AtomicU64,
    /// Maximum burst size
    max_burst: u64,
}

impl Pacer {
    /// Create a new pacer
    pub fn new(initial_rate: u64, max_burst: u64) -> Self {
        Self {
            tokens: AtomicU64::new(max_burst),
            last_update: AtomicU64::new(0),
            rate: AtomicU64::new(initial_rate),
            max_burst,
        }
    }

    /// Update the pacing rate
    pub fn set_rate(&self, rate: u64) {
        self.rate.store(rate, Ordering::Release);
    }

    /// Check if we can send bytes
    pub fn can_send(&self, bytes: u64, now: Duration) -> bool {
        self.refill_tokens(now);
        self.tokens.load(Ordering::Acquire) >= bytes
    }

    /// Consume tokens for sending
    pub fn on_sent(&self, bytes: u64, now: Duration) {
        self.refill_tokens(now);
        self.tokens.fetch_sub(bytes.min(self.tokens.load(Ordering::Acquire)), Ordering::AcqRel);
    }

    /// Get time until we can send
    pub fn time_until_send(&self, bytes: u64, now: Duration) -> Duration {
        self.refill_tokens(now);

        let tokens = self.tokens.load(Ordering::Acquire);
        if tokens >= bytes {
            return 0;
        }

        let rate = self.rate.load(Ordering::Acquire);
        if rate == 0 {
            return Duration::MAX;
        }

        let needed = bytes - tokens;
        (needed as u128 * 1_000_000_000 / rate as u128) as Duration
    }

    /// Refill tokens based on elapsed time
    fn refill_tokens(&self, now: Duration) {
        let last = self.last_update.load(Ordering::Acquire);
        if now <= last {
            return;
        }

        let elapsed = now - last;
        let rate = self.rate.load(Ordering::Acquire);

        // Calculate new tokens
        let new_tokens = (rate as u128 * elapsed as u128 / 1_000_000_000) as u64;

        if new_tokens > 0 {
            let current = self.tokens.load(Ordering::Acquire);
            let total = (current + new_tokens).min(self.max_burst);
            self.tokens.store(total, Ordering::Release);
            self.last_update.store(now, Ordering::Release);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_new_reno_basic() {
        let cc = NewReno::new(1200);

        assert_eq!(cc.cwnd(), INITIAL_WINDOW);
        assert!(cc.in_slow_start());

        // Send and ack
        cc.on_packet_sent(1200, 0);
        cc.on_packet_acked(1200, 50_000_000, 50_000_000);

        // CWND should increase
        assert!(cc.cwnd() > INITIAL_WINDOW);
    }

    #[test]
    fn test_cubic_congestion() {
        let cc = Cubic::new(1200);

        // Trigger congestion
        cc.on_packet_sent(12000, 0);
        cc.on_congestion_event(0, 100_000_000);

        assert!(cc.in_recovery());
        assert!(cc.cwnd() < INITIAL_WINDOW);
    }

    #[test]
    fn test_bbr_startup() {
        let cc = Bbr::new(1200);

        assert_eq!(cc.state(), BbrState::Startup);
        assert!(cc.in_slow_start());
    }

    #[test]
    fn test_pacer() {
        let pacer = Pacer::new(1_000_000, 10000); // 1 MB/s

        assert!(pacer.can_send(1000, 0));
        pacer.on_sent(1000, 0);
    }
}
