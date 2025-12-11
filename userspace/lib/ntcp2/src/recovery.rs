//! QUIC Loss Detection and Recovery
//!
//! This module implements QUIC loss detection and recovery according to RFC 9002.
//!
//! ## Loss Detection Mechanisms
//!
//! 1. **Packet threshold**: A packet is lost if a later packet has been acknowledged
//!    and the gap exceeds `kPacketThreshold` (default: 3)
//!
//! 2. **Time threshold**: A packet is lost if it was sent more than a threshold
//!    time ago: `max(kTimeThreshold * max(smoothed_rtt, latest_rtt), kGranularity)`
//!
//! ## Probe Timeout (PTO)
//!
//! PTO triggers retransmission when no ACK is received:
//! `PTO = smoothed_rtt + max(4 * rttvar, kGranularity) + max_ack_delay`

use crate::error::{Error, NgError, Result};
use crate::types::EncryptionLevel;
use crate::{Duration, Timestamp, NGTCP2_TSTAMP_MAX};

use parking_lot::RwLock;
use std::collections::{BTreeMap, HashMap, VecDeque};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};

// ============================================================================
// Constants (RFC 9002)
// ============================================================================

/// Maximum reordering in packets before a packet is considered lost
pub const PACKET_THRESHOLD: u64 = 3;

/// Maximum reordering in time (RTT multiplier)
pub const TIME_THRESHOLD: f64 = 9.0 / 8.0;

/// Timer granularity (1ms in nanoseconds)
pub const TIMER_GRANULARITY: Duration = 1_000_000;

/// Initial RTT estimate (333ms in nanoseconds)
pub const INITIAL_RTT: Duration = 333_000_000;

/// Maximum ACK delay (25ms in nanoseconds, RFC 9000 default)
pub const MAX_ACK_DELAY: Duration = 25_000_000;

/// Minimum CRYPTO data retransmission timeout (1 second)
pub const MIN_CRYPTO_TIMEOUT: Duration = 1_000_000_000;

// ============================================================================
// Sent Packet
// ============================================================================

/// Information about a sent packet
#[derive(Debug, Clone)]
pub struct SentPacket {
    /// Packet number
    pub pkt_num: u64,
    /// Time sent (nanoseconds)
    pub time_sent: Timestamp,
    /// Is ACK-eliciting (requires acknowledgment)
    pub ack_eliciting: bool,
    /// Is in flight (counts towards bytes_in_flight)
    pub in_flight: bool,
    /// Size in bytes (for congestion control)
    pub size: usize,
    /// Encryption level
    pub level: EncryptionLevel,
    /// Frames in this packet (for retransmission)
    pub frames: Vec<SentFrame>,
}

/// Frame sent in a packet (for retransmission tracking)
#[derive(Debug, Clone)]
pub enum SentFrame {
    /// CRYPTO frame
    Crypto { offset: u64, len: usize },
    /// STREAM frame
    Stream { stream_id: i64, offset: u64, len: usize, fin: bool },
    /// ACK frame
    Ack { largest_acked: u64 },
    /// Other frame types (no retransmission needed)
    Other,
}

// ============================================================================
// ACK Range
// ============================================================================

/// Range of acknowledged packets
#[derive(Debug, Clone, Copy)]
pub struct AckRange {
    /// Start of range (inclusive)
    pub start: u64,
    /// End of range (inclusive)
    pub end: u64,
}

impl AckRange {
    /// Create a new ACK range
    pub fn new(start: u64, end: u64) -> Self {
        Self { start, end }
    }

    /// Check if a packet number is in this range
    pub fn contains(&self, pkt_num: u64) -> bool {
        pkt_num >= self.start && pkt_num <= self.end
    }
}

// ============================================================================
// RTT Estimator
// ============================================================================

