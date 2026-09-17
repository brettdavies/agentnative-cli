//! Response headers as the fetch layer exposes them: names lowercased,
//! insertion order kept, and a repeated name joined with `, `, which is how
//! the Fetch API's `Headers` object presents them to the site engine.

/// Response headers with lowercased names.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Headers {
    entries: Vec<(String, String)>,
}

impl Headers {
    /// No headers.
    pub fn new() -> Self {
        Self::default()
    }

    /// Build from name/value pairs, lowercasing names and joining repeats.
    pub fn from_pairs<I, K, V>(pairs: I) -> Self
    where
        I: IntoIterator<Item = (K, V)>,
        K: AsRef<str>,
        V: AsRef<str>,
    {
        let mut headers = Self::new();
        for (name, value) in pairs {
            headers.append(name.as_ref(), value.as_ref());
        }
        headers
    }

    /// Append one header. A name already present has the value joined onto
    /// its existing one with `, `.
    pub fn append(&mut self, name: &str, value: &str) {
        let name = name.to_ascii_lowercase();
        match self.entries.iter_mut().find(|(n, _)| *n == name) {
            Some((_, existing)) => {
                existing.push_str(", ");
                existing.push_str(value);
            }
            None => self.entries.push((name, value.to_string())),
        }
    }

    /// The value for a name, matched case-insensitively.
    pub fn get(&self, name: &str) -> Option<&str> {
        self.entries
            .iter()
            .find(|(n, _)| n.eq_ignore_ascii_case(name))
            .map(|(_, v)| v.as_str())
    }

    /// Whether a header with this name is present.
    pub fn contains(&self, name: &str) -> bool {
        self.get(name).is_some()
    }

    /// Every header in insertion order.
    pub fn iter(&self) -> impl Iterator<Item = (&str, &str)> {
        self.entries.iter().map(|(n, v)| (n.as_str(), v.as_str()))
    }

    /// The number of distinct header names.
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// Whether there are no headers.
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::Headers;

    #[test]
    fn names_are_lowercased_and_lookups_are_case_insensitive() {
        let headers = Headers::from_pairs([("Content-Type", "text/html"), ("X-Probe", "yes")]);
        assert_eq!(headers.get("content-type"), Some("text/html"));
        assert_eq!(headers.get("CONTENT-TYPE"), Some("text/html"));
        assert_eq!(
            headers.iter().map(|(n, _)| n).collect::<Vec<_>>(),
            ["content-type", "x-probe"]
        );
        assert!(headers.contains("x-probe"));
        assert!(!headers.contains("location"));
    }

    #[test]
    fn a_repeated_name_joins_its_values() {
        let headers = Headers::from_pairs([("Vary", "Accept"), ("vary", "User-Agent")]);
        assert_eq!(headers.get("vary"), Some("Accept, User-Agent"));
        assert_eq!(headers.len(), 1);
    }

    #[test]
    fn empty_headers_report_empty() {
        let headers = Headers::new();
        assert!(headers.is_empty());
        assert_eq!(headers.get("anything"), None);
    }
}
