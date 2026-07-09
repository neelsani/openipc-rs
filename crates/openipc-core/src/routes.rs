use std::{collections::HashMap, sync::Arc};

use crate::channel::ChannelId;
use crate::ieee80211::{FrameLayout, WifiFrame};
use crate::pipeline::{
    MockPayloadPipeline, PayloadPipeline, PayloadPipelineEvent, RecoveredPayload,
};
use crate::wfb::{FecCounters, WfbError, WfbKeypair};

/// Application-defined identifier for a recovered-payload output.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct PayloadRouteId(u64);

impl PayloadRouteId {
    /// Create a route id from a stable numeric value.
    pub const fn new(raw: u64) -> Self {
        Self(raw)
    }

    /// Return the raw route id value.
    pub const fn raw(self) -> u64 {
        self.0
    }
}

/// Key for one WFB runtime inside [`PayloadRouteManager`].
///
/// Routes with the same `(channel_id, key_slot)` share decryption, FEC state,
/// and counters.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct PayloadRuntimeKey {
    channel_id: ChannelId,
    key_slot: u64,
}

impl PayloadRuntimeKey {
    /// Create a runtime key from a channel id and caller-defined key slot.
    pub const fn new(channel_id: ChannelId, key_slot: u64) -> Self {
        Self {
            channel_id,
            key_slot,
        }
    }

    /// Return the WFB/OpenIPC channel id for this runtime.
    pub const fn channel_id(self) -> ChannelId {
        self.channel_id
    }

    /// Return the key slot for this runtime.
    pub const fn key_slot(self) -> u64 {
        self.key_slot
    }
}

/// Event emitted by route-manager processing.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PayloadRouteEvent {
    /// Frame did not match any configured route or usable payload.
    IgnoredFrame,
    /// A WFB session packet established or refreshed a runtime session.
    SessionEstablished {
        /// Runtime whose WFB session changed.
        runtime: PayloadRuntimeKey,
        /// Route ids attached to the runtime.
        route_ids: Arc<[PayloadRouteId]>,
        /// Session epoch accepted from the transmitter.
        epoch: u64,
        /// Number of primary fragments in each FEC block.
        fec_k: usize,
        /// Total primary plus parity fragments in each FEC block.
        fec_n: usize,
    },
    /// A recovered payload was emitted by a runtime.
    Payload {
        /// Runtime that recovered the payload.
        runtime: PayloadRuntimeKey,
        /// Route ids that should receive the payload.
        route_ids: Arc<[PayloadRouteId]>,
        /// Recovered payload bytes and packet metadata.
        payload: RecoveredPayload,
    },
}

/// Error returned while routing a WFB frame or decrypted fragment.
#[derive(Debug, PartialEq, Eq)]
pub enum PayloadRouteError {
    /// Caller referenced a runtime key that is not registered.
    UnknownRuntime(PayloadRuntimeKey),
    /// Underlying WFB parser/decrypt/FEC error.
    Wfb(WfbError),
}

impl std::fmt::Display for PayloadRouteError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::UnknownRuntime(key) => write!(
                f,
                "unknown payload runtime for channel 0x{:08x} key slot {}",
                key.channel_id().raw(),
                key.key_slot()
            ),
            Self::Wfb(err) => std::fmt::Display::fmt(err, f),
        }
    }
}

impl std::error::Error for PayloadRouteError {}

impl From<WfbError> for PayloadRouteError {
    fn from(err: WfbError) -> Self {
        Self::Wfb(err)
    }
}

#[derive(Debug, Clone)]
enum PayloadChannelPipeline {
    Real(Box<PayloadPipeline>),
    Mock(MockPayloadPipeline),
}

impl PayloadChannelPipeline {
    fn channel_id(&self) -> ChannelId {
        match self {
            Self::Real(pipeline) => pipeline.channel_id(),
            Self::Mock(pipeline) => pipeline.channel_id(),
        }
    }

    fn fec_counters(&self) -> FecCounters {
        match self {
            Self::Real(pipeline) => pipeline.fec_counters(),
            Self::Mock(pipeline) => pipeline.fec_counters(),
        }
    }

    fn accepts_80211_frame(&self, frame: &[u8]) -> bool {
        match self {
            Self::Real(pipeline) => pipeline.accepts_80211_frame(frame),
            Self::Mock(_) => false,
        }
    }

