use icu_locale_core::LanguageIdentifier;

/// A language, as a BCP 47 tag such as `en`, `sr-Latn` or `zh-Hant-TW`.
///
/// Tags are kept in their canonical form, so `EN-us` and `en-US` are equal.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct Language(LanguageIdentifier);

/// A tag that isn't valid BCP 47.
#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
#[error("{tag:?} isn't a language tag")]
pub struct Error {
    /// The tag.
    pub tag: String,
}

impl Language {
    /// The language `tag` names.
    pub fn parse(tag: &str) -> Result<Language, Error> {
        LanguageIdentifier::try_from_str(tag)
            .map(Language)
            .map_err(|_| Error {
                tag: tag.to_string(),
            })
    }

    /// The undetermined language, `und`: text whose language isn't known.
    pub fn und() -> Language {
        Language(LanguageIdentifier::UNKNOWN)
    }

    /// Its ICU4X identifier.
    pub fn identifier(&self) -> &LanguageIdentifier {
        &self.0
    }
}

impl Default for Language {
    fn default() -> Language {
        Language::und()
    }
}

impl std::fmt::Display for Language {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        std::fmt::Display::fmt(&self.0, f)
    }
}

impl std::str::FromStr for Language {
    type Err = Error;

    fn from_str(tag: &str) -> Result<Language, Error> {
        Language::parse(tag)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tags_are_canonical() {
        assert_eq!(Language::parse("EN-us").unwrap().to_string(), "en-US");
        assert_eq!(Language::parse("sr-latn").unwrap().to_string(), "sr-Latn");
        assert_eq!(Language::und().to_string(), "und");
        assert_eq!(Language::default(), Language::parse("und").unwrap());
    }

    #[test]
    fn nonsense_is_refused() {
        assert_eq!(
            Language::parse("not a tag"),
            Err(Error {
                tag: "not a tag".to_string()
            })
        );
    }
}
