// Written by Richard Christopher, Copyright 2026 NeoTec, LLC
// SPDX-License-Identifier: GPL-3.0-or-later

//! The environment layer, behind a trait so it can be tested without touching
//! the process environment.

/// A source of environment variables.
pub trait Environment {
    /// The value of `key`, if set.
    fn get(&self, key: &str) -> Option<String>;

    /// Every set variable whose name begins with `prefix`, as
    /// `(suffix, value)` pairs.
    fn with_prefix(&self, prefix: &str) -> Vec<(String, String)>;
}

/// The real process environment.
pub struct ProcessEnvironment;

impl Environment for ProcessEnvironment {
    fn get(&self, key: &str) -> Option<String> {
        std::env::var(key).ok()
    }

    fn with_prefix(&self, prefix: &str) -> Vec<(String, String)> {
        std::env::vars()
            .filter_map(|(k, v)| k.strip_prefix(prefix).map(|s| (s.to_owned(), v)))
            .collect()
    }
}

/// A fixed set of variables, for tests.
#[derive(Default)]
pub struct StaticEnvironment {
    entries: Vec<(String, String)>,
}

impl StaticEnvironment {
    /// An environment holding `entries`.
    pub fn new(entries: impl IntoIterator<Item = (&'static str, &'static str)>) -> Self {
        StaticEnvironment {
            entries: entries.into_iter().map(|(k, v)| (k.to_owned(), v.to_owned())).collect(),
        }
    }
}

impl Environment for StaticEnvironment {
    fn get(&self, key: &str) -> Option<String> {
        self.entries.iter().find(|(k, _)| k == key).map(|(_, v)| v.clone())
    }

    fn with_prefix(&self, prefix: &str) -> Vec<(String, String)> {
        self.entries
            .iter()
            .filter_map(|(k, v)| k.strip_prefix(prefix).map(|s| (s.to_owned(), v.clone())))
            .collect()
    }
}