    fn push_matched_payload_into(
        &mut self,
        payload: &[u8],
        events: &mut Vec<PayloadPipelineEvent>,
    ) -> Result<(), WfbError> {
        match self {
            Self::Real(pipeline) => pipeline.push_matched_payload_into(payload, events),
            Self::Mock(_) => {
                events.clear();
                events.push(PayloadPipelineEvent::IgnoredFrame);
                Ok(())
            }
        }
    }

    fn push_decrypted_80211_frame(
        &mut self,
        frame: &[u8],
        decrypted_fragment: &[u8],
    ) -> Result<Vec<PayloadPipelineEvent>, WfbError> {
        match self {
            Self::Real(pipeline) => pipeline.push_decrypted_80211_frame(frame, decrypted_fragment),
            Self::Mock(_) => Ok(vec![PayloadPipelineEvent::IgnoredFrame]),
        }
    }

    fn push_decrypted_fragment(
        &mut self,
        data_nonce: u64,
        decrypted_fragment: &[u8],
    ) -> Result<Vec<PayloadPipelineEvent>, WfbError> {
        match self {
            Self::Real(pipeline) => {
                pipeline.push_decrypted_fragment(data_nonce, decrypted_fragment)
            }
            Self::Mock(pipeline) => Ok(pipeline.push_payload(data_nonce, decrypted_fragment)),
        }
    }

    fn push_mock_payload(&mut self, packet_seq: u64, data: &[u8]) -> Vec<PayloadPipelineEvent> {
        match self {
            Self::Real(_) => vec![PayloadPipelineEvent::IgnoredFrame],
            Self::Mock(pipeline) => pipeline.push_payload(packet_seq, data),
        }
    }
}

/// One route-manager runtime for a single WFB/OpenIPC channel and key slot.
///
/// A runtime can be backed by the real WFB [`PayloadPipeline`] or by a fully
/// synthetic [`MockPayloadPipeline`]. Route IDs attached to the same runtime
/// share the same recovered payload stream.
#[derive(Debug, Clone)]
pub struct PayloadChannelRuntime {
    pipeline: PayloadChannelPipeline,
    route_ids: Arc<[PayloadRouteId]>,
    pipeline_events: Vec<PayloadPipelineEvent>,
}

impl PayloadChannelRuntime {
    fn real(pipeline: PayloadPipeline, route_id: PayloadRouteId) -> Self {
        Self {
            pipeline: PayloadChannelPipeline::Real(Box::new(pipeline)),
            route_ids: Arc::from([route_id]),
            pipeline_events: Vec::with_capacity(1),
        }
    }

    fn mock(channel_id: ChannelId, route_id: PayloadRouteId) -> Self {
        Self {
            pipeline: PayloadChannelPipeline::Mock(MockPayloadPipeline::new(channel_id)),
            route_ids: Arc::from([route_id]),
            pipeline_events: Vec::with_capacity(1),
        }
    }

    /// Return this runtime's channel id.
    pub fn channel_id(&self) -> ChannelId {
        self.pipeline.channel_id()
    }

    /// Return the route ids attached to this runtime.
    pub fn route_ids(&self) -> &[PayloadRouteId] {
        &self.route_ids
    }

    fn push_route_id(&mut self, route_id: PayloadRouteId) {
        if self.route_ids.contains(&route_id) {
            return;
        }
        let mut route_ids = self.route_ids.to_vec();
        route_ids.push(route_id);
        self.route_ids = route_ids.into();
    }
}

/// Fanout manager for one or more OpenIPC/WFB payload routes.
///
/// The manager owns one [`PayloadPipeline`] per `(channel_id, key_slot)` and
/// lets multiple route IDs share that runtime. This is useful for outputs like
/// video display plus RTP forwarding, or video plus telemetry.
#[derive(Debug, Clone)]
pub struct PayloadRouteManager {
    frame_layout: FrameLayout,
    runtimes: HashMap<PayloadRuntimeKey, PayloadChannelRuntime>,
}

impl PayloadRouteManager {
    /// Create an empty route manager for frames with the given layout.
    pub fn new(frame_layout: FrameLayout) -> Self {
        Self {
            frame_layout,
            runtimes: HashMap::new(),
        }
    }

