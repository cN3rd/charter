use std::fmt;
use std::str::FromStr;

use crate::error::InvalidValue;

/// Defines a validated string type with parsing, display and serde support.
macro_rules! string_newtype {
    ($(#[$meta:meta])* $name:ident, $expected:literal, |$v:ident| $accepts:expr) => {
        $(#[$meta])*
        #[derive(
            Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, ::serde::Serialize, ::serde::Deserialize,
        )]
        #[serde(try_from = "String")]
        pub struct $name(String);

        impl TryFrom<String> for $name {
            type Error = $crate::error::InvalidValue;

            fn try_from(value: String) -> Result<Self, Self::Error> {
                Self::new(value)
            }
        }

        impl $name {
            /// # Errors
            ///
            /// Returns [`InvalidValue`](crate::InvalidValue) if `value` breaks this type's grammar.
            pub fn new(value: impl Into<String>) -> Result<Self, $crate::error::InvalidValue> {
                let $v: String = value.into();
                if $accepts {
                    Ok(Self($v))
                } else {
                    Err($crate::error::InvalidValue::new($expected, format_args!("{:?}", $v)))
                }
            }

            #[must_use]
            pub fn as_str(&self) -> &str {
                &self.0
            }
        }

        impl ::std::fmt::Display for $name {
            fn fmt(&self, f: &mut ::std::fmt::Formatter<'_>) -> ::std::fmt::Result {
                f.write_str(&self.0)
            }
        }

        impl ::std::borrow::Borrow<str> for $name {
            fn borrow(&self) -> &str {
                &self.0
            }
        }
    };
}

pub(crate) use string_newtype;

string_newtype!(
    /// A plugin's namespace: `[a-z][a-z0-9_]*`.
    Namespace, "a namespace matching [a-z][a-z0-9_]*", |v| is_namespace(&v)
);
string_newtype!(
    /// The part of a key after its namespace: any non-empty text, dots included.
    Name, "a non-empty name", |v| !v.is_empty()
);

/// An extension key or an `other` timing name: a namespace and a name, written
/// `<namespace>.<name>` and split at the first dot.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, ::serde::Deserialize)]
#[serde(try_from = "String")]
pub struct Key {
    /// Plugin owning the key.
    namespace: Namespace,
    /// Non-empty part following the first dot; may itself contain dots.
    name: Name,
}
impl Key {
    #[must_use]
    pub const fn new(namespace: Namespace, name: Name) -> Self {
        Self { namespace, name }
    }

    /// The owning plugin's namespace.
    #[must_use]
    pub const fn namespace(&self) -> &Namespace {
        &self.namespace
    }

    #[must_use]
    pub const fn name(&self) -> &Name {
        &self.name
    }
}

impl FromStr for Key {
    type Err = InvalidValue;

    fn from_str(text: &str) -> Result<Self, InvalidValue> {
        split(text)
            .map(|(namespace, name)| Self {
                namespace: Namespace(namespace.to_owned()),
                name: Name(name.to_owned()),
            })
            .ok_or_else(|| InvalidValue::new("<namespace>.<name>", format_args!("{text:?}")))
    }
}

impl TryFrom<String> for Key {
    type Error = InvalidValue;

    fn try_from(text: String) -> Result<Self, InvalidValue> {
        text.parse()
    }
}

impl fmt::Display for Key {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}.{}", self.namespace.as_str(), self.name.as_str())
    }
}

impl ::serde::Serialize for Key {
    fn serialize<S: ::serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.collect_str(self)
    }
}

/// Defines an enum of a closed shared vocabulary plus `Other` for a plugin's own words, read and
/// written as text.
macro_rules! vocabulary {
    ($(#[$meta:meta])* $name:ident, $expected:literal, { $($variant:ident => $word:literal),+ $(,)? }) => {
        $(#[$meta])*
        #[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, ::serde::Deserialize)]
        #[serde(try_from = "String")]
        pub enum $name {
            $($variant,)+
            /// A plugin's own word, under its namespace.
            Other(Key),
        }

        impl $name {
            /// The owning plugin's namespace, or `None` for a shared word.
            #[must_use]
            pub const fn namespace(&self) -> Option<&Namespace> {
                match self {
                    Self::Other(key) => Some(key.namespace()),
                    $(Self::$variant)|+ => None,
                }
            }
        }

        impl FromStr for $name {
            type Err = InvalidValue;

            fn from_str(text: &str) -> Result<Self, InvalidValue> {
                match text {
                    $($word => Ok(Self::$variant),)+
                    _ => text
                        .parse()
                        .map(Self::Other)
                        .map_err(|_| InvalidValue::new($expected, format_args!("{text:?}"))),
                }
            }
        }

        impl TryFrom<String> for $name {
            type Error = InvalidValue;

            fn try_from(text: String) -> Result<Self, InvalidValue> {
                text.parse()
            }
        }

        impl fmt::Display for $name {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                match self {
                    $(Self::$variant => f.write_str($word),)+
                    Self::Other(key) => key.fmt(f),
                }
            }
        }

        impl ::serde::Serialize for $name {
            fn serialize<S: ::serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
                serializer.collect_str(self)
            }
        }
    };
}

