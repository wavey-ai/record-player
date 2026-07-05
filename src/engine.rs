use crate::*;
use thiserror::Error;

#[derive(Debug, Error, PartialEq)]
pub enum PlayerError {
    #[error("operation requires ready audio")]
    AudioNotReady,
    #[error("scratch is already active")]
    ScratchAlreadyActive,
    #[error("scratch is not active")]
    ScratchNotActive,
    #[error("invalid number")]
    InvalidNumber,
}

#[derive(Debug, Clone)]
pub struct PlayerEngine {
    config: PlayerConfig,
    state: PlayerState,
    commands: Vec<HostCommand>,
    two_deck_mode: bool,
    revision: u64,
}

impl PlayerEngine {
    pub fn new(config: PlayerConfig) -> Self { Self { config, state: PlayerState::default(), commands: Vec::new(), two_deck_mode: false, revision: 0 } }
    pub fn state(&self) -> &PlayerState { &self.state }
    pub fn revision(&self) -> u64 { self.revision }
    pub fn view(&self) -> PlayerViewState { PlayerViewState::from_state(&self.state, self.revision) }
    pub fn drain_commands(&mut self) -> Vec<HostCommand> { std::mem::take(&mut self.commands) }
    fn deck(&self, id: DeckId) -> &DeckState { &self.state.decks[id.index()] }
    fn deck_mut(&mut self, id: DeckId) -> &mut DeckState { &mut self.state.decks[id.index()] }

