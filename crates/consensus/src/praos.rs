use crate::ConsensusError;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ActiveSlotCoeff(pub f64);

pub fn leadership_threshold(
    active_slot_coeff: ActiveSlotCoeff,
    sigma: f64,
) -> Result<f64, ConsensusError> {
    if !(0.0..=1.0).contains(&active_slot_coeff.0) {
        return Err(ConsensusError::InvalidActiveSlotCoeff);
    }

    Ok(1.0 - (1.0 - active_slot_coeff.0).powf(sigma))
}
