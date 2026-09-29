use std::fmt;

/// Most millionths a decimal holds: fifteen significant digits, few enough to survive a trip
/// through a double.
pub(crate) const MAX_MILLIONTHS: i64 = 999_999_999_999_999;

/// The whole millionths `value` holds, if it has at most six places and fifteen significant
/// digits.
pub(crate) fn to_millionths(value: f64) -> Option<i64> {
    let scaled = (value * 1e6).round_ties_even();
    if scaled.is_nan() || scaled.abs() > i64_to_f64(MAX_MILLIONTHS) {
        return None;
    }
    let millionths = bounded_whole_to_i64(scaled);
    #[expect(clippy::float_cmp, reason = "exactness is the point")]
    (from_millionths(millionths) == value).then_some(millionths)
}

/// The double nearest to `millionths` millionths.
pub(crate) fn from_millionths(millionths: i64) -> f64 {
    let whole = i64_to_f64(millionths);
    whole / 1e6
}

#[expect(
    clippy::as_conversions,
    clippy::cast_precision_loss,
    reason = "millionths stay below 2^53"
)]
/// A millionths count as a double; callers keep it below the exact integer limit.
fn i64_to_f64(value: i64) -> f64 {
    value as f64
}

#[expect(
    clippy::as_conversions,
    clippy::cast_possible_truncation,
    reason = "caller bounds a whole number to millionths"
)]
/// Whole scaled decimal already checked against the millionths bound.
fn bounded_whole_to_i64(value: f64) -> i64 {
    value as i64
}

/// Writes `millionths` as the shortest decimal text for it: no exponent, no trailing zeros.
pub(crate) fn write_millionths(f: &mut fmt::Formatter<'_>, millionths: i64) -> fmt::Result {
    let sign = if millionths < 0 { "-" } else { "" };
    let magnitude = millionths.unsigned_abs();
    let (whole, fraction) = (
        magnitude.div_euclid(1_000_000),
        magnitude.rem_euclid(1_000_000),
    );
    if fraction == 0 {
        return write!(f, "{sign}{whole}");
    }
    let places = format!("{fraction:06}");
    write!(f, "{sign}{whole}.{}", places.trim_end_matches('0'))
}