    /// Return the frame layout used for all registered routes.
    pub const fn frame_layout(&self) -> FrameLayout {
        self.frame_layout
    }

    /// Return the number of distinct WFB runtimes.
    pub fn runtime_count(&self) -> usize {
        self.runtimes.len()
    }

    /// Add a route that receives already-plain WFB fragments.
    ///
    /// Routes with the same channel id and key slot share one runtime.
    pub fn add_plain_route(
        &mut self,
        route_id: PayloadRouteId,
        channel_id: ChannelId,
        key_slot: u64,
        fec_k: usize,
        fec_n: usize,
    ) -> Result<PayloadRuntimeKey, PayloadRouteError> {
        let key = PayloadRuntimeKey::new(channel_id, key_slot);
        if let Some(runtime) = self.runtimes.get_mut(&key) {
            runtime.push_route_id(route_id);
            return Ok(key);
        }

        let pipeline = PayloadPipeline::new(channel_id, self.frame_layout, fec_k, fec_n)?;
        self.runtimes
            .insert(key, PayloadChannelRuntime::real(pipeline, route_id));
        Ok(key)
    }

    /// Add a route that receives encrypted WFB frames and session packets.
    ///
    /// Routes with the same channel id and key slot share one runtime.
    pub fn add_keyed_route(
        &mut self,
        route_id: PayloadRouteId,
        channel_id: ChannelId,
        key_slot: u64,
        keypair: WfbKeypair,
        minimum_epoch: u64,
    ) -> Result<PayloadRuntimeKey, PayloadRouteError> {
        let key = PayloadRuntimeKey::new(channel_id, key_slot);
        if let Some(runtime) = self.runtimes.get_mut(&key) {
            runtime.push_route_id(route_id);
            return Ok(key);
        }

        let pipeline =
            PayloadPipeline::with_keypair(channel_id, self.frame_layout, keypair, minimum_epoch)?;
        self.runtimes
            .insert(key, PayloadChannelRuntime::real(pipeline, route_id));
        Ok(key)
    }

    /// Add a direct recovered-payload route.
    ///
    /// Direct routes skip 802.11, WFB decryption, and FEC. Push payload bytes
    /// with [`Self::push_direct_payload`]; downstream route fanout and RTP
    /// handling still run exactly like radio-backed routes. This is suitable
    /// for RTP arriving from UDP as well as no-hardware tests.
    pub fn add_direct_route(
        &mut self,
        route_id: PayloadRouteId,
        channel_id: ChannelId,
        key_slot: u64,
    ) -> PayloadRuntimeKey {
        let key = PayloadRuntimeKey::new(channel_id, key_slot);
        if let Some(runtime) = self.runtimes.get_mut(&key) {
            runtime.push_route_id(route_id);
            return key;
        }

        self.runtimes
            .insert(key, PayloadChannelRuntime::mock(channel_id, route_id));
        key
    }

    /// Add a synthetic route for no-hardware tests and development.
    ///
    /// This is an alias for [`Self::add_direct_route`].
    pub fn add_mock_route(
        &mut self,
        route_id: PayloadRouteId,
        channel_id: ChannelId,
        key_slot: u64,
    ) -> PayloadRuntimeKey {
        self.add_direct_route(route_id, channel_id, key_slot)
    }

    /// Return route ids attached to a runtime key.
    pub fn route_ids(&self, key: PayloadRuntimeKey) -> Option<&[PayloadRouteId]> {
        self.runtimes
            .get(&key)
            .map(PayloadChannelRuntime::route_ids)
    }

    /// Return shared route membership and whether this is a direct runtime.
    pub(crate) fn route_membership(
        &self,
        key: PayloadRuntimeKey,
    ) -> Option<(Arc<[PayloadRouteId]>, bool)> {
        self.runtimes.get(&key).map(|runtime| {
            (
                Arc::clone(&runtime.route_ids),
                matches!(runtime.pipeline, PayloadChannelPipeline::Mock(_)),
            )
        })
    }

    /// Return cumulative FEC counters for a runtime key.
    pub fn fec_counters(&self, key: PayloadRuntimeKey) -> Option<FecCounters> {
        self.runtimes
            .get(&key)
            .map(|runtime| runtime.pipeline.fec_counters())
    }

