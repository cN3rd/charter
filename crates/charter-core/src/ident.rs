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

            /// The validated text held by this identifier.
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
string_newtype!(
    /// A chart-local layer name, between 1 and 64 characters without surrounding whitespace or controls.
    LayerName, "a layer name of 1 to 64 characters without surrounding whitespace or controls",
    |v| !v.is_empty()
        && v.chars().count() <= 64
        && v.trim() == v
        && !v.chars().any(char::is_control)
);

/// An extension key or an `other` timing name: a namespace and a name, written
/// `<namespace>.<name>` and split at the first dot.
///
/// The pair sits behind one pointer so that a key costs a word wherever it is held. Every note,
/// path point and marker carries a [`Kind`], [`Ease`] or [`MarkerKind`] whose `Other` holds one,
/// and a shared word such as `tap` would otherwise pay for the widest plugin key.
#[derive(Clone, PartialEq, Eq, PartialOrd, Ord, Hash, ::serde::Deserialize)]
#[serde(try_from = "String")]
pub struct Key(Box<Parts>);

/// The two halves of a [`Key`], ordered namespace first so keys sort by owner.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
struct Parts {
    /// Plugin owning the key.
    namespace: Namespace,
    /// Non-empty part following the first dot; may itself contain dots.
    name: Name,
}

impl Key {
    /// Joins an owning namespace and non-empty name into an extension key.
    #[must_use]
    pub fn new(namespace: Namespace, name: Name) -> Self {
        Self(Box::new(Parts { namespace, name }))
    }

    /// The owning plugin's namespace.
    #[must_use]
    pub fn namespace(&self) -> &Namespace {
        &self.0.namespace
    }

    /// Whether `namespace` owns this extension key.
    #[must_use]
    pub fn is_owned_by(&self, namespace: &str) -> bool {
        self.0.namespace.as_str() == namespace
    }

    /// Part of the key after the first dot.
    #[must_use]
    pub fn name(&self) -> &Name {
        &self.0.name
    }
}

impl FromStr for Key {
    type Err = InvalidValue;

    fn from_str(text: &str) -> Result<Self, InvalidValue> {
        split(text)
            .map(|(namespace, name)| {
                Self::new(Namespace(namespace.to_owned()), Name(name.to_owned()))
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
        write!(f, "{}.{}", self.0.namespace.as_str(), self.0.name.as_str())
    }
}

/// Written as the two halves, so that holding them behind a pointer stays invisible.
impl fmt::Debug for Key {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Key")
            .field("namespace", &self.0.namespace)
            .field("name", &self.0.name)
            .finish()
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
            $(#[doc = concat!("Shared `", $word, "` vocabulary word.")]
            $variant,)+
            /// A plugin's own word, under its namespace.
            Other(Key),
        }

        impl $name {
            /// The owning plugin's namespace, or `None` for a shared word.
            #[must_use]
            pub fn namespace(&self) -> Option<&Namespace> {
                match self {
                    Self::Other(key) => Some(key.namespace()),
                    $(Self::$variant)|+ => None,
                }
            }

            /// Whether this is a shared word or a word owned by `namespace`.
            #[must_use]
            pub fn is_shared_or_owned_by(&self, namespace: &str) -> bool {
                self.namespace().is_none_or(|owner| owner.as_str() == namespace)
            }

            /// The plugin-owned key when `namespace` owns this word.
            #[must_use]
            pub fn key_owned_by(&self, namespace: &str) -> Option<&Key> {
                match self {
                    Self::Other(key) if key.is_owned_by(namespace) => Some(key),
                    Self::Other(_) $(| Self::$variant)+ => None,
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

/// Proofs for Kani, run with `cargo kani -p charter-core`.
#[cfg(kani)]
mod proofs {
    use super::*;

    #[kani::proof]
    #[kani::unwind(7)]
    fn a_bounded_namespace_is_accepted_exactly_for_its_ascii_grammar() {
        let bytes: [u8; 4] = kani::any();
        let length: usize = kani::any();
        kani::assume(length <= bytes.len());
        if let Ok(text) = std::str::from_utf8(&bytes[..length]) {
            let expected = bytes[..length].first().is_some_and(u8::is_ascii_lowercase)
                && bytes[1..length.max(1)].iter().all(|byte| {
                    byte.is_ascii_lowercase() || byte.is_ascii_digit() || *byte == b'_'
                });
            assert!(is_namespace(text) == expected);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[expect(
        clippy::non_ascii_literal,
        reason = "the layer-name test counts a Unicode emoji as one character"
    )]
    fn layer_names_reject_empty_whitespace_controls_and_overlong_text() {
        for bad in [
            "",
            " Kick",
            "Kick ",
            "Kick\nSnare",
            "\u{7f}",
            "a".repeat(65).as_str(),
        ] {
            assert!(LayerName::new(bad).is_err(), "{bad:?}");
        }
        assert_eq!(LayerName::new("Kick drum").unwrap().as_str(), "Kick drum");
        LayerName::new("🎵".repeat(64)).unwrap();
    }

    proptest::proptest! {
        #![proptest_config(crate::proptest_config())]

        #[test]
        fn arbitrary_layer_names_never_bypass_validation(text in ".{0,80}") {
            let json = serde_json::to_string(&text).unwrap();
            match LayerName::new(text) {
                Ok(name) => {
                    let parsed: LayerName = serde_json::from_str(&json).unwrap();
                    proptest::prop_assert_eq!(parsed, name);
                }
                Err(_) => {
                    proptest::prop_assert!(serde_json::from_str::<LayerName>(&json).is_err());
                }
            }
        }
    }

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

    #[test]
    fn keys_tell_whether_a_namespace_owns_them() {
        let key: Key = "sm.stop".parse().unwrap();
        assert!(key.is_owned_by("sm"));
        assert!(!key.is_owned_by("ttr"));
    }

    #[test]
    fn vocabulary_words_tell_whether_a_ruleset_should_keep_them() {
        assert!(
            "bookmark"
                .parse::<MarkerKind>()
                .unwrap()
                .is_shared_or_owned_by("sm")
        );
        assert!(
            "sm.label"
                .parse::<MarkerKind>()
                .unwrap()
                .is_shared_or_owned_by("sm")
        );
        assert!(
            !"ttr.section"
                .parse::<MarkerKind>()
                .unwrap()
                .is_shared_or_owned_by("sm")
        );
    }

    #[test]
    fn vocabulary_words_expose_their_key_only_for_the_owning_namespace() {
        let label = "sm.label".parse::<MarkerKind>().unwrap();
        assert_eq!(label.key_owned_by("sm").unwrap().name().as_str(), "label");
        assert!(label.key_owned_by("ttr").is_none());
        assert!(
            "bookmark"
                .parse::<MarkerKind>()
                .unwrap()
                .key_owned_by("sm")
                .is_none()
        );
    }
}
