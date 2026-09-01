// Written by Richard Christopher, Copyright 2026 NeoTec, LLC
// SPDX-License-Identifier: GPL-3.0-or-later

//! The symbol table.
//!
//! Interning is a hash lookup, not a scan. `builder.rs::intern_tag` dedups the
//! per-file tag table by linear scan, which is `O(n·k)`; that is affordable for
//! the handful of distinct tags one file carries and is not affordable for
//! every mapping key in a project, so this table keeps an index.

use std::collections::HashMap;

/// Handle to an interned string. Equal handles mean equal text, so comparing
/// two names is a `u32` comparison.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Debug)]
pub struct Symbol(pub u32);

impl Symbol {
    /// The handle as a `usize` index.
    #[must_use]
    pub fn index(self) -> usize {
        self.0 as usize
    }
}

/// Every distinct name a project mentions, stored once.
#[derive(Default)]
pub struct SymbolTable {
    names: Vec<Box<str>>,
    index: HashMap<Box<str>, Symbol>,
}

impl SymbolTable {
    /// An empty table.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Intern `text`, returning the handle it already had or a fresh one.
    pub fn intern(&mut self, text: &str) -> Symbol {
        if let Some(symbol) = self.index.get(text) {
            return *symbol;
        }
        let symbol = Symbol(u32::try_from(self.names.len()).expect("symbol table overflow"));
        let owned: Box<str> = text.into();
        self.names.push(owned.clone());
        self.index.insert(owned, symbol);
        symbol
    }

    /// The handle `text` already has, without creating one.
    #[must_use]
    pub fn get(&self, text: &str) -> Option<Symbol> {
        self.index.get(text).copied()
    }

    /// The text behind a handle.
    #[must_use]
    pub fn resolve(&self, symbol: Symbol) -> Option<&str> {
        self.names.get(symbol.index()).map(|s| &**s)
    }

    /// Number of distinct names.
    #[must_use]
    pub fn len(&self) -> usize {
        self.names.len()
    }

    /// Whether nothing has been interned.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.names.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn equal_text_interns_to_one_handle() {
        let mut table = SymbolTable::new();
        let a = table.intern("port");
        let b = table.intern("port");
        let c = table.intern("host");
        assert_eq!(a, b);
        assert_ne!(a, c);
        assert_eq!(table.len(), 2);
    }

    #[test]
    fn a_handle_resolves_back_to_its_text() {
        let mut table = SymbolTable::new();
        let symbol = table.intern("acme");
        assert_eq!(table.resolve(symbol), Some("acme"));
        assert_eq!(table.get("acme"), Some(symbol));
        assert_eq!(table.get("missing"), None);
        assert_eq!(table.resolve(Symbol(99)), None);
    }

    #[test]
    fn an_empty_table_reports_itself_empty() {
        let table = SymbolTable::new();
        assert!(table.is_empty());
    }
}
