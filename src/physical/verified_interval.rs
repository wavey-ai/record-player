//! Finite outward-rounded interval arithmetic for admission proofs.

use thiserror::Error;

/// Reports a value that cannot form a finite verified interval.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Error)]
pub(crate) enum OutwardIntervalError {
    #[error("interval bounds must be finite")]
    NonFiniteBound,
    #[error("the interval lower bound exceeds the upper bound")]
    ReversedBounds,
    #[error("the interval operation produced an unbounded result")]
    UnboundedResult,
    #[error("the denominator interval contains zero")]
    DenominatorContainsZero,
    #[error("the square-root interval contains a negative value")]
    NegativeSquareRoot,
}

/// Contains a closed finite interval with outward-rounded bounds.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct OutwardInterval {
    lower: f64,
    upper: f64,
}

impl OutwardInterval {
    /// Makes an interval that contains one finite value.
    pub(crate) fn point(value: f64) -> Result<Self, OutwardIntervalError> {
        Self::hull(value, value)
    }

    /// Makes an interval from finite ordered bounds.
    pub(crate) fn hull(lower: f64, upper: f64) -> Result<Self, OutwardIntervalError> {
        if !lower.is_finite() || !upper.is_finite() {
            return Err(OutwardIntervalError::NonFiniteBound);
        }
        if lower > upper {
            return Err(OutwardIntervalError::ReversedBounds);
        }
        Ok(Self {
            lower: canonical_zero(lower),
            upper: canonical_zero(upper),
        })
    }

    /// Gets the inclusive lower bound.
    pub(crate) const fn lower(self) -> f64 {
        self.lower
    }

    /// Gets the inclusive upper bound.
    pub(crate) const fn upper(self) -> f64 {
        self.upper
    }

    /// Tests if this interval contains a finite value.
    pub(crate) fn contains(self, value: f64) -> bool {
        value.is_finite() && self.lower <= value && value <= self.upper
    }

    /// Tests if this interval contains another interval.
    pub(crate) fn contains_interval(self, other: Self) -> bool {
        self.lower <= other.lower && other.upper <= self.upper
    }

    /// Tests if this interval contains zero.
    pub(crate) fn contains_zero(self) -> bool {
        self.lower <= 0.0 && self.upper >= 0.0
    }

    fn is_point(self, value: f64) -> bool {
        self.lower == value && self.upper == value
    }

    /// Gets an outward upper bound for the interval width.
    pub(crate) fn width(self) -> Result<f64, OutwardIntervalError> {
        let raw = self.upper - self.lower;
        if raw == 0.0 {
            return Ok(0.0);
        }
        outward_up(raw)
    }

    /// Gets the minimum absolute value in this interval.
    pub(crate) fn minimum_absolute(self) -> f64 {
        if self.contains_zero() {
            0.0
        } else {
            self.lower.abs().min(self.upper.abs())
        }
    }

    /// Gets the maximum absolute value in this interval.
    pub(crate) fn maximum_absolute(self) -> f64 {
        self.lower.abs().max(self.upper.abs())
    }

    /// Gets the hull of this interval and another interval.
    pub(crate) fn hull_with(self, other: Self) -> Self {
        Self {
            lower: self.lower.min(other.lower),
            upper: self.upper.max(other.upper),
        }
    }

    /// Adds two intervals.
    pub(crate) fn add(self, other: Self) -> Result<Self, OutwardIntervalError> {
        if self.is_point(0.0) {
            return Ok(other);
        }
        if other.is_point(0.0) {
            return Ok(self);
        }
        let lower_raw = self.lower + other.lower;
        let upper_raw = self.upper + other.upper;
        Self::from_outward_results(lower_raw, upper_raw, lower_raw == 0.0, upper_raw == 0.0)
    }

    /// Subtracts another interval from this interval.
    pub(crate) fn subtract(self, other: Self) -> Result<Self, OutwardIntervalError> {
        if other.is_point(0.0) {
            return Ok(self);
        }
        if self.is_point(0.0) {
            return Ok(other.negate());
        }
        self.add(other.negate())
    }

    /// Negates this interval exactly.
    pub(crate) fn negate(self) -> Self {
        Self {
            lower: canonical_zero(-self.upper),
            upper: canonical_zero(-self.lower),
        }
    }

