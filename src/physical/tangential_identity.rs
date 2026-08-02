//! Tests provisional hard-cell identity plumbing for future tangential state.
//!
//! This module does not authorize material state. A production capability must
//! bind each trace to its source, generation, wall, and material instance.

use thiserror::Error;

use super::groove::GrooveContentIdentity;
use super::stylus::{CertifiedContactPositionInterval, StylusTraceContactSet};

/// Identifies the material-contact key schema.
pub(crate) const TANGENTIAL_CONTACT_IDENTITY_VERSION: u32 = 1;

/// Identifies one wall and one real base spline cell in a canonical groove source.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct TangentialContactIdentity {
    identity_version: u32,
    canonical_source_identity: GrooveContentIdentity,
    source_generation: u64,
    wall_index: u8,
    base_spline_cell: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Error)]
pub(crate) enum TangentialContactIdentityError {
    #[error("the canonical groove source identity is invalid")]
    InvalidCanonicalSourceIdentity,
    #[error("the canonical groove source requires at least four frames")]
    InvalidSourceFrameCount,
    #[error("45/45 wall index must be zero or one")]
    InvalidWallIndex,
    #[error("the trace does not contain a certified contact-position interval")]
    CertifiedContactPositionIntervalUnavailable,
    #[error("the contact-position interval is not a valid live tracer result")]
    InvalidCertifiedContactPositionInterval,
    #[error("the contact-position interval is outside the real source domain")]
    ContactPositionOutsideSource,
    #[error("same-wall multiple contact cannot use one tangential material identity")]
    SameWallMultipleContactUnsupported,
    #[error("the certified contact bounds do not isolate one real base spline cell")]
    TangentialContactIdentityNotIsolated,
}

/// Tests one provisional key from one live same-wall trace.
///
/// This function does not bind the caller-supplied lineage to the trace.
/// It must not authorize material state.
#[allow(dead_code)]
pub(crate) fn resolve_tangential_contact_identity(
    canonical_source_identity: GrooveContentIdentity,
    source_generation: u64,
    source_frame_count: u64,
    wall_index: usize,
    contacts: StylusTraceContactSet,
) -> Result<TangentialContactIdentity, TangentialContactIdentityError> {
    canonical_source_identity
        .validate_current()
        .map_err(|_| TangentialContactIdentityError::InvalidCanonicalSourceIdentity)?;
    if source_frame_count < 4 {
        return Err(TangentialContactIdentityError::InvalidSourceFrameCount);
    }
    let wall_index = u8::try_from(wall_index)
        .ok()
        .filter(|wall| *wall <= 1)
        .ok_or(TangentialContactIdentityError::InvalidWallIndex)?;
    if contacts.contact_count == 0 {
        return Err(TangentialContactIdentityError::CertifiedContactPositionIntervalUnavailable);
    }
    if contacts.contact_count != 1 {
        return Err(TangentialContactIdentityError::SameWallMultipleContactUnsupported);
    }
    let interval = contacts.contacts[0]
        .certified_position_interval
        .ok_or(TangentialContactIdentityError::CertifiedContactPositionIntervalUnavailable)?;
    let base_spline_cell = isolated_base_spline_cell(interval, source_frame_count)?;
    Ok(TangentialContactIdentity {
        identity_version: TANGENTIAL_CONTACT_IDENTITY_VERSION,
        canonical_source_identity,
        source_generation,
        wall_index,
        base_spline_cell,
    })
}

#[derive(Debug, Clone, Copy, PartialEq)]
#[allow(dead_code)]
struct DecomposedSourceFrame {
    floor: u64,
    fraction: f64,
}