    /// Return true when an 802.11 frame belongs to the selected runtime.
    pub fn accepts_80211_frame(&self, key: PayloadRuntimeKey, frame: &[u8]) -> bool {
        self.runtimes
            .get(&key)
            .map(|runtime| runtime.pipeline.accepts_80211_frame(frame))
            .unwrap_or(false)
    }

    /// Return true when at least one configured runtime receives `channel_id`.
    pub fn accepts_channel_id(&self, channel_id: ChannelId) -> bool {
        self.runtimes
            .keys()
            .any(|key| key.channel_id() == channel_id)
    }

    /// Route one raw 802.11 frame to every matching runtime.
    pub fn push_80211_frame(
        &mut self,
        frame: &[u8],
    ) -> Result<Vec<PayloadRouteEvent>, PayloadRouteError> {
        let Ok(frame_view) = WifiFrame::parse(frame, self.frame_layout) else {
            return Ok(vec![PayloadRouteEvent::IgnoredFrame]);
        };
        self.push_wifi_frame(frame_view)
    }

    /// Route an already-validated borrowed WiFi frame.
    pub fn push_wifi_frame(
        &mut self,
        frame_view: WifiFrame<'_>,
    ) -> Result<Vec<PayloadRouteEvent>, PayloadRouteError> {
        let mut route_events = Vec::new();
        self.push_wifi_frame_into(frame_view, &mut route_events)?;
        Ok(route_events)
    }

    /// Route a validated WiFi frame into a reusable event buffer.
    pub fn push_wifi_frame_into(
        &mut self,
        frame_view: WifiFrame<'_>,
        route_events: &mut Vec<PayloadRouteEvent>,
    ) -> Result<(), PayloadRouteError> {
        route_events.clear();
        let Some(channel_id) = frame_view.channel_id() else {
            route_events.push(PayloadRouteEvent::IgnoredFrame);
            return Ok(());
        };

        let mut matched = false;
        let mut first_error = None;

        for (key, runtime) in self
            .runtimes
            .iter_mut()
            .filter(|(key, _)| key.channel_id() == channel_id)
        {
            matched = true;
            match runtime
                .pipeline
                .push_matched_payload_into(frame_view.payload(), &mut runtime.pipeline_events)
            {
                Ok(()) => {
                    map_pipeline_events_into(
                        *key,
                        Arc::clone(&runtime.route_ids),
                        runtime.pipeline_events.drain(..),
                        route_events,
                    );
                }
                Err(err) => {
                    if first_error.is_none() {
                        first_error = Some(err);
                    }
                }
            }
        }

        if !matched {
            route_events.push(PayloadRouteEvent::IgnoredFrame);
            return Ok(());
        }
        if route_events.is_empty() {
            if let Some(err) = first_error {
                return Err(err.into());
            }
        }
        Ok(())
    }

    /// Route one 802.11 frame with a caller-supplied decrypted fragment.
    pub fn push_decrypted_80211_frame(
        &mut self,
        key: PayloadRuntimeKey,
        frame: &[u8],
        decrypted_fragment: &[u8],
    ) -> Result<Vec<PayloadRouteEvent>, PayloadRouteError> {
        let runtime = self
            .runtimes
            .get_mut(&key)
            .ok_or(PayloadRouteError::UnknownRuntime(key))?;
        let events = runtime
            .pipeline
            .push_decrypted_80211_frame(frame, decrypted_fragment)?;
        Ok(map_pipeline_events(key, runtime.route_ids(), events))
    }

    /// Push a decrypted fragment directly into one runtime.
    pub fn push_decrypted_fragment(
        &mut self,
        key: PayloadRuntimeKey,
        data_nonce: u64,
        decrypted_fragment: &[u8],
    ) -> Result<Vec<PayloadRouteEvent>, PayloadRouteError> {
        let runtime = self
            .runtimes
            .get_mut(&key)
            .ok_or(PayloadRouteError::UnknownRuntime(key))?;
        let events = runtime
            .pipeline
            .push_decrypted_fragment(data_nonce, decrypted_fragment)?;
        Ok(map_pipeline_events(key, runtime.route_ids(), events))
    }