/// RTT estimation and tracking
pub struct RttEstimator {
    /// Latest RTT sample
    latest_rtt: AtomicU64,
    /// Smoothed RTT (exponential moving average)
    smoothed_rtt: AtomicU64,
    /// RTT variance
    rttvar: AtomicU64,
    /// Minimum RTT observed
    min_rtt: AtomicU64,
    /// First RTT sample received
    has_sample: AtomicBool,
    /// Maximum ACK delay
    max_ack_delay: AtomicU64,
}

impl RttEstimator {
    /// Create a new RTT estimator
    pub fn new() -> Self {
        Self {
            latest_rtt: AtomicU64::new(INITIAL_RTT),
            smoothed_rtt: AtomicU64::new(INITIAL_RTT),
            rttvar: AtomicU64::new(INITIAL_RTT / 2),
            min_rtt: AtomicU64::new(u64::MAX),
            has_sample: AtomicBool::new(false),
            max_ack_delay: AtomicU64::new(MAX_ACK_DELAY),
        }
    }

    /// Update RTT with a new sample
    ///
    /// Based on RFC 9002 Section 5.3
    pub fn update_rtt(&self, ack_delay: Duration, rtt_sample: Duration) {
        self.latest_rtt.store(rtt_sample, Ordering::Release);

        // Update min_rtt
        self.min_rtt.fetch_min(rtt_sample, Ordering::AcqRel);

        // Adjust for ACK delay
        let min_rtt = self.min_rtt.load(Ordering::Acquire);
        let max_ack_delay = self.max_ack_delay.load(Ordering::Acquire);
        let adjusted_rtt = if rtt_sample > min_rtt + ack_delay {
            rtt_sample - ack_delay.min(max_ack_delay)
        } else {
            rtt_sample
        };

        if !self.has_sample.swap(true, Ordering::AcqRel) {
            // First sample
            self.smoothed_rtt.store(adjusted_rtt, Ordering::Release);
            self.rttvar.store(adjusted_rtt / 2, Ordering::Release);
        } else {
            // Update EWMA
            let smoothed_rtt = self.smoothed_rtt.load(Ordering::Acquire);
            let rttvar = self.rttvar.load(Ordering::Acquire);

            // rttvar = 3/4 * rttvar + 1/4 * |smoothed_rtt - adjusted_rtt|
            let delta = if adjusted_rtt > smoothed_rtt {
                adjusted_rtt - smoothed_rtt
            } else {
                smoothed_rtt - adjusted_rtt
            };
            let new_rttvar = (rttvar * 3 + delta) / 4;
            self.rttvar.store(new_rttvar, Ordering::Release);

            // smoothed_rtt = 7/8 * smoothed_rtt + 1/8 * adjusted_rtt
            let new_smoothed = (smoothed_rtt * 7 + adjusted_rtt) / 8;
            self.smoothed_rtt.store(new_smoothed, Ordering::Release);
        }
    }

    /// Get latest RTT
    pub fn latest_rtt(&self) -> Duration {
        self.latest_rtt.load(Ordering::Acquire)
    }

    /// Get smoothed RTT
    pub fn smoothed_rtt(&self) -> Duration {
        self.smoothed_rtt.load(Ordering::Acquire)
    }

    /// Get RTT variance
    pub fn rttvar(&self) -> Duration {
        self.rttvar.load(Ordering::Acquire)
    }

    /// Get minimum RTT
    pub fn min_rtt(&self) -> Duration {
        let min = self.min_rtt.load(Ordering::Acquire);
        if min == u64::MAX {
            INITIAL_RTT
        } else {
            min
        }
    }

    /// Calculate PTO (Probe Timeout)
    ///
    /// PTO = smoothed_rtt + max(4 * rttvar, kGranularity) + max_ack_delay
    pub fn pto(&self, include_max_ack_delay: bool) -> Duration {
        let smoothed_rtt = self.smoothed_rtt();
        let rttvar = self.rttvar();
        let max_ack_delay = if include_max_ack_delay {
            self.max_ack_delay.load(Ordering::Acquire)
        } else {
            0
        };

        smoothed_rtt + (4 * rttvar).max(TIMER_GRANULARITY) + max_ack_delay
    }