#[allow(dead_code)]
fn isolated_base_spline_cell(
    interval: CertifiedContactPositionInterval,
    source_frame_count: u64,
) -> Result<u64, TangentialContactIdentityError> {
    let lower_relative = interval.lower_relative_source_frame();
    let upper_relative = interval.upper_relative_source_frame();
    if !interval.is_tracer_produced()
        || !lower_relative.is_finite()
        || !upper_relative.is_finite()
        || lower_relative > upper_relative
    {
        return Err(TangentialContactIdentityError::InvalidCertifiedContactPositionInterval);
    }
    let lower = decompose_source_frame(interval.source_frame_origin(), lower_relative)
        .ok_or(TangentialContactIdentityError::ContactPositionOutsideSource)?;
    let upper = decompose_source_frame(interval.source_frame_origin(), upper_relative)
        .ok_or(TangentialContactIdentityError::ContactPositionOutsideSource)?;
    let final_source_frame = source_frame_count
        .checked_sub(1)
        .ok_or(TangentialContactIdentityError::InvalidSourceFrameCount)?;
    let lower_cell = material_cell_for_bound(lower, final_source_frame)
        .ok_or(TangentialContactIdentityError::ContactPositionOutsideSource)?;
    let upper_cell = material_cell_for_bound(upper, final_source_frame)
        .ok_or(TangentialContactIdentityError::ContactPositionOutsideSource)?;
    if lower_cell != upper_cell {
        return Err(TangentialContactIdentityError::TangentialContactIdentityNotIsolated);
    }
    Ok(lower_cell)
}

#[allow(dead_code)]
fn decompose_source_frame(origin: u64, relative: f64) -> Option<DecomposedSourceFrame> {
    if !relative.is_finite() {
        return None;
    }
    let relative_floor = relative.floor();
    const TWO_TO_64: f64 = 18_446_744_073_709_551_616.0;
    let floor = if relative_floor >= 0.0 {
        if relative_floor >= TWO_TO_64 {
            return None;
        }
        origin.checked_add(relative_floor as u64)?
    } else {
        let magnitude = -relative_floor;
        if magnitude >= TWO_TO_64 {
            return None;
        }
        origin.checked_sub(magnitude as u64)?
    };
    let fraction = relative - relative_floor;
    if !(0.0..1.0).contains(&fraction) {
        return None;
    }
    Some(DecomposedSourceFrame { floor, fraction })
}