    /// Push an already-recovered payload into one direct runtime.
    pub fn push_direct_payload(
        &mut self,
        key: PayloadRuntimeKey,
        packet_seq: u64,
        data: &[u8],
    ) -> Result<Vec<PayloadRouteEvent>, PayloadRouteError> {
        let runtime = self
            .runtimes
            .get_mut(&key)
            .ok_or(PayloadRouteError::UnknownRuntime(key))?;
        let events = runtime.pipeline.push_mock_payload(packet_seq, data);
        Ok(map_pipeline_events(key, runtime.route_ids(), events))
    }

    /// Push a synthetic recovered payload into one mock runtime.
    ///
    /// This is an alias for [`Self::push_direct_payload`].
    pub fn push_mock_payload(
        &mut self,
        key: PayloadRuntimeKey,
        packet_seq: u64,
        data: &[u8],
    ) -> Result<Vec<PayloadRouteEvent>, PayloadRouteError> {
        self.push_direct_payload(key, packet_seq, data)
    }
}

fn map_pipeline_events(
    runtime: PayloadRuntimeKey,
    route_ids: &[PayloadRouteId],
    events: Vec<PayloadPipelineEvent>,
) -> Vec<PayloadRouteEvent> {
    let mut mapped = Vec::with_capacity(events.len());
    map_pipeline_events_into(runtime, Arc::from(route_ids), events, &mut mapped);
    mapped
}

fn map_pipeline_events_into(
    runtime: PayloadRuntimeKey,
    route_ids: Arc<[PayloadRouteId]>,
    events: impl IntoIterator<Item = PayloadPipelineEvent>,
    mapped: &mut Vec<PayloadRouteEvent>,
) {
    mapped.extend(events.into_iter().map(|event| match event {
        PayloadPipelineEvent::IgnoredFrame => PayloadRouteEvent::IgnoredFrame,
        PayloadPipelineEvent::SessionEstablished {
            epoch,
            fec_k,
            fec_n,
        } => PayloadRouteEvent::SessionEstablished {
            runtime,
            route_ids: Arc::clone(&route_ids),
            epoch,
            fec_k,
            fec_n,
        },
        PayloadPipelineEvent::Payload(payload) => PayloadRouteEvent::Payload {
            runtime,
            route_ids: Arc::clone(&route_ids),
            payload,
        },
    }));
}

#[cfg(test)]
mod tests {
    use super::*;

    fn plain(payload: &[u8]) -> Vec<u8> {
        let mut out = Vec::new();
        out.push(0);
        out.extend_from_slice(&(payload.len() as u16).to_be_bytes());
        out.extend_from_slice(payload);
        out
    }

    #[test]
    fn routes_share_one_runtime_per_channel_and_key_slot() {
        let mut manager = PayloadRouteManager::new(FrameLayout::WithFcs);
        let channel = ChannelId::default_video();
        let runtime = manager
            .add_plain_route(PayloadRouteId::new(1), channel, 0, 1, 1)
            .unwrap();
        let same_runtime = manager
            .add_plain_route(PayloadRouteId::new(2), channel, 0, 1, 1)
            .unwrap();

        assert_eq!(runtime, same_runtime);
        assert_eq!(manager.runtime_count(), 1);

        let events = manager
            .push_decrypted_fragment(runtime, 0, &plain(b"rtp bytes"))
            .unwrap();
        assert_eq!(
            events,
            vec![PayloadRouteEvent::Payload {
                runtime,
                route_ids: vec![PayloadRouteId::new(1), PayloadRouteId::new(2)].into(),
                payload: RecoveredPayload {
                    channel_id: channel,
                    packet_seq: 0,
                    data: b"rtp bytes".to_vec(),
                },
            }]
        );
    }

    #[test]
    fn different_channels_get_different_runtimes() {
        let mut manager = PayloadRouteManager::new(FrameLayout::WithFcs);
        let video = ChannelId::default_video();
        let telemetry = ChannelId::from_link_port(
            crate::channel::DEFAULT_LINK_ID,
            crate::RadioPort::TelemetryRx,
        );

        let video_runtime = manager
            .add_plain_route(PayloadRouteId::new(1), video, 0, 1, 1)
            .unwrap();
        let telemetry_runtime = manager
            .add_plain_route(PayloadRouteId::new(2), telemetry, 0, 1, 1)
            .unwrap();

        assert_ne!(video_runtime, telemetry_runtime);
        assert_eq!(manager.runtime_count(), 2);
    }
}