    /// Multiplies two intervals.
    pub(crate) fn multiply(self, other: Self) -> Result<Self, OutwardIntervalError> {
        if self.is_point(0.0) || other.is_point(0.0) {
            return Self::point(0.0);
        }
        if self.is_point(1.0) {
            return Ok(other);
        }
        if other.is_point(1.0) {
            return Ok(self);
        }
        if self.is_point(-1.0) {
            return Ok(other.negate());
        }
        if other.is_point(-1.0) {
            return Ok(self.negate());
        }
        let products = [
            endpoint_product(self.lower, other.lower)?,
            endpoint_product(self.lower, other.upper)?,
            endpoint_product(self.upper, other.lower)?,
            endpoint_product(self.upper, other.upper)?,
        ];
        let lower = products
            .iter()
            .map(|product| product.lower)
            .fold(f64::INFINITY, f64::min);
        let upper = products
            .iter()
            .map(|product| product.upper)
            .fold(f64::NEG_INFINITY, f64::max);
        Self::hull(lower, upper).map_err(|_| OutwardIntervalError::UnboundedResult)
    }

    /// Divides this interval by an interval that excludes zero.
    pub(crate) fn divide(self, denominator: Self) -> Result<Self, OutwardIntervalError> {
        if denominator.contains_zero() {
            return Err(OutwardIntervalError::DenominatorContainsZero);
        }
        if self.is_point(0.0) {
            return Self::point(0.0);
        }
        if denominator.is_point(1.0) {
            return Ok(self);
        }
        if denominator.is_point(-1.0) {
            return Ok(self.negate());
        }
        let quotients = [
            endpoint_quotient(self.lower, denominator.lower)?,
            endpoint_quotient(self.lower, denominator.upper)?,
            endpoint_quotient(self.upper, denominator.lower)?,
            endpoint_quotient(self.upper, denominator.upper)?,
        ];
        let lower = quotients
            .iter()
            .map(|quotient| quotient.lower)
            .fold(f64::INFINITY, f64::min);
        let upper = quotients
            .iter()
            .map(|quotient| quotient.upper)
            .fold(f64::NEG_INFINITY, f64::max);
        Self::hull(lower, upper).map_err(|_| OutwardIntervalError::UnboundedResult)
    }

    /// Squares this interval.
    pub(crate) fn square(self) -> Result<Self, OutwardIntervalError> {
        let upper_endpoint = if self.lower.abs() >= self.upper.abs() {
            self.lower
        } else {
            self.upper
        };
        let upper_raw = upper_endpoint * upper_endpoint;
        let upper = if upper_raw == 0.0 && upper_endpoint == 0.0 {
            0.0
        } else {
            outward_up(upper_raw)?
        };

        let lower = if self.contains_zero() {
            0.0
        } else {
            let lower_endpoint = if self.lower.abs() <= self.upper.abs() {
                self.lower
            } else {
                self.upper
            };
            let lower_raw = lower_endpoint * lower_endpoint;
            if lower_raw == 0.0 {
                0.0
            } else {
                outward_down(lower_raw)?.max(0.0)
            }
        };
        Self::hull(lower, upper).map_err(|_| OutwardIntervalError::UnboundedResult)
    }

    /// Computes the square root of a nonnegative interval.
    pub(crate) fn sqrt(self) -> Result<Self, OutwardIntervalError> {
        if self.lower < 0.0 {
            return Err(OutwardIntervalError::NegativeSquareRoot);
        }
        let lower_raw = self.lower.sqrt();
        let upper_raw = self.upper.sqrt();
        Self::from_outward_results(lower_raw, upper_raw, self.lower == 0.0, self.upper == 0.0)
    }

    fn from_outward_results(
        lower_raw: f64,
        upper_raw: f64,
        lower_is_exact_zero: bool,
        upper_is_exact_zero: bool,
    ) -> Result<Self, OutwardIntervalError> {
        let lower = if lower_is_exact_zero {
            0.0
        } else {
            outward_down(lower_raw)?
        };
        let upper = if upper_is_exact_zero {
            0.0
        } else {
            outward_up(upper_raw)?
        };
        Self::hull(lower, upper).map_err(|_| OutwardIntervalError::UnboundedResult)
    }
}

#[derive(Debug, Clone, Copy)]
struct OutwardEndpoint {
    lower: f64,
    upper: f64,
}