#[allow(dead_code)]
fn material_cell_for_bound(bound: DecomposedSourceFrame, final_source_frame: u64) -> Option<u64> {
    if bound.floor < final_source_frame {
        return Some(bound.floor);
    }
    if bound.floor == final_source_frame && bound.fraction == 0.0 {
        return final_source_frame.checked_sub(1);
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::physical::{StylusTraceContact, MAX_SPHERICAL_TRACE_CONTACTS_PER_WALL};

    fn contact_set(interval: CertifiedContactPositionInterval) -> StylusTraceContactSet {
        let mut contacts = [StylusTraceContact::default(); MAX_SPHERICAL_TRACE_CONTACTS_PER_WALL];
        contacts[0].certified_position_interval = Some(interval);
        StylusTraceContactSet {
            center_displacement_m: 0.0,
            contact_count: 1,
            contacts,
        }
    }

    fn identity(
        interval: CertifiedContactPositionInterval,
        source_frame_count: u64,
    ) -> Result<TangentialContactIdentity, TangentialContactIdentityError> {
        resolve_tangential_contact_identity(
            GrooveContentIdentity::from_sha256([0x51; 32]),
            73,
            source_frame_count,
            1,
            contact_set(interval),
        )
    }

    #[test]
    fn closed_bounds_must_isolate_one_half_open_cell() {
        let interior = CertifiedContactPositionInterval::from_closed_relative_bounds(
            0,
            9.25,
            9.25_f64.next_up(),
        )
        .unwrap();
        assert_eq!(identity(interior, 20).unwrap().base_spline_cell, 9);

        let exact_join =
            CertifiedContactPositionInterval::from_closed_relative_bounds(0, 10.0, 10.0).unwrap();
        assert_eq!(identity(exact_join, 20).unwrap().base_spline_cell, 10);

        let straddled_join = CertifiedContactPositionInterval::from_closed_relative_bounds(
            0,
            10.0_f64.next_down(),
            10.0,
        )
        .unwrap();
        assert_eq!(
            identity(straddled_join, 20),
            Err(TangentialContactIdentityError::TangentialContactIdentityNotIsolated)
        );
    }

    #[test]
    fn final_coordinate_belongs_to_the_last_real_spline_cell() {
        let endpoint =
            CertifiedContactPositionInterval::from_closed_relative_bounds(0, 19.0, 19.0).unwrap();
        assert_eq!(identity(endpoint, 20).unwrap().base_spline_cell, 18);

        let outside = CertifiedContactPositionInterval::from_closed_relative_bounds(
            0,
            19.0,
            19.0_f64.next_up(),
        )
        .unwrap();
        assert_eq!(
            identity(outside, 20),
            Err(TangentialContactIdentityError::ContactPositionOutsideSource)
        );
    }

    #[test]
    fn outward_record_edge_extensions_remain_an_activation_blocker() {
        let samples = [0.0_f32; 20];
        for center in [0.0, 19.0] {
            let traced = crate::physical::trace_spherical_uniform_contacts(
                &samples,
                center,
                2.0e-6,
                crate::physical::StylusGeometry::default(),
            )
            .unwrap();
            assert_eq!(
                resolve_tangential_contact_identity(
                    GrooveContentIdentity::from_sha256([0x51; 32]),
                    73,
                    samples.len() as u64,
                    0,
                    traced,
                ),
                Err(TangentialContactIdentityError::ContactPositionOutsideSource)
            );
        }
    }

    #[test]
    fn bounds_before_or_after_the_source_reject_transactionally() {
        let before =
            CertifiedContactPositionInterval::from_closed_relative_bounds(0, -0.25, -0.125)
                .unwrap();
        assert_eq!(
            identity(before, 20),
            Err(TangentialContactIdentityError::ContactPositionOutsideSource)
        );

        let after =
            CertifiedContactPositionInterval::from_closed_relative_bounds(0, 19.25, 19.5).unwrap();
        assert_eq!(
            identity(after, 20),
            Err(TangentialContactIdentityError::ContactPositionOutsideSource)
        );
    }

    #[test]
    fn signed_local_bounds_keep_large_origins_exact() {
        let origin = u64::MAX - 32;
        let interval = CertifiedContactPositionInterval::from_closed_relative_bounds(
            origin,
            8.25,
            8.25_f64.next_up(),
        )
        .unwrap();
        assert_eq!(
            identity(interval, u64::MAX).unwrap().base_spline_cell,
            origin + 8
        );

        let before_origin =
            CertifiedContactPositionInterval::from_closed_relative_bounds(origin, -2.75, -2.5)
                .unwrap();
        assert_eq!(
            identity(before_origin, u64::MAX).unwrap().base_spline_cell,
            origin - 3
        );
    }

    #[test]
    fn absent_or_multiple_contacts_reject_transactionally() {
        let interval =
            CertifiedContactPositionInterval::from_closed_relative_bounds(0, 4.25, 4.5).unwrap();
        let mut absent = contact_set(interval);
        absent.contacts[0].certified_position_interval = None;
        assert_eq!(
            resolve_tangential_contact_identity(
                GrooveContentIdentity::from_sha256([0x51; 32]),
                73,
                20,
                0,
                absent,
            ),
            Err(TangentialContactIdentityError::CertifiedContactPositionIntervalUnavailable)
        );

        let mut multiple = contact_set(interval);
        multiple.contact_count = 2;
        multiple.contacts[1].certified_position_interval = Some(interval);
        assert_eq!(
            resolve_tangential_contact_identity(
                GrooveContentIdentity::from_sha256([0x51; 32]),
                73,
                20,
                0,
                multiple,
            ),
            Err(TangentialContactIdentityError::SameWallMultipleContactUnsupported)
        );
    }

    #[test]
    fn deserialized_reversed_bounds_reject_transactionally() {
        let interval: CertifiedContactPositionInterval =
            serde_json::from_value(serde_json::json!({
                "sourceFrameOrigin": 0,
                "lowerRelativeSourceFrame": 9.9,
                "upperRelativeSourceFrame": 9.1
            }))
            .unwrap();
        assert_eq!(
            identity(interval, 20),
            Err(TangentialContactIdentityError::InvalidCertifiedContactPositionInterval)
        );
    }

    #[test]
    fn deserialized_bounds_cannot_claim_tracer_certification() {
        let interval: CertifiedContactPositionInterval =
            serde_json::from_value(serde_json::json!({
                "sourceFrameOrigin": 0,
                "lowerRelativeSourceFrame": 9.1,
                "upperRelativeSourceFrame": 9.2
            }))
            .unwrap();
        assert_eq!(
            identity(interval, 20),
            Err(TangentialContactIdentityError::InvalidCertifiedContactPositionInterval)
        );
    }

    #[test]
    fn serialization_preserves_bounds_but_not_the_live_trace_seal() {
        let live =
            CertifiedContactPositionInterval::from_closed_relative_bounds(0, 9.1, 9.2).unwrap();
        let encoded = serde_json::to_string(&live).unwrap();
        let decoded: CertifiedContactPositionInterval = serde_json::from_str(&encoded).unwrap();
        assert_eq!(decoded, live);
        assert_eq!(decoded.source_frame_origin(), live.source_frame_origin());
        assert_eq!(
            decoded.lower_relative_source_frame(),
            live.lower_relative_source_frame()
        );
        assert_eq!(
            decoded.upper_relative_source_frame(),
            live.upper_relative_source_frame()
        );
        assert!(live.is_tracer_produced());
        assert!(!decoded.is_tracer_produced());
        assert_eq!(
            identity(decoded, 20),
            Err(TangentialContactIdentityError::InvalidCertifiedContactPositionInterval)
        );
    }

    #[test]
    fn resolver_is_deterministic_and_does_not_allocate() {
        let interval =
            CertifiedContactPositionInterval::from_closed_relative_bounds(0, 4.25, 4.5).unwrap();
        let contacts = contact_set(interval);
        let mut result = None;
        assert_no_alloc::assert_no_alloc(|| {
            result = Some(resolve_tangential_contact_identity(
                GrooveContentIdentity::from_sha256([0x51; 32]),
                73,
                20,
                1,
                contacts,
            ));
        });
        let first = result.unwrap().unwrap();
        let second = identity(interval, 20).unwrap();
        assert_eq!(first, second);
        assert_eq!(first.identity_version, TANGENTIAL_CONTACT_IDENTITY_VERSION);
    }

    #[test]
    fn every_declared_key_component_changes_the_identity() {
        let first_interval =
            CertifiedContactPositionInterval::from_closed_relative_bounds(0, 4.25, 4.5).unwrap();
        let next_interval =
            CertifiedContactPositionInterval::from_closed_relative_bounds(0, 5.25, 5.5).unwrap();
        let source = GrooveContentIdentity::from_sha256([0x51; 32]);
        let contacts = contact_set(first_interval);
        let reference = resolve_tangential_contact_identity(source, 73, 20, 0, contacts).unwrap();
        assert_ne!(
            resolve_tangential_contact_identity(
                GrooveContentIdentity::from_sha256([0x52; 32]),
                73,
                20,
                0,
                contacts,
            )
            .unwrap(),
            reference
        );
        assert_ne!(
            resolve_tangential_contact_identity(source, 74, 20, 0, contacts).unwrap(),
            reference
        );
        assert_ne!(
            resolve_tangential_contact_identity(source, 73, 20, 1, contacts).unwrap(),
            reference
        );
        assert_ne!(
            resolve_tangential_contact_identity(source, 73, 20, 0, contact_set(next_interval),)
                .unwrap(),
            reference
        );
    }
}
