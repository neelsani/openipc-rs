use crate::types::RadioConfig;

#[derive(Debug, Clone, Default)]
pub(crate) struct Jaguar1RetuneState {
    pub rf18: [Option<u32>; 4],
    pub last_fc: Option<u32>,
    pub last_spur_class: Option<u8>,
    pub last_subchannel: Option<u8>,
}

#[derive(Debug, Clone, Default)]
pub(crate) struct Jaguar2RetuneState {
    pub rf18: Option<u32>,
    pub compose_primed: bool,
    pub compose_agc: u32,
    pub compose_fc: u32,
    pub compose_rf_be: u32,
    pub last_agc_bucket: Option<u8>,
    pub last_fc: Option<u32>,
    pub last_rf_be: Option<u8>,
    pub last_df18: Option<bool>,
    pub last_cck_key: Option<bool>,
}

#[derive(Debug, Clone, Default)]
pub(crate) struct Jaguar3RetuneState {
    pub compose_primed: bool,
    pub compose_1c90: u32,
    pub compose_1830: u32,
    pub compose_4130: u32,
    pub compose_r0: u32,
    pub compose_c30: u32,
    pub compose_808: u32,
    pub compose_rfwin_a: u32,
    pub compose_rfwin_b: u32,
    pub last_sco: Option<u32>,
    pub last_dfir: Option<u32>,
    pub last_agc_key: Option<u8>,
    pub rxbb_asserted: bool,
}

#[derive(Debug, Clone, Default)]
pub(crate) struct FastRetuneState {
    pub radio: Option<RadioConfig>,
    pub jaguar1: Jaguar1RetuneState,
    pub jaguar2: Jaguar2RetuneState,
    pub jaguar3: Jaguar3RetuneState,
}

impl FastRetuneState {
    pub fn note_full_tune(&mut self, radio: RadioConfig) {
        self.radio = Some(radio);
        self.jaguar1 = Jaguar1RetuneState::default();
        self.jaguar2 = Jaguar2RetuneState::default();
        self.jaguar3 = Jaguar3RetuneState::default();
    }

    pub fn invalidate_jaguar3(&mut self) {
        self.jaguar3 = Jaguar3RetuneState::default();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_full_tune_invalidates_every_generation_cache() {
        let mut state = FastRetuneState::default();
        state.jaguar1.rf18[0] = Some(1);
        state.jaguar2.compose_primed = true;
        state.jaguar3.compose_primed = true;
        let radio = RadioConfig::default();

        state.note_full_tune(radio);

        assert_eq!(state.radio, Some(radio));
        assert_eq!(state.jaguar1.rf18, [None; 4]);
        assert!(!state.jaguar2.compose_primed);
        assert!(!state.jaguar3.compose_primed);
    }

    #[test]
    fn jaguar3_invalidation_preserves_the_tracked_radio() {
        let radio = RadioConfig::default();
        let mut state = FastRetuneState {
            radio: Some(radio),
            ..FastRetuneState::default()
        };
        state.jaguar3.compose_primed = true;

        state.invalidate_jaguar3();

        assert_eq!(state.radio, Some(radio));
        assert!(!state.jaguar3.compose_primed);
    }
}