fn endpoint_product(left: f64, right: f64) -> Result<OutwardEndpoint, OutwardIntervalError> {
    let raw = left * right;
    if left == 0.0 || right == 0.0 {
        return Ok(OutwardEndpoint {
            lower: 0.0,
            upper: 0.0,
        });
    }
    Ok(OutwardEndpoint {
        lower: outward_down(raw)?,
        upper: outward_up(raw)?,
    })
}

fn endpoint_quotient(
    numerator: f64,
    denominator: f64,
) -> Result<OutwardEndpoint, OutwardIntervalError> {
    debug_assert!(denominator != 0.0);
    if numerator == 0.0 {
        return Ok(OutwardEndpoint {
            lower: 0.0,
            upper: 0.0,
        });
    }
    let raw = numerator / denominator;
    Ok(OutwardEndpoint {
        lower: outward_down(raw)?,
        upper: outward_up(raw)?,
    })
}

fn outward_down(value: f64) -> Result<f64, OutwardIntervalError> {
    if !value.is_finite() {
        return Err(OutwardIntervalError::UnboundedResult);
    }
    let result = value.next_down();
    if result.is_finite() {
        Ok(result)
    } else {
        Err(OutwardIntervalError::UnboundedResult)
    }
}

fn outward_up(value: f64) -> Result<f64, OutwardIntervalError> {
    if !value.is_finite() {
        return Err(OutwardIntervalError::UnboundedResult);
    }
    let result = value.next_up();
    if result.is_finite() {
        Ok(result)
    } else {
        Err(OutwardIntervalError::UnboundedResult)
    }
}

