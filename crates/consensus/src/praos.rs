use crate::ConsensusError;

/// The Praos active slot coefficient, expected to be in the inclusive range `[0, 1]`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ActiveSlotCoeff(pub f64);

/// Computes the Praos leadership threshold $\phi_f(\sigma) = 1 - (1 - f)^\sigma$.
pub fn leadership_threshold(
    active_slot_coeff: ActiveSlotCoeff,
    sigma: f64,
) -> Result<f64, ConsensusError> {
    if !(0.0..=1.0).contains(&active_slot_coeff.0) {
        return Err(ConsensusError::InvalidActiveSlotCoeff);
    }

    Ok(1.0 - (1.0 - active_slot_coeff.0).powf(sigma))
}