    pub fn dispatch(&mut self, event: PlayerEvent) -> Result<(), PlayerError> {
        match event {
            PlayerEvent::HydrateDeck { deck, loaded, status, duration_seconds, current_seconds, playing, transport_on, needle_lifted, playback_rate, channel_gain } => {
                if !duration_seconds.is_finite() || !current_seconds.is_finite() || !playback_rate.is_finite() || !channel_gain.is_finite() { return Err(PlayerError::InvalidNumber); }
                let d = self.deck_mut(deck);
                d.loaded = loaded;
                d.playback.load_status = status;
                d.playback.duration_seconds = duration_seconds.max(0.0);
                d.playback.current_seconds = current_seconds.clamp(0.0, d.playback.duration_seconds.max(current_seconds));
                d.playback.playing = playing;
                d.playback.playback_rate = playback_rate;
                d.transport.motor_on = transport_on;
                d.needle.lifted = needle_lifted;
                d.mixer.channel_gain = channel_gain.clamp(0.0, 1.0);
            }
            PlayerEvent::SetActiveDeck { deck } => self.state.active_deck = deck,
            PlayerEvent::PlaybackPositionObserved { deck, seconds } => {
                if !seconds.is_finite() { return Err(PlayerError::InvalidNumber); }
                if !self.deck(deck).scratch.active && self.deck(deck).playback.suspended_at_seconds.is_none() {
                    let duration = self.deck(deck).playback.duration_seconds;
                    self.deck_mut(deck).playback.current_seconds = seconds.clamp(0.0, duration.max(seconds));
                }
            }
            PlayerEvent::PlaybackEnded { deck } => {
                let duration = self.deck(deck).playback.duration_seconds;
                let d = self.deck_mut(deck);
                d.playback.playing = false;
                d.playback.current_seconds = duration;
                self.commands.push(HostCommand::RefreshView);
            }
            PlayerEvent::SetLoadState { deck, status, loaded, duration_seconds } => {
                if !duration_seconds.is_finite() { return Err(PlayerError::InvalidNumber); }
                {
                    let d = self.deck_mut(deck);
                    d.loaded = loaded;
                    d.playback.load_status = status;
                    d.playback.duration_seconds = duration_seconds.max(0.0);
                }
                let should_start = status == LoadStatus::Ready
                    && self.deck(deck).playback.pending_play_when_ready
                    && self.deck(deck).transport.motor_on;
                if should_start {
                    let offset_seconds = self.deck(deck).playback.current_seconds;
                    let rate = self.deck(deck).playback.playback_rate;
                    let d = self.deck_mut(deck);
                    d.playback.pending_play_when_ready = false;
                    d.playback.playing = true;
                    self.commands.push(HostCommand::StartPacketPlayback { deck, offset_seconds, rate, platter_handoff: false });
                }
            }
            PlayerEvent::ToggleTransport { deck } => { let running = !self.deck(deck).transport.motor_on; self.set_transport(deck, running); }
            PlayerEvent::SetTransport { deck, running } => self.set_transport(deck, running),
            PlayerEvent::TogglePlayback { deck } => self.toggle_playback(deck)?,
            PlayerEvent::SetNeedle { deck, lifted, observed_playback_seconds } => self.set_needle(deck, lifted, observed_playback_seconds)?,
            PlayerEvent::Seek { deck, seconds } => self.seek(deck, seconds)?,
            PlayerEvent::SetPlaybackRate { deck, rate } => {
                if !rate.is_finite() { return Err(PlayerError::InvalidNumber); }
                self.deck_mut(deck).playback.playback_rate = rate;
                if self.deck(deck).playback.playing { self.commands.push(HostCommand::StartPacketPlayback { deck, offset_seconds: self.deck(deck).playback.current_seconds, rate, platter_handoff: true }); }
            }
            PlayerEvent::StartTimedRegion { region, now_ms, duration_seconds } => self.start_region(region, now_ms, duration_seconds)?,
            PlayerEvent::StopTimedRegion { region, completed } => self.stop_region(region, completed),
            PlayerEvent::TimedRegionElapsed { region } => self.finish_region(region),
            PlayerEvent::StartClipLoop { clip_id } => { self.stop_regions(); self.state.clip_loop.active = true; self.state.clip_loop.clip_id = Some(clip_id); self.commands.push(HostCommand::RefreshView); }
            PlayerEvent::StopClipLoop => self.stop_clip_loop(),
            PlayerEvent::BeginScratch { deck, pointer_id, playback_seconds, rotation_degrees } => self.begin_scratch(deck, pointer_id, playback_seconds, rotation_degrees)?,
            PlayerEvent::MoveScratch { deck, position_frames, rendered_position_frames, rate, rotation_degrees, impulse } => self.move_scratch(deck, position_frames, rendered_position_frames, rate, rotation_degrees, impulse)?,
            PlayerEvent::ScratchRenderedPosition { deck, rendered_position_frames } => { if !rendered_position_frames.is_finite() { return Err(PlayerError::InvalidNumber); } if self.deck(deck).scratch.active { self.deck_mut(deck).scratch.rendered_position_frames = rendered_position_frames.max(0.0); } },
            PlayerEvent::EndScratch { deck, rendered_position_frames, rotation_degrees, resume_playback, save_sample, can_platter_handoff } => self.end_scratch(deck, rendered_position_frames, rotation_degrees, resume_playback, save_sample, can_platter_handoff)?,
            PlayerEvent::SetCrossfader { value } => { self.state.decks[0].mixer.crossfader = value.clamp(0.0, 1.0); self.emit_mixer(); }
            PlayerEvent::SetChannelGain { deck, value } => { self.deck_mut(deck).mixer.channel_gain = value.clamp(0.0, 1.0); self.emit_mixer(); }
            PlayerEvent::SetTwoDeckMode { enabled } => { self.two_deck_mode = enabled; self.emit_mixer(); }
            PlayerEvent::Tick { now_ms } => { if !now_ms.is_finite() { return Err(PlayerError::InvalidNumber); } self.tick_regions(now_ms); }
        }
        self.revision = self.revision.wrapping_add(1);
        Ok(())
    }

    fn set_transport(&mut self, deck: DeckId, running: bool) {
        self.deck_mut(deck).transport.motor_on = running;
        self.commands.push(HostCommand::SetMotor { deck, running });
        let rate = if running { self.deck(deck).playback.playback_rate } else { 0.0 };
        self.commands.push(HostCommand::SetScratchTransport { deck, hand_contact: false, motor_rate: rate });
        if !running {
            self.deck_mut(deck).playback.playing = false;
            self.deck_mut(deck).playback.suspended_at_seconds = None;
            self.commands.push(HostCommand::StopPacketPlayback { deck, platter_handoff: true });
        }
        self.commands.push(HostCommand::RefreshView);
    }