/// Defines a type holding a decimal's whole millionths, written as an ordinary JSON number.
macro_rules! decimal {
    ($(#[$meta:meta])* $name:ident, $expected:literal, |$m:ident| $accepts:expr) => {
        $(#[$meta])*
        #[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, ::serde::Deserialize)]
        #[serde(try_from = "f64")]
        pub struct $name(i64);

        impl TryFrom<f64> for $name {
            type Error = $crate::error::InvalidValue;

            fn try_from(value: f64) -> Result<Self, Self::Error> {
                $crate::number::to_millionths(value)
                    .filter(|&$m| $accepts)
                    .map(Self)
                    .ok_or_else(|| $crate::error::InvalidValue::new($expected, value))
            }
        }

        impl ::std::fmt::Display for $name {
            fn fmt(&self, f: &mut ::std::fmt::Formatter<'_>) -> ::std::fmt::Result {
                $crate::number::write_millionths(f, self.0)
            }
        }

        impl From<$name> for f64 {
            fn from(value: $name) -> Self {
                $crate::number::from_millionths(value.0)
            }
        }

        impl ::serde::Serialize for $name {
            fn serialize<S: ::serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
                serializer.serialize_f64($crate::number::from_millionths(self.0))
            }
        }
    };
}

pub(crate) use decimal;

decimal!(
    /// A number of at most six decimal places and fifteen significant digits, held as whole
    /// millionths so it is exact.
    Decimal, "a decimal of at most six places and fifteen digits", |_m| true
);

impl Decimal {
    /// The decimal of `millionths` whole millionths, if it is within range. For constants.
    #[must_use]
    pub const fn from_millionths(millionths: i64) -> Option<Self> {
        if -MAX_MILLIONTHS <= millionths && millionths <= MAX_MILLIONTHS {
            Some(Self(millionths))
        } else {
            None
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::error::InvalidValue;

    fn decimal(value: f64) -> Result<Decimal, InvalidValue> {
        Decimal::try_from(value)
    }

    #[test]
    fn a_decimal_keeps_six_places_exactly() {
        for value in [
            -0.014,
            120.5,
            0.000_001,
            999_999_999.999_999,
            -999_999_999.999_999,
        ] {
            let written = serde_json::to_value(decimal(value).unwrap()).unwrap();
            assert_eq!(written, serde_json::json!(value));
        }
    }

    #[test]
    fn a_seventh_place_or_a_sixteenth_digit_is_refused() {
        for bad in [0.123_456_7, 1e9, -1e9, f64::NAN, f64::INFINITY] {
            decimal(bad).unwrap_err();
        }
    }

    #[test]
    fn decimals_read_from_json_numbers() {
        let read: Decimal = serde_json::from_value(serde_json::json!(-0.014)).unwrap();
        assert_eq!(read, decimal(-0.014).unwrap());
        serde_json::from_value::<Decimal>(serde_json::json!(0.123_456_7)).unwrap_err();
    }

    #[test]
    fn every_count_of_millionths_reads_back_as_itself() {
        let spread = (-MAX_MILLIONTHS..=MAX_MILLIONTHS).step_by(9_999_999_991);
        for millionths in spread.chain(-100_000..=100_000) {
            assert_eq!(to_millionths(from_millionths(millionths)), Some(millionths));
        }
    }

    #[test]
    fn decimals_show_as_their_shortest_decimal_text() {
        for (value, text) in [
            (-0.014, "-0.014"),
            (120.5, "120.5"),
            (120.0, "120"),
            (0.000_001, "0.000001"),
            (-999_999_999.999_999, "-999999999.999999"),
        ] {
            assert_eq!(decimal(value).unwrap().to_string(), text);
        }
    }

    #[test]
    fn constants_are_built_from_whole_millionths_within_range() {
        const FIVE: Option<Decimal> = Decimal::from_millionths(5_000_000);
        assert_eq!(FIVE, decimal(5.0).ok());
        assert_eq!(Decimal::from_millionths(MAX_MILLIONTHS + 1), None);
        assert_eq!(Decimal::from_millionths(-MAX_MILLIONTHS - 1), None);
    }

    #[test]
    fn a_decimal_reads_back_as_the_number_it_holds() {
        assert!((f64::from(crate::Bpm::try_from(150.5).unwrap()) - 150.5).abs() < f64::EPSILON);
        assert!((f64::from(decimal(-0.25).unwrap()) + 0.25).abs() < f64::EPSILON);
    }

    #[test]
    fn negative_zero_serializes_as_zero_and_nonfinite_numbers_are_refused() {
        let zero = decimal(-0.0).unwrap();
        assert_eq!(zero.to_string(), "0");
        assert_eq!(serde_json::to_string(&zero).unwrap(), "0.0");
        for bad in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
            decimal(bad).unwrap_err();
        }
    }

    #[test]
    fn rounding_does_not_silently_accept_a_seventh_decimal_place() {
        for bad in [0.000_000_5, -0.000_000_5, 0.000_001_4, -0.000_001_4] {
            decimal(bad).unwrap_err();
        }
        assert_eq!(decimal(0.000_001).unwrap().to_string(), "0.000001");
    }

    proptest::proptest! {
        #![proptest_config(crate::proptest_config())]

        #[test]
        fn every_representable_millionth_survives_a_round_trip(
            millionths in -MAX_MILLIONTHS..=MAX_MILLIONTHS
        ) {
            let decimal = Decimal::from_millionths(millionths).unwrap();
            let decoded: Decimal = serde_json::from_str(&serde_json::to_string(&decimal).unwrap()).unwrap();
            proptest::prop_assert_eq!(decoded, decimal);
        }
    }
}

/// Proofs for Kani, run with `cargo kani -p charter-core`. The round trip from millionths to a
/// double and back is tested instead: CBMC did not finish it in 15 minutes.
#[cfg(kani)]
mod proofs {
    use super::*;

    #[kani::proof]
    fn no_double_reads_as_a_count_out_of_range() {
        let value: f64 = kani::any();
        if let Some(millionths) = to_millionths(value) {
            assert!((-MAX_MILLIONTHS..=MAX_MILLIONTHS).contains(&millionths));
        }
    }

    #[kani::proof]
    fn decimal_construction_accepts_exactly_the_bounded_millionths() {
        let millionths: i64 = kani::any();
        let parsed = Decimal::from_millionths(millionths);
        assert!(parsed.is_some() == (-MAX_MILLIONTHS..=MAX_MILLIONTHS).contains(&millionths));
        if let Some(decimal) = parsed {
            assert!(decimal.0 == millionths);
        }
    }
}