    /// Calculate loss delay threshold
    ///
    /// time_threshold = max(kTimeThreshold * max(smoothed_rtt, latest_rtt), kGranularity)
    pub fn loss_delay(&self) -> Duration {
        let smoothed_rtt = self.smoothed_rtt();
        let latest_rtt = self.latest_rtt();
        let max_rtt = smoothed_rtt.max(latest_rtt);

        ((max_rtt as f64 * TIME_THRESHOLD) as Duration).max(TIMER_GRANULARITY)
    }

    /// Set maximum ACK delay (from transport parameters)
    pub fn set_max_ack_delay(&self, delay: Duration) {
        self.max_ack_delay.store(delay, Ordering::Release);
    }
}

impl Default for RttEstimator {
    fn default() -> Self {
        Self::new()
    }
}

// ============================================================================
// Packet Number Space
// ============================================================================

/// Packet number space (Initial, Handshake, or Application Data)
#[repr(u8)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum PacketNumberSpace {
    /// Initial space
    Initial = 0,
    /// Handshake space
    Handshake = 1,
    /// Application Data space (1-RTT)
    ApplicationData = 2,
}

impl From<EncryptionLevel> for PacketNumberSpace {
    fn from(level: EncryptionLevel) -> Self {
        match level {
            EncryptionLevel::Initial => PacketNumberSpace::Initial,
            EncryptionLevel::ZeroRtt => PacketNumberSpace::ApplicationData,
            EncryptionLevel::Handshake => PacketNumberSpace::Handshake,
            EncryptionLevel::OneRtt => PacketNumberSpace::ApplicationData,
        }
    }
}

/// State for a single packet number space
pub struct PnSpaceState {
    /// Sent packets awaiting acknowledgment
    sent_packets: BTreeMap<u64, SentPacket>,
    /// Largest acknowledged packet number
    largest_acked: Option<u64>,
    /// Time of last ACK-eliciting packet
    time_of_last_ack_eliciting: Option<Timestamp>,
    /// Loss time (earliest time a packet might be declared lost)
    loss_time: Option<Timestamp>,
    /// ACK frequency
    ack_frequency: u64,
}

impl PnSpaceState {
    /// Create a new packet number space state
    pub fn new() -> Self {
        Self {
            sent_packets: BTreeMap::new(),
            largest_acked: None,
            time_of_last_ack_eliciting: None,
            loss_time: None,
            ack_frequency: 1,
        }
    }

    /// Add a sent packet
    pub fn on_packet_sent(&mut self, packet: SentPacket) {
        if packet.ack_eliciting {
            self.time_of_last_ack_eliciting = Some(packet.time_sent);
        }
        self.sent_packets.insert(packet.pkt_num, packet);
    }

    /// Remove acknowledged packet
    pub fn on_packet_acked(&mut self, pkt_num: u64) -> Option<SentPacket> {
        self.sent_packets.remove(&pkt_num)
    }

    /// Get packets in range
    pub fn get_packets_in_range(&self, start: u64, end: u64) -> Vec<&SentPacket> {
        self.sent_packets
            .range(start..=end)
            .map(|(_, pkt)| pkt)
            .collect()
    }

    /// Get unacked sent packets iterator
    pub fn sent_packets(&self) -> impl Iterator<Item = &SentPacket> {
        self.sent_packets.values()
    }

    /// Check if there are unacked packets
    pub fn has_unacked(&self) -> bool {
        !self.sent_packets.is_empty()
    }

    /// Set loss time
    pub fn set_loss_time(&mut self, time: Option<Timestamp>) {
        self.loss_time = time;
    }

    /// Get loss time
    pub fn loss_time(&self) -> Option<Timestamp> {
        self.loss_time
    }
}

impl Default for PnSpaceState {
    fn default() -> Self {
        Self::new()
    }
}

// ============================================================================
// Loss Detector
// ============================================================================