const fn canonical_zero(value: f64) -> f64 {
    if value == 0.0 {
        0.0
    } else {
        value
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use num_rational::BigRational;

    fn exact(value: f64) -> BigRational {
        BigRational::from_float(value).expect("the test value must be finite")
    }

    fn assert_contains_exact_range(
        interval: OutwardInterval,
        exact_lower: &BigRational,
        exact_upper: &BigRational,
    ) {
        let actual_lower = exact(interval.lower());
        let actual_upper = exact(interval.upper());
        assert!(
            actual_lower <= *exact_lower,
            "{} is above exact lower bound {exact_lower}",
            interval.lower()
        );
        assert!(
            actual_upper >= *exact_upper,
            "{} is below exact upper bound {exact_upper}",
            interval.upper()
        );
    }

    fn assert_contains_exact_value(interval: OutwardInterval, exact_value: &BigRational) {
        assert_contains_exact_range(interval, exact_value, exact_value);
    }

    fn exact_minimum(values: &[BigRational]) -> BigRational {
        values.iter().min().expect("nonempty values").clone()
    }

    fn exact_maximum(values: &[BigRational]) -> BigRational {
        values.iter().max().expect("nonempty values").clone()
    }

    fn dyadic_intervals() -> Vec<(OutwardInterval, BigRational, BigRational)> {
        let values = [-8.0, -3.5, -1.0, -0.25, 0.0, 0.125, 1.5, 6.0];
        let mut intervals = Vec::new();
        for (lower_index, lower) in values.iter().copied().enumerate() {
            for upper in values[lower_index..].iter().copied() {
                intervals.push((
                    OutwardInterval::hull(lower, upper).unwrap(),
                    exact(lower),
                    exact(upper),
                ));
            }
        }
        intervals
    }

    fn next_random_bits(state: &mut u64) -> u64 {
        *state = state.wrapping_add(0x9e37_79b9_7f4a_7c15);
        let mut value = *state;
        value = (value ^ (value >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
        value = (value ^ (value >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
        value ^ (value >> 31)
    }

    fn random_moderate_normal(state: &mut u64) -> f64 {
        const SIGN_MASK: u64 = 1_u64 << 63;
        const FRACTION_MASK: u64 = (1_u64 << 52) - 1;
        let bits = next_random_bits(state);
        let sign = bits & SIGN_MASK;
        let exponent = 991 + ((bits >> 52) % 65);
        f64::from_bits(sign | (exponent << 52) | (bits & FRACTION_MASK))
    }

    fn random_nonzero_subnormal(state: &mut u64) -> f64 {
        const SIGN_MASK: u64 = 1_u64 << 63;
        const FRACTION_MASK: u64 = (1_u64 << 52) - 1;
        let bits = next_random_bits(state);
        let sign = bits & SIGN_MASK;
        let fraction = (bits & FRACTION_MASK).max(1);
        f64::from_bits(sign | fraction)
    }

    #[test]
    fn constructors_reject_nonfinite_and_reversed_bounds() {
        assert_eq!(
            OutwardInterval::point(f64::NAN),
            Err(OutwardIntervalError::NonFiniteBound)
        );
        assert_eq!(
            OutwardInterval::hull(f64::NEG_INFINITY, 0.0),
            Err(OutwardIntervalError::NonFiniteBound)
        );
        assert_eq!(
            OutwardInterval::hull(0.0, f64::INFINITY),
            Err(OutwardIntervalError::NonFiniteBound)
        );
        assert_eq!(
            OutwardInterval::hull(2.0, 1.0),
            Err(OutwardIntervalError::ReversedBounds)
        );
    }

    #[test]
    fn constructors_canonicalize_signed_zero() {
        let interval = OutwardInterval::hull(-0.0, 0.0).unwrap();
        assert_eq!(interval.lower().to_bits(), 0.0_f64.to_bits());
        assert_eq!(interval.upper().to_bits(), 0.0_f64.to_bits());
    }

    #[test]
    fn exact_zero_survives_all_supported_operations() {
        let zero = OutwardInterval::point(0.0).unwrap();
        let one = OutwardInterval::point(1.0).unwrap();
        let negative_one = OutwardInterval::point(-1.0).unwrap();
        let nonzero = OutwardInterval::hull(2.0, 4.0).unwrap();
        let crossing = OutwardInterval::hull(-f64::MAX, f64::MAX).unwrap();

        assert_eq!(one.add(negative_one).unwrap(), zero);
        assert_eq!(one.subtract(one).unwrap(), zero);
        assert_eq!(zero.negate(), zero);
        assert_eq!(zero.multiply(crossing).unwrap(), zero);
        assert_eq!(zero.divide(nonzero).unwrap(), zero);
        assert_eq!(zero.square().unwrap(), zero);
        assert_eq!(zero.sqrt().unwrap(), zero);
    }

    #[test]
    fn exact_identities_accept_maximum_finite_bounds() {
        let zero = OutwardInterval::point(0.0).unwrap();
        let one = OutwardInterval::point(1.0).unwrap();
        for interval in [
            OutwardInterval::point(f64::MAX).unwrap(),
            OutwardInterval::point(-f64::MAX).unwrap(),
            OutwardInterval::hull(-f64::MAX, f64::MAX).unwrap(),
        ] {
            assert_eq!(interval.add(zero).unwrap(), interval);
            assert_eq!(zero.add(interval).unwrap(), interval);
            assert_eq!(interval.subtract(zero).unwrap(), interval);
            assert_eq!(interval.multiply(one).unwrap(), interval);
            assert_eq!(one.multiply(interval).unwrap(), interval);
            assert_eq!(interval.divide(one).unwrap(), interval);
        }
    }

    #[test]
    fn division_rejects_each_zero_containing_denominator_shape() {
        let numerator = OutwardInterval::point(1.0).unwrap();
        for denominator in [
            OutwardInterval::hull(-1.0, 1.0).unwrap(),
            OutwardInterval::hull(0.0, 1.0).unwrap(),
            OutwardInterval::hull(-1.0, 0.0).unwrap(),
            OutwardInterval::point(-0.0).unwrap(),
        ] {
            assert_eq!(
                numerator.divide(denominator),
                Err(OutwardIntervalError::DenominatorContainsZero)
            );
        }
    }

    #[test]
    fn operations_reject_unbounded_results() {
        let maximum = OutwardInterval::point(f64::MAX).unwrap();
        let minimum_positive = OutwardInterval::point(f64::from_bits(1)).unwrap();
        let two = OutwardInterval::point(2.0).unwrap();
        let one = OutwardInterval::point(1.0).unwrap();

        assert_eq!(
            maximum.add(maximum),
            Err(OutwardIntervalError::UnboundedResult)
        );
        assert_eq!(
            maximum.multiply(two),
            Err(OutwardIntervalError::UnboundedResult)
        );
        assert_eq!(maximum.square(), Err(OutwardIntervalError::UnboundedResult));
        assert_eq!(
            one.divide(minimum_positive),
            Err(OutwardIntervalError::UnboundedResult)
        );
        assert_eq!(
            OutwardInterval::hull(-f64::MAX, f64::MAX).unwrap().width(),
            Err(OutwardIntervalError::UnboundedResult)
        );
    }

    #[test]
    fn multiplication_encloses_subnormal_underflow() {
        let minimum_positive = f64::from_bits(1);
        let half = OutwardInterval::point(0.5).unwrap();
        let positive = OutwardInterval::point(minimum_positive)
            .unwrap()
            .multiply(half)
            .unwrap();
        let negative = OutwardInterval::point(-minimum_positive)
            .unwrap()
            .multiply(half)
            .unwrap();

        assert_eq!(positive.lower(), -minimum_positive);
        assert_eq!(positive.upper(), minimum_positive);
        assert_eq!(negative.lower(), -minimum_positive);
        assert_eq!(negative.upper(), minimum_positive);
    }

    #[test]
    fn normal_subnormal_boundaries_and_cancellation_are_enclosed() {
        let minimum_subnormal = f64::from_bits(1);
        let maximum_subnormal = f64::from_bits((1_u64 << 52) - 1);
        let minimum_normal = f64::MIN_POSITIVE;
        let next_normal = f64::from_bits(minimum_normal.to_bits() + 1);
        let two = OutwardInterval::point(2.0).unwrap();

        let boundary_sum = OutwardInterval::point(maximum_subnormal)
            .unwrap()
            .add(OutwardInterval::point(minimum_subnormal).unwrap())
            .unwrap();
        assert_contains_exact_value(boundary_sum, &exact(minimum_normal));

        let boundary_difference = OutwardInterval::point(minimum_normal)
            .unwrap()
            .subtract(OutwardInterval::point(minimum_subnormal).unwrap())
            .unwrap();
        assert_contains_exact_value(boundary_difference, &exact(maximum_subnormal));

        let half_normal = OutwardInterval::point(minimum_normal)
            .unwrap()
            .divide(two)
            .unwrap();
        assert_contains_exact_value(half_normal, &(exact(minimum_normal) / exact(2.0)));

        for value in [
            minimum_subnormal,
            maximum_subnormal,
            minimum_normal,
            next_normal,
            -minimum_subnormal,
            -maximum_subnormal,
            -minimum_normal,
            -next_normal,
        ] {
            let interval = OutwardInterval::point(value).unwrap();
            let negative = OutwardInterval::point(-value).unwrap();
            assert_eq!(
                interval.add(negative).unwrap(),
                OutwardInterval::point(0.0).unwrap()
            );
            assert_eq!(
                interval.subtract(interval).unwrap(),
                OutwardInterval::point(0.0).unwrap()
            );
        }
    }

    #[test]
    fn division_encloses_all_numerator_and_denominator_signs() {
        let cases = [
            ((-8.0, -2.0), (2.0, 4.0)),
            ((-8.0, -2.0), (-4.0, -2.0)),
            ((2.0, 8.0), (-4.0, -2.0)),
            ((2.0, 8.0), (2.0, 4.0)),
        ];
        for ((numerator_lower, numerator_upper), (denominator_lower, denominator_upper)) in cases {
            let numerator = OutwardInterval::hull(numerator_lower, numerator_upper).unwrap();
            let denominator = OutwardInterval::hull(denominator_lower, denominator_upper).unwrap();
            let exact_candidates = [
                exact(numerator_lower) / exact(denominator_lower),
                exact(numerator_lower) / exact(denominator_upper),
                exact(numerator_upper) / exact(denominator_lower),
                exact(numerator_upper) / exact(denominator_upper),
            ];
            assert_contains_exact_range(
                numerator.divide(denominator).unwrap(),
                &exact_minimum(&exact_candidates),
                &exact_maximum(&exact_candidates),
            );
        }
    }

    #[test]
    fn square_and_sqrt_keep_nonnegative_lower_bounds() {
        let minimum_positive = f64::from_bits(1);
        let squared = OutwardInterval::point(minimum_positive)
            .unwrap()
            .square()
            .unwrap();
        assert_eq!(squared.lower(), 0.0);
        assert_eq!(squared.upper(), minimum_positive);

        let square_root = OutwardInterval::point(2.0).unwrap().sqrt().unwrap();
        let exact_two = exact(2.0);
        assert!(exact(square_root.lower()).pow(2) <= exact_two);
        assert!(exact(square_root.upper()).pow(2) >= exact_two);
        assert_eq!(
            OutwardInterval::hull(-1.0, 4.0).unwrap().sqrt(),
            Err(OutwardIntervalError::NegativeSquareRoot)
        );
    }

    #[test]
    fn bounds_width_hull_and_absolute_queries_are_consistent() {
        let left = OutwardInterval::hull(-3.0, -1.0).unwrap();
        let right = OutwardInterval::hull(2.0, 5.0).unwrap();
        let hull = left.hull_with(right);

        assert!(hull.contains_interval(left));
        assert!(hull.contains_interval(right));
        assert!(hull.contains(0.0));
        assert!(!hull.contains(f64::NAN));
        assert_eq!(left.minimum_absolute(), 1.0);
        assert_eq!(hull.minimum_absolute(), 0.0);
        assert_eq!(hull.maximum_absolute(), 5.0);
        assert!(hull.width().unwrap() >= 8.0);
    }

    #[test]
    fn dyadic_oracle_exhaustively_checks_binary_operations() {
        let intervals = dyadic_intervals();
        for (left, left_lower, left_upper) in &intervals {
            for (right, right_lower, right_upper) in &intervals {
                let sum = left.add(*right).unwrap();
                assert_contains_exact_range(
                    sum,
                    &(left_lower + right_lower),
                    &(left_upper + right_upper),
                );

                let difference = left.subtract(*right).unwrap();
                assert_contains_exact_range(
                    difference,
                    &(left_lower - right_upper),
                    &(left_upper - right_lower),
                );

                let product_candidates = [
                    left_lower * right_lower,
                    left_lower * right_upper,
                    left_upper * right_lower,
                    left_upper * right_upper,
                ];
                let product = left.multiply(*right).unwrap();
                assert_contains_exact_range(
                    product,
                    &exact_minimum(&product_candidates),
                    &exact_maximum(&product_candidates),
                );

                if !right.contains_zero() {
                    let quotient_candidates = [
                        left_lower / right_lower,
                        left_lower / right_upper,
                        left_upper / right_lower,
                        left_upper / right_upper,
                    ];
                    let quotient = left.divide(*right).unwrap();
                    assert_contains_exact_range(
                        quotient,
                        &exact_minimum(&quotient_candidates),
                        &exact_maximum(&quotient_candidates),
                    );
                }
            }

            let square_candidates = [left_lower * left_lower, left_upper * left_upper];
            let exact_square_lower = if left.contains_zero() {
                exact(0.0)
            } else {
                exact_minimum(&square_candidates)
            };
            let exact_square_upper = exact_maximum(&square_candidates);
            assert_contains_exact_range(
                left.square().unwrap(),
                &exact_square_lower,
                &exact_square_upper,
            );
        }
    }

    #[test]
    fn deterministic_random_bits_match_exact_rational_oracles() {
        let mut state = 0x6a09_e667_f3bc_c909;
        for _ in 0..4_096 {
            let left_value = random_moderate_normal(&mut state);
            let right_value = random_moderate_normal(&mut state);
            let left = OutwardInterval::point(left_value).unwrap();
            let right = OutwardInterval::point(right_value).unwrap();
            let exact_left = exact(left_value);
            let exact_right = exact(right_value);

            assert_contains_exact_value(left.add(right).unwrap(), &(&exact_left + &exact_right));
            assert_contains_exact_value(
                left.subtract(right).unwrap(),
                &(&exact_left - &exact_right),
            );
            assert_contains_exact_value(
                left.multiply(right).unwrap(),
                &(&exact_left * &exact_right),
            );
            assert_contains_exact_value(left.divide(right).unwrap(), &(&exact_left / &exact_right));

            let subnormal_value = random_nonzero_subnormal(&mut state);
            let subnormal = OutwardInterval::point(subnormal_value).unwrap();
            let exact_subnormal = exact(subnormal_value);
            assert_contains_exact_value(
                subnormal.multiply(right).unwrap(),
                &(&exact_subnormal * &exact_right),
            );
            assert_contains_exact_value(
                subnormal.divide(right).unwrap(),
                &(&exact_subnormal / &exact_right),
            );
        }
    }
}