    fn toggle_playback(&mut self, deck: DeckId) -> Result<(), PlayerError> {
        if self.state.clip_loop.active { self.stop_clip_loop(); self.set_needle(deck, true, self.deck(deck).playback.current_seconds)?; return Ok(()); }
        if self.deck(deck).scratch.active { let rendered = self.deck(deck).scratch.rendered_position_frames; let rot = self.deck(deck).scratch.base_rotation_degrees; self.end_scratch(deck, rendered, rot, false, false, false)?; self.set_needle(deck, true, self.deck(deck).playback.current_seconds)?; return Ok(()); }
        if self.state.lead_in.active { self.set_needle(deck, true, self.deck(deck).playback.current_seconds)?; self.stop_region(SurfaceRegion::LeadIn, false); self.commands.push(HostCommand::PublishTransportIntent); return Ok(()); }
        if self.state.deadwax.active { self.set_needle(deck, true, self.deck(deck).playback.current_seconds)?; self.stop_region(SurfaceRegion::Deadwax, false); self.commands.push(HostCommand::PublishTransportIntent); return Ok(()); }
        if self.deck(deck).playback.load_status != LoadStatus::Ready {
            let running = !self.deck(deck).transport.motor_on; self.set_transport(deck, running); self.deck_mut(deck).needle.lifted = true;
            if self.deck(deck).playback.load_status == LoadStatus::Loading { self.deck_mut(deck).playback.pending_play_when_ready = running; }
            return Ok(());
        }
        if self.deck(deck).playback.playing {
            self.deck_mut(deck).playback.playing = false;
            self.deck_mut(deck).playback.suspended_at_seconds = None;
            self.commands.push(HostCommand::StopPacketPlayback { deck, platter_handoff: false });
        } else {
            self.deck_mut(deck).transport.motor_on = true;
            self.deck_mut(deck).needle.lifted = false;
            self.deck_mut(deck).playback.suspended_at_seconds = None;
            self.deck_mut(deck).playback.playing = true;
            self.commands.push(HostCommand::SetMotor { deck, running: true });
            self.commands.push(HostCommand::SetPacketGain { deck, gain: 1.0, ramp_ms: 0 });
            self.commands.push(HostCommand::StartPacketPlayback { deck, offset_seconds: self.deck(deck).playback.current_seconds, rate: self.deck(deck).playback.playback_rate, platter_handoff: false });
        }
        self.commands.push(HostCommand::RefreshView); Ok(())
    }

    fn freezable(&self, deck: DeckId) -> bool { deck != DeckId::A || (!self.state.lead_in.active && !self.state.deadwax.active && !self.state.clip_loop.active && !self.deck(deck).scratch.active) }

    fn set_needle(&mut self, deck: DeckId, lifted: bool, observed: f64) -> Result<(), PlayerError> {
        if !observed.is_finite() { return Err(PlayerError::InvalidNumber); }
        if self.deck(deck).needle.lifted == lifted {
            return Ok(());
        }
        self.deck_mut(deck).needle.lifted = lifted;
        if lifted {
            if self.deck(deck).playback.playing && self.deck(deck).transport.motor_on && self.freezable(deck) {
                let t = observed.clamp(0.0, self.deck(deck).playback.duration_seconds.max(0.0));
                let d = self.deck_mut(deck); d.playback.suspended_at_seconds = Some(t); d.playback.current_seconds = t;
            }
            self.commands.push(HostCommand::SetPacketGain { deck, gain: 0.0, ramp_ms: 0 });
        } else {
            self.commands.push(HostCommand::SetPacketGain { deck, gain: 1.0, ramp_ms: 0 });
            if let Some(t) = self.deck_mut(deck).playback.suspended_at_seconds.take() {
                if self.deck(deck).transport.motor_on && self.freezable(deck) {
                    self.deck_mut(deck).playback.current_seconds = t;
                    self.commands.push(HostCommand::SetScratchPosition { deck, position_frames: t * self.config.sample_rate, impulse: 0.25 });
                    self.commands.push(HostCommand::StartPacketPlayback { deck, offset_seconds: t, rate: self.deck(deck).playback.playback_rate, platter_handoff: true });
                }
            }
        }
        self.emit_mixer(); self.commands.push(HostCommand::RefreshView); Ok(())
    }

    fn seek(&mut self, deck: DeckId, seconds: f64) -> Result<(), PlayerError> {
        if !seconds.is_finite() { return Err(PlayerError::InvalidNumber); }
        let t = seconds.clamp(0.0, self.deck(deck).playback.duration_seconds.max(0.0)); self.deck_mut(deck).playback.current_seconds = t;
        self.commands.push(HostCommand::SeekPacketPlayback { deck, offset_seconds: t });
        self.commands.push(HostCommand::SetScratchPosition { deck, position_frames: t * self.config.sample_rate, impulse: 0.0 }); Ok(())
    }