/// Loss detection state
pub struct LossDetector {
    /// RTT estimator
    rtt: RttEstimator,
    /// State per packet number space
    spaces: RwLock<[PnSpaceState; 3]>,
    /// Number of PTO events
    pto_count: AtomicU64,
    /// Alarm time (earliest of all timers)
    loss_detection_timer: AtomicU64,
    /// Bytes in flight
    bytes_in_flight: AtomicU64,
    /// Number of ack-eliciting packets in flight
    ack_eliciting_in_flight: AtomicU64,
    /// Handshake confirmed
    handshake_confirmed: AtomicBool,
    /// Peer completed address validation (server only)
    peer_completed_address_validation: AtomicBool,
}

impl LossDetector {
    /// Create a new loss detector
    pub fn new() -> Self {
        Self {
            rtt: RttEstimator::new(),
            spaces: RwLock::new([
                PnSpaceState::new(),
                PnSpaceState::new(),
                PnSpaceState::new(),
            ]),
            pto_count: AtomicU64::new(0),
            loss_detection_timer: AtomicU64::new(NGTCP2_TSTAMP_MAX),
            bytes_in_flight: AtomicU64::new(0),
            ack_eliciting_in_flight: AtomicU64::new(0),
            handshake_confirmed: AtomicBool::new(false),
            peer_completed_address_validation: AtomicBool::new(false),
        }
    }

    /// Get RTT estimator
    pub fn rtt(&self) -> &RttEstimator {
        &self.rtt
    }

    /// Get bytes in flight
    pub fn bytes_in_flight(&self) -> u64 {
        self.bytes_in_flight.load(Ordering::Acquire)
    }

    /// Record a packet being sent
    pub fn on_packet_sent(&self, packet: SentPacket) {
        let space = PacketNumberSpace::from(packet.level) as usize;

        if packet.in_flight {
            self.bytes_in_flight
                .fetch_add(packet.size as u64, Ordering::AcqRel);
        }

        if packet.ack_eliciting {
            self.ack_eliciting_in_flight.fetch_add(1, Ordering::AcqRel);
        }

        let mut spaces = self.spaces.write();
        spaces[space].on_packet_sent(packet);
    }

    /// Process an ACK frame
    ///
    /// Returns (newly acked packets, lost packets)
    pub fn on_ack_received(
        &self,
        space: PacketNumberSpace,
        largest_acked: u64,
        ack_delay: Duration,
        ack_ranges: &[AckRange],
        now: Timestamp,
    ) -> (Vec<SentPacket>, Vec<SentPacket>) {
        let mut spaces = self.spaces.write();
        let pn_space = &mut spaces[space as usize];

        let mut newly_acked = Vec::new();
        let mut lost = Vec::new();

        // Process ACK ranges
        for range in ack_ranges {
            for pkt_num in range.start..=range.end {
                if let Some(packet) = pn_space.on_packet_acked(pkt_num) {
                    if packet.in_flight {
                        self.bytes_in_flight
                            .fetch_sub(packet.size as u64, Ordering::AcqRel);
                    }
                    if packet.ack_eliciting {
                        self.ack_eliciting_in_flight.fetch_sub(1, Ordering::AcqRel);
                    }
                    newly_acked.push(packet);
                }
            }
        }

        // Update RTT if largest newly acked
        if let Some(largest_packet) = newly_acked.iter().find(|p| p.pkt_num == largest_acked) {
            let rtt_sample = now.saturating_sub(largest_packet.time_sent);
            self.rtt.update_rtt(ack_delay, rtt_sample);

            // Reset PTO count on valid ACK
            self.pto_count.store(0, Ordering::Release);
        }

        // Update largest acked
        pn_space.largest_acked = Some(
            pn_space
                .largest_acked
                .map(|la| la.max(largest_acked))
                .unwrap_or(largest_acked),
        );

        // Detect lost packets
        lost = self.detect_lost_packets(space, now);

        // Reset loss time and recalculate
        pn_space.set_loss_time(None);
        let loss_delay = self.rtt.loss_delay();

        for packet in pn_space.sent_packets() {
            if packet.pkt_num > largest_acked {
                continue;
            }

            let time_since_sent = now.saturating_sub(packet.time_sent);

            if time_since_sent > loss_delay {
                // Would be declared lost - already handled above
            } else {
                // Might be lost later - set loss time
                let loss_time = packet.time_sent + loss_delay;
                match pn_space.loss_time() {
                    Some(current) if current <= loss_time => {}
                    _ => pn_space.set_loss_time(Some(loss_time)),
                }
            }
        }

        drop(spaces);

        // Set loss detection timer
        self.set_loss_detection_timer(now);

        (newly_acked, lost)
    }

