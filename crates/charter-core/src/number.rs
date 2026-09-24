use std::fmt;

/// Most millionths a decimal holds: fifteen significant digits, few enough to survive a trip
/// through a double.
pub(crate) const MAX_MILLIONTHS: i64 = 999_999_999_999_999;

/// The whole millionths `value` holds, if it has at most six places and fifteen significant
/// digits.
pub(crate) fn to_millionths(value: f64) -> Option<i64> {
    let scaled = (value * 1e6).round_ties_even();
    #[allow(clippy::cast_precision_loss)] // below 2^53, so exact
    if scaled.is_nan() || scaled.abs() > MAX_MILLIONTHS as f64 {
        return None;
    }
    #[allow(clippy::cast_possible_truncation)] // whole and within range
    let millionths = scaled as i64;
    #[allow(clippy::float_cmp)] // exactness is the point
    (from_millionths(millionths) == value).then_some(millionths)
}

/// The double nearest to `millionths` millionths.
pub(crate) fn from_millionths(millionths: i64) -> f64 {
    #[allow(clippy::cast_precision_loss)] // below 2^53, so exact
    let whole = millionths as f64;
    whole / 1e6
}

/// Writes `millionths` as the shortest decimal text for it: no exponent, no trailing zeros.
pub(crate) fn write_millionths(f: &mut fmt::Formatter<'_>, millionths: i64) -> fmt::Result {
    let sign = if millionths < 0 { "-" } else { "" };
    let magnitude = millionths.unsigned_abs();
    let (whole, fraction) = (magnitude / 1_000_000, magnitude % 1_000_000);
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
            assert!(decimal(bad).is_err(), "{bad} accepted");
        }
    }

    #[test]
    fn decimals_read_from_json_numbers() {
        let read: Decimal = serde_json::from_value(serde_json::json!(-0.014)).unwrap();
        assert_eq!(read, decimal(-0.014).unwrap());
        assert!(serde_json::from_value::<Decimal>(serde_json::json!(0.123_456_7)).is_err());
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
}