    fn start_region(&mut self, region: SurfaceRegion, now_ms: f64, duration: f64) -> Result<(), PlayerError> {
        if !now_ms.is_finite() || !duration.is_finite() || duration <= 0.0 { return Err(PlayerError::InvalidNumber); }
        let deck = DeckId::A;
        if self.deck(deck).needle.lifted || self.deck(deck).playback.load_status != LoadStatus::Ready || self.deck(deck).scratch.active || self.state.clip_loop.active { return Err(PlayerError::AudioNotReady); }
        self.stop_regions();
        let target = match region { SurfaceRegion::LeadIn => &mut self.state.lead_in, SurfaceRegion::Deadwax => &mut self.state.deadwax, SurfaceRegion::Programme => return Err(PlayerError::InvalidNumber) };
        target.active = true; target.completed = false; target.started_at_ms = now_ms; target.duration_ms = duration * 1000.0;
        self.deck_mut(deck).playback.playing = true;
        self.commands.push(HostCommand::StopPacketPlayback { deck, platter_handoff: false });
        self.commands.push(HostCommand::StartSurfaceRegion { region, duration_seconds: duration }); self.commands.push(HostCommand::RefreshView); Ok(())
    }
    fn stop_regions(&mut self) { if self.state.lead_in.active { self.stop_region(SurfaceRegion::LeadIn, false); } if self.state.deadwax.active { self.stop_region(SurfaceRegion::Deadwax, false); } }
    fn stop_region(&mut self, region: SurfaceRegion, completed: bool) { let target = match region { SurfaceRegion::LeadIn => &mut self.state.lead_in, SurfaceRegion::Deadwax => &mut self.state.deadwax, SurfaceRegion::Programme => return }; if target.active { target.active = false; target.completed = completed; self.commands.push(HostCommand::StopSurfaceRegion { region }); self.commands.push(HostCommand::RefreshView); } }
    fn finish_region(&mut self, region: SurfaceRegion) { self.stop_region(region, true); if region == SurfaceRegion::LeadIn { let t = self.deck(DeckId::A).playback.current_seconds; self.commands.push(HostCommand::StartPacketPlayback { deck: DeckId::A, offset_seconds: t, rate: self.deck(DeckId::A).playback.playback_rate, platter_handoff: true }); } else if region == SurfaceRegion::Deadwax { self.deck_mut(DeckId::A).playback.playing = false; } }
    fn stop_clip_loop(&mut self) { if self.state.clip_loop.active { self.state.clip_loop = ClipLoopState::default(); self.commands.push(HostCommand::StopClipLoop); self.commands.push(HostCommand::RefreshView); } }

    fn begin_scratch(&mut self, deck: DeckId, pointer_id: i32, seconds: f64, rotation: f64) -> Result<(), PlayerError> {
        if self.deck(deck).scratch.active { return Err(PlayerError::ScratchAlreadyActive); }
        if self.deck(deck).playback.load_status != LoadStatus::Ready { return Err(PlayerError::AudioNotReady); }
        self.stop_regions(); self.stop_clip_loop();
        let was_playing = self.deck(deck).playback.playing;
        if was_playing { self.commands.push(HostCommand::StopPacketPlayback { deck, platter_handoff: true }); }
        let frames = seconds * self.config.sample_rate;
        let d = self.deck_mut(deck); d.needle.lifted = false; d.playback.playing = false; d.playback.current_seconds = seconds; d.scratch = ScratchState { active: true, pointer_id: Some(pointer_id), was_playing, started_at_seconds: seconds, target_position_frames: frames, rendered_position_frames: frames, target_rate: 0.0, base_rotation_degrees: rotation };
        self.commands.push(HostCommand::SetPacketGain { deck, gain: 1.0, ramp_ms: 0 });
        self.commands.push(HostCommand::SetScratchTransport { deck, hand_contact: true, motor_rate: self.deck(deck).playback.playback_rate });
        self.commands.push(HostCommand::SetScratchTarget { deck, position_frames: frames, rate: 0.0, impulse: 0.0 });
        self.commands.push(HostCommand::CaptureScratchStart { deck, start_seconds: seconds, rotation_degrees: rotation }); self.commands.push(HostCommand::RefreshView); Ok(())
    }