    /// Detect lost packets in a space
    fn detect_lost_packets(&self, space: PacketNumberSpace, now: Timestamp) -> Vec<SentPacket> {
        let mut spaces = self.spaces.write();
        let pn_space = &mut spaces[space as usize];

        let Some(largest_acked) = pn_space.largest_acked else {
            return Vec::new();
        };

        let loss_delay = self.rtt.loss_delay();
        let lost_send_time = now.saturating_sub(loss_delay);

        let mut lost = Vec::new();
        let mut to_remove = Vec::new();

        for (pkt_num, packet) in pn_space.sent_packets.iter() {
            if *pkt_num > largest_acked {
                continue;
            }

            // Check packet threshold
            let packet_threshold_lost = largest_acked >= PACKET_THRESHOLD + *pkt_num;

            // Check time threshold
            let time_threshold_lost = packet.time_sent <= lost_send_time;

            if packet_threshold_lost || time_threshold_lost {
                to_remove.push(*pkt_num);
            }
        }

        for pkt_num in to_remove {
            if let Some(packet) = pn_space.sent_packets.remove(&pkt_num) {
                if packet.in_flight {
                    self.bytes_in_flight
                        .fetch_sub(packet.size as u64, Ordering::AcqRel);
                }
                if packet.ack_eliciting {
                    self.ack_eliciting_in_flight.fetch_sub(1, Ordering::AcqRel);
                }
                lost.push(packet);
            }
        }

        lost
    }

    /// Set loss detection timer
    fn set_loss_detection_timer(&self, now: Timestamp) {
        let spaces = self.spaces.read();

        // Check for loss time
        let mut earliest_loss_time = None;
        for space in spaces.iter() {
            if let Some(lt) = space.loss_time() {
                earliest_loss_time = Some(
                    earliest_loss_time
                        .map(|e: Timestamp| e.min(lt))
                        .unwrap_or(lt),
                );
            }
        }

        if let Some(loss_time) = earliest_loss_time {
            self.loss_detection_timer.store(loss_time, Ordering::Release);
            return;
        }

        // No loss time - check for ack-eliciting in flight
        if self.ack_eliciting_in_flight.load(Ordering::Acquire) == 0 {
            if self.peer_completed_address_validation.load(Ordering::Acquire) {
                // No timer needed
                self.loss_detection_timer.store(NGTCP2_TSTAMP_MAX, Ordering::Release);
                return;
            }
        }

        // Calculate PTO
        let pto_count = self.pto_count.load(Ordering::Acquire);
        let handshake_confirmed = self.handshake_confirmed.load(Ordering::Acquire);

        // Include max_ack_delay only if handshake confirmed
        let pto = self.rtt.pto(handshake_confirmed);
        let pto_timeout = pto << pto_count; // Exponential backoff

        // Find earliest time of ack-eliciting packet
        let mut earliest_time = NGTCP2_TSTAMP_MAX;
        for space in spaces.iter() {
            if let Some(t) = space.time_of_last_ack_eliciting {
                earliest_time = earliest_time.min(t);
            }
        }

        if earliest_time == NGTCP2_TSTAMP_MAX {
            earliest_time = now;
        }

        let timer = earliest_time + pto_timeout;
        self.loss_detection_timer.store(timer, Ordering::Release);
    }