vocabulary!(
    /// A note kind: a shared word, or a plugin's own under its namespace.
    Kind, "a shared kind or <namespace>.<name>", {
        Tap => "tap",
        Hold => "hold",
        Roll => "roll",
        Mine => "mine",
        Lift => "lift",
        Fake => "fake",
    }
);

vocabulary!(
    /// An easing: a shared word, or a plugin's own under its namespace.
    Ease, "a shared easing or <namespace>.<name>", {
        Linear => "linear",
        Step => "step",
        In => "in",
        Out => "out",
        InOut => "in_out",
    }
);

vocabulary!(
    /// A marker kind: a shared word, or a plugin's own under its namespace.
    MarkerKind, "a shared marker kind or <namespace>.<name>", {
        Section => "section",
        Bookmark => "bookmark",
    }
);

/// Splits `<namespace>.<name>` at the first dot, if `value` has that shape.
fn split(value: &str) -> Option<(&str, &str)> {
    value
        .split_once('.')
        .filter(|(namespace, name)| is_namespace(namespace) && !name.is_empty())
}

/// Whether `value` matches the lowercase ASCII namespace grammar.
fn is_namespace(value: &str) -> bool {
    let mut chars = value.chars();
    chars.next().is_some_and(|c| c.is_ascii_lowercase())
        && chars.all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_')
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn namespaces_are_lowercase_identifiers() {
        for good in ["osu_mania", "sm", "a1"] {
            assert!(Namespace::new(good).is_ok(), "{good:?} rejected");
        }
        for bad in ["", "Osu", "9k", "_x", "a.b", "a-b"] {
            assert!(Namespace::new(bad).is_err(), "{bad:?} accepted");
        }
    }

    #[test]
    fn keys_are_prefixed_names_split_at_the_first_dot() {
        let key: Key = "arcaea.timing.group".parse().unwrap();
        assert_eq!(key.namespace().as_str(), "arcaea");
        for bad in ["tap", "", ".x", "x.", "Osu.x", "9k.x"] {
            assert!(bad.parse::<Key>().is_err(), "{bad:?} accepted");
        }
    }

    #[test]
    fn kinds_are_shared_words_or_prefixed_names() {
        assert_eq!("tap".parse::<Kind>(), Ok(Kind::Tap));
        let flick: Kind = "psk.flick".parse().unwrap();
        assert_eq!(flick.namespace().map(Namespace::as_str), Some("psk"));
        for bad in ["slide", "linear", "Tap", ".x"] {
            assert!(bad.parse::<Kind>().is_err(), "{bad:?} accepted");
        }
    }

    #[test]
    fn easings_are_shared_words_or_prefixed_names() {
        assert_eq!("in_out".parse::<Ease>(), Ok(Ease::InOut));
        let si: Ease = "arcaea.si".parse().unwrap();
        assert_eq!(si.namespace().map(Namespace::as_str), Some("arcaea"));
        for bad in ["tap", "bezier", ".x"] {
            assert!(bad.parse::<Ease>().is_err(), "{bad:?} accepted");
        }
    }

    #[test]
    fn every_shared_word_reads_back_as_itself() {
        for word in ["tap", "hold", "roll", "mine", "lift", "fake"] {
            assert_eq!(word.parse::<Kind>().unwrap().to_string(), word);
        }
        for word in ["linear", "step", "in", "out", "in_out"] {
            assert_eq!(word.parse::<Ease>().unwrap().to_string(), word);
        }
    }

    #[test]
    fn identifiers_deserialize_through_their_grammar_and_write_as_plain_strings() {
        serde_json::from_value::<Kind>(serde_json::json!("slide")).unwrap_err();
        let kind: Kind = serde_json::from_value(serde_json::json!("psk.flick")).unwrap();
        assert_eq!(
            serde_json::to_value(&kind).unwrap(),
            serde_json::json!("psk.flick")
        );
    }

    #[test]
    fn maps_keyed_by_validated_strings_look_up_by_plain_str() {
        let plugins =
            std::collections::BTreeMap::from([(Namespace::new("swing").unwrap(), "0.2.0")]);
        assert_eq!(plugins.get("swing"), Some(&"0.2.0"));
    }

    #[test]
    fn a_key_keeps_its_name_after_the_first_dot() {
        let key: Key = "arcaea.timing.group".parse().unwrap();
        assert_eq!(
            (key.namespace().as_str(), key.name().as_str()),
            ("arcaea", "timing.group")
        );
        assert_eq!(key.to_string(), "arcaea.timing.group");
    }
}