    fn move_scratch(&mut self, deck: DeckId, position: f64, rendered: f64, rate: f32, rotation: f64, impulse: f32) -> Result<(), PlayerError> {
        if !self.deck(deck).scratch.active { return Err(PlayerError::ScratchNotActive); }
        let sample_rate = self.config.sample_rate;
        let d = self.deck_mut(deck); d.scratch.target_position_frames = position.max(0.0); d.scratch.rendered_position_frames = rendered.max(0.0); d.scratch.target_rate = rate; d.scratch.base_rotation_degrees = rotation; d.playback.current_seconds = position.max(0.0) / sample_rate;
        self.commands.push(HostCommand::SetScratchTarget { deck, position_frames: position.max(0.0), rate, impulse: impulse.clamp(0.0, 1.0) }); self.commands.push(HostCommand::RefreshView); Ok(())
    }

    fn end_scratch(&mut self, deck: DeckId, rendered: f64, rotation: f64, resume: bool, save: bool, handoff: bool) -> Result<(), PlayerError> {
        if !self.deck(deck).scratch.active { return Err(PlayerError::ScratchNotActive); }
        let was_playing = self.deck(deck).scratch.was_playing;
        let seconds = rendered.max(0.0) / self.config.sample_rate;
        self.commands.push(HostCommand::CaptureScratchFinish { deck, end_seconds: seconds, rotation_degrees: rotation, save_sample: save });
        { let d = self.deck_mut(deck); d.scratch = ScratchState::default(); d.playback.current_seconds = seconds; d.playback.playing = was_playing && resume; }
        self.commands.push(HostCommand::SetScratchTransport { deck, hand_contact: false, motor_rate: if self.deck(deck).transport.motor_on { self.deck(deck).playback.playback_rate } else { 0.0 } });
        if was_playing && resume { self.commands.push(HostCommand::StartPacketPlayback { deck, offset_seconds: seconds, rate: self.deck(deck).playback.playback_rate, platter_handoff: handoff }); }
        self.commands.push(HostCommand::RefreshView); Ok(())
    }

    fn tick_regions(&mut self, now_ms: f64) {
        let lead_elapsed = self.state.lead_in.active && now_ms >= self.state.lead_in.started_at_ms + self.state.lead_in.duration_ms;
        let dead_elapsed = self.state.deadwax.active && now_ms >= self.state.deadwax.started_at_ms + self.state.deadwax.duration_ms;
        if lead_elapsed { self.finish_region(SurfaceRegion::LeadIn); }
        if dead_elapsed { self.finish_region(SurfaceRegion::Deadwax); }
    }

    fn emit_mixer(&mut self) {
        let x = self.state.decks[0].mixer.crossfader;
        let (a, b) = sharp_crossfader_gains(x, self.config.sharp_crossfader_width);
        let a = a * self.state.decks[0].mixer.channel_gain;
        let b_needle = if self.state.decks[1].needle.lifted { 0.0 } else { 1.0 };
        let b = if self.two_deck_mode && self.state.decks[1].loaded { b * self.state.decks[1].mixer.channel_gain * b_needle } else { 0.0 };
        self.commands.push(HostCommand::SetMixerTrackGain { track: 0, gain: a, ramp_ms: 12 });
        self.commands.push(HostCommand::SetMixerTrackGain { track: 1, gain: b, ramp_ms: 12 });
    }
}

impl Default for PlayerEngine { fn default() -> Self { Self::new(PlayerConfig::default()) } }

#[cfg(test)]
mod tests {
    use super::*;
    fn ready(e: &mut PlayerEngine) { e.dispatch(PlayerEvent::SetLoadState { deck: DeckId::A, status: LoadStatus::Ready, loaded: true, duration_seconds: 180.0 }).unwrap(); e.drain_commands(); }

