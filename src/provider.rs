// Copyright (c) 2026 Otávio C.
// SPDX-License-Identifier: MIT

//! Provider selection: which lyrics API `http::Client` talks to.
//!
//! Both known providers speak the identical LRCLIB wire format, so a provider is just
//! a pair of URLs plus a display name, not a separate trait implementation. Adding another
//! LRCLIB-API-compatible service later is a one-line addition to `ProviderKind`/`spec()`. A
//! provider with a genuinely different response shape would be the point where a real
//! `Provider` trait becomes worth introducing, not before.

use clap::ValueEnum;

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub enum ProviderKind {
    /// The reference LRCLIB API: <https://lrclib.net/docs>.
    Lrclib,
    /// lrcmux's LRCLIB-compatible shim, aggregating multiple sources: <https://lrcmux.dev/docs>.
    Lrcmux,
}

pub struct ProviderSpec {
    /// Short name used in logging and error messages.
    pub name: &'static str,
    pub get_url: &'static str,
    pub search_url: &'static str,
    /// An extra provider-specific header name to also carry the User-Agent value in, if the
    /// provider documents one (e.g. LRCLIB's own suggested `Lrclib-Client` alternative for
    /// clients that can't set `User-Agent` directly). `None` when a provider has no such
    /// alternative, since sending a header literally named after another provider would be
    /// misleading, so this is opt-in per provider rather than a hardcoded constant.
    pub client_header: Option<&'static str>,
}

impl ProviderKind {
    pub fn spec(self) -> ProviderSpec {
        match self {
            ProviderKind::Lrclib => ProviderSpec {
                name: "lrclib",
                get_url: "https://lrclib.net/api/get",
                search_url: "https://lrclib.net/api/search",
                client_header: Some("Lrclib-Client"),
            },
            ProviderKind::Lrcmux => ProviderSpec {
                name: "lrcmux",
                get_url: "https://api.lrcmux.dev/compat/lrclib/api/get",
                search_url: "https://api.lrcmux.dev/compat/lrclib/api/search",
                client_header: None,
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn each_provider_has_distinct_well_shaped_urls() {
        let lrclib = ProviderKind::Lrclib.spec();
        let lrcmux = ProviderKind::Lrcmux.spec();

        for spec in [&lrclib, &lrcmux] {
            assert!(spec.get_url.ends_with("/api/get"), "{}", spec.get_url);
            assert!(
                spec.search_url.ends_with("/api/search"),
                "{}",
                spec.search_url
            );
        }

        assert_ne!(lrclib.get_url, lrcmux.get_url);
        assert_ne!(lrclib.search_url, lrcmux.search_url);
        assert_ne!(lrclib.name, lrcmux.name);
    }
}