    /// Get loss detection timer expiry
    pub fn get_loss_detection_timer(&self) -> Timestamp {
        self.loss_detection_timer.load(Ordering::Acquire)
    }

    /// Handle loss detection timer expiry
    ///
    /// Returns frames that need retransmission
    pub fn on_loss_detection_timeout(&self, now: Timestamp) -> Vec<SentFrame> {
        let mut retransmit = Vec::new();

        // Check for loss time expiry
        {
            let spaces = self.spaces.read();
            for (i, space) in spaces.iter().enumerate() {
                if let Some(loss_time) = space.loss_time() {
                    if now >= loss_time {
                        // Detect losses
                        drop(spaces);
                        let pn_space = match i {
                            0 => PacketNumberSpace::Initial,
                            1 => PacketNumberSpace::Handshake,
                            _ => PacketNumberSpace::ApplicationData,
                        };
                        let lost = self.detect_lost_packets(pn_space, now);

                        // Collect frames for retransmission
                        for packet in lost {
                            retransmit.extend(packet.frames);
                        }

                        self.set_loss_detection_timer(now);
                        return retransmit;
                    }
                }
            }
        }

        // PTO expired - send probe
        self.pto_count.fetch_add(1, Ordering::AcqRel);

        // Determine which space to probe
        let spaces = self.spaces.read();

        if !self.handshake_confirmed.load(Ordering::Acquire) {
            // Probe Initial or Handshake
            for (i, space) in spaces.iter().enumerate() {
                if space.has_unacked() {
                    // Return frames from earliest unacked packet for retransmission
                    if let Some(packet) = space.sent_packets().next() {
                        retransmit.extend(packet.frames.clone());
                    }
                    break;
                }
            }
        } else {
            // Probe Application Data
            if let Some(packet) = spaces[2].sent_packets().next() {
                retransmit.extend(packet.frames.clone());
            }
        }

        drop(spaces);
        self.set_loss_detection_timer(now);

        retransmit
    }

    /// Mark handshake as confirmed
    pub fn on_handshake_confirmed(&self) {
        self.handshake_confirmed.store(true, Ordering::Release);
    }

    /// Mark peer as having completed address validation
    pub fn on_peer_completed_address_validation(&self) {
        self.peer_completed_address_validation.store(true, Ordering::Release);
    }

    /// Discard a packet number space (e.g., after handshake)
    pub fn discard_space(&self, space: PacketNumberSpace) {
        let mut spaces = self.spaces.write();
        let pn_space = &mut spaces[space as usize];

        // Remove all packets from bytes in flight
        for packet in pn_space.sent_packets.values() {
            if packet.in_flight {
                self.bytes_in_flight
                    .fetch_sub(packet.size as u64, Ordering::AcqRel);
            }
            if packet.ack_eliciting {
                self.ack_eliciting_in_flight.fetch_sub(1, Ordering::AcqRel);
            }
        }

        *pn_space = PnSpaceState::new();
    }

    /// Check if in persistent congestion
    ///
    /// Persistent congestion occurs when all packets sent over a period
    /// greater than the persistent congestion duration are lost.
    pub fn in_persistent_congestion(&self, lost_packets: &[SentPacket]) -> bool {
        if lost_packets.len() < 2 {
            return false;
        }

        // Find the time range of lost packets
        let mut earliest = NGTCP2_TSTAMP_MAX;
        let mut latest = 0;

        for packet in lost_packets {
            earliest = earliest.min(packet.time_sent);
            latest = latest.max(packet.time_sent);
        }

        // Duration must exceed persistent congestion duration
        // PC duration = smoothed_rtt + max(4 * rttvar, kGranularity) + max_ack_delay
        let pc_duration = self.rtt.pto(true) * PERSISTENT_CONGESTION_THRESHOLD;

        latest - earliest > pc_duration
    }