    #[test] fn needle_freezes_and_resumes_same_groove() {
        let mut e=PlayerEngine::default(); ready(&mut e); e.dispatch(PlayerEvent::TogglePlayback { deck: DeckId::A }).unwrap(); e.drain_commands();
        e.dispatch(PlayerEvent::SetNeedle { deck: DeckId::A, lifted: true, observed_playback_seconds: 12.25 }).unwrap();
        assert_eq!(e.state().decks[0].playback.suspended_at_seconds, Some(12.25)); e.drain_commands();
        e.dispatch(PlayerEvent::SetNeedle { deck: DeckId::A, lifted: false, observed_playback_seconds: 99.0 }).unwrap();
        assert!(e.drain_commands().iter().any(|c| matches!(c, HostCommand::StartPacketPlayback { offset_seconds, platter_handoff: true, .. } if (*offset_seconds-12.25).abs()<1e-9)));
    }
    #[test] fn needle_reassertion_is_idempotent() {
        let mut e=PlayerEngine::default(); ready(&mut e);
        e.dispatch(PlayerEvent::SetNeedle { deck: DeckId::A, lifted: true, observed_playback_seconds: 10.0 }).unwrap();
        assert!(e.drain_commands().is_empty());
        e.dispatch(PlayerEvent::TogglePlayback { deck: DeckId::A }).unwrap(); e.drain_commands();
        e.dispatch(PlayerEvent::SetNeedle { deck: DeckId::A, lifted: false, observed_playback_seconds: 0.0 }).unwrap();
        assert!(e.drain_commands().is_empty());
    }
    #[test] fn explicit_stop_clears_suspended_resume_bookmark() {
        let mut e=PlayerEngine::default(); ready(&mut e);
        e.dispatch(PlayerEvent::TogglePlayback { deck: DeckId::A }).unwrap(); e.drain_commands();
        e.dispatch(PlayerEvent::SetNeedle { deck: DeckId::A, lifted: true, observed_playback_seconds: 12.25 }).unwrap();
        assert_eq!(e.state().decks[0].playback.suspended_at_seconds, Some(12.25));
        e.drain_commands();
        e.dispatch(PlayerEvent::TogglePlayback { deck: DeckId::A }).unwrap();
        assert_eq!(e.state().decks[0].playback.suspended_at_seconds, None);
        e.drain_commands();
        e.dispatch(PlayerEvent::SetNeedle { deck: DeckId::A, lifted: false, observed_playback_seconds: 99.0 }).unwrap();
        assert!(
            !e.drain_commands().iter().any(|c| matches!(c, HostCommand::StartPacketPlayback { platter_handoff: true, .. }))
        );
    }
    #[test] fn scratch_release_uses_rendered_not_target_position() {
        let mut e=PlayerEngine::default(); ready(&mut e); e.dispatch(PlayerEvent::TogglePlayback { deck: DeckId::A }).unwrap(); e.drain_commands();
        e.dispatch(PlayerEvent::BeginScratch { deck: DeckId::A, pointer_id: 1, playback_seconds: 2.0, rotation_degrees: 40.0 }).unwrap(); e.drain_commands();
        e.dispatch(PlayerEvent::MoveScratch { deck: DeckId::A, position_frames: 200000.0, rendered_position_frames: 180000.0, rate: 1.0, rotation_degrees: 80.0, impulse: 0.0 }).unwrap(); e.drain_commands();
        e.dispatch(PlayerEvent::EndScratch { deck: DeckId::A, rendered_position_frames: 180000.0, rotation_degrees: 80.0, resume_playback: true, save_sample: true, can_platter_handoff: true }).unwrap();
        assert!(e.drain_commands().iter().any(|c| matches!(c, HostCommand::StartPacketPlayback { offset_seconds, .. } if (*offset_seconds-3.75).abs()<1e-9)));
    }
    #[test] fn sharp_crossfader_preserves_full_middle() { let (a,b)=sharp_crossfader_gains(0.5,0.08); assert_eq!((a,b),(1.0,1.0)); }
    #[test] fn scratch_interrupts_surface_region() {
        let mut e=PlayerEngine::default(); ready(&mut e); e.state.decks[0].needle.lifted=false;
        e.dispatch(PlayerEvent::StartTimedRegion { region: SurfaceRegion::LeadIn, now_ms: 1.0, duration_seconds: 2.0 }).unwrap(); e.drain_commands();
        e.dispatch(PlayerEvent::BeginScratch { deck: DeckId::A, pointer_id: 1, playback_seconds: 0.0, rotation_degrees: 0.0 }).unwrap();
        assert!(!e.state().lead_in.active);
    }
}