    /// Get PTO count
    pub fn pto_count(&self) -> u64 {
        self.pto_count.load(Ordering::Acquire)
    }

    /// Reset PTO count (on successful ACK)
    pub fn reset_pto_count(&self) {
        self.pto_count.store(0, Ordering::Release);
    }
}

impl Default for LossDetector {
    fn default() -> Self {
        Self::new()
    }
}

// ============================================================================
// Crypto Retransmission
// ============================================================================

/// Tracks CRYPTO data that needs retransmission
pub struct CryptoRetransmitter {
    /// Pending CRYPTO data per level (offset, data)
    pending: [VecDeque<(u64, Vec<u8>)>; 4],
    /// Next expected offset per level
    next_offset: [u64; 4],
}

impl CryptoRetransmitter {
    /// Create a new crypto retransmitter
    pub fn new() -> Self {
        Self {
            pending: [
                VecDeque::new(),
                VecDeque::new(),
                VecDeque::new(),
                VecDeque::new(),
            ],
            next_offset: [0; 4],
        }
    }

    /// Queue CRYPTO data for sending
    pub fn write(&mut self, level: EncryptionLevel, data: &[u8]) {
        let idx = level as usize;
        let offset = self.next_offset[idx];
        self.pending[idx].push_back((offset, data.to_vec()));
        self.next_offset[idx] += data.len() as u64;
    }

    /// Mark CRYPTO data as acknowledged
    pub fn on_ack(&mut self, level: EncryptionLevel, offset: u64, len: usize) {
        let idx = level as usize;
        self.pending[idx].retain(|(o, d)| {
            let end = *o + d.len() as u64;
            // Keep if not fully acknowledged
            end > offset + len as u64 || *o >= offset + len as u64
        });
    }

    /// Mark CRYPTO data as lost (needs retransmission)
    pub fn on_loss(&mut self, level: EncryptionLevel, offset: u64, len: usize) {
        // Already in pending, will be retransmitted
    }

    /// Get next CRYPTO data to send
    pub fn next(&self, level: EncryptionLevel, max_len: usize) -> Option<(u64, &[u8])> {
        let idx = level as usize;
        self.pending[idx].front().map(|(offset, data)| {
            let len = data.len().min(max_len);
            (*offset, &data[..len])
        })
    }

    /// Check if there's pending CRYPTO data
    pub fn has_pending(&self, level: EncryptionLevel) -> bool {
        !self.pending[level as usize].is_empty()
    }
}

impl Default for CryptoRetransmitter {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_rtt_estimator() {
        let rtt = RttEstimator::new();

        // First sample
        rtt.update_rtt(0, 100_000_000); // 100ms
        assert_eq!(rtt.smoothed_rtt(), 100_000_000);

        // Second sample
        rtt.update_rtt(0, 80_000_000); // 80ms
        // Should be smoothed: (100*7 + 80)/8 = 97.5ms
        assert!(rtt.smoothed_rtt() < 100_000_000);
    }

    #[test]
    fn test_pto_calculation() {
        let rtt = RttEstimator::new();
        rtt.update_rtt(0, 100_000_000);

        let pto = rtt.pto(true);
        // PTO = smoothed_rtt + max(4*rttvar, granularity) + max_ack_delay
        assert!(pto > 100_000_000);
    }

    #[test]
    fn test_loss_detector() {
        let detector = LossDetector::new();

        // Send a packet
        let packet = SentPacket {
            pkt_num: 0,
            time_sent: 0,
            ack_eliciting: true,
            in_flight: true,
            size: 1200,
            level: EncryptionLevel::Initial,
            frames: vec![],
        };
        detector.on_packet_sent(packet);

        assert_eq!(detector.bytes_in_flight(), 1200);
    }

    #[test]
    fn test_ack_range() {
        let range = AckRange::new(5, 10);
        assert!(range.contains(5));
        assert!(range.contains(7));
        assert!(range.contains(10));
        assert!(!range.contains(4));
        assert!(!range.contains(11));
    }
}
