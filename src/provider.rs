// Copyright (c) 2026 Otávio C.
// SPDX-License-Identifier: MIT

//! The lyrics providers and their base URLs.

use clap::ValueEnum;
use serde::Deserialize;

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ProviderKind {
    #[value(help = "The reference LRCLIB API: <https://lrclib.net/docs>")]
    Lrclib,
    #[value(
        help = "lrcmux's LRCLIB-compatible shim, aggregating multiple sources: <https://lrcmux.dev/docs>"
    )]
    Lrcmux,
}

pub struct ProviderSpec {
    pub name: &'static str,
    pub get_url: &'static str,
    pub search_url: &'static str,
    pub client_header: Option<&'static str>,
}

impl ProviderKind {
    #[must_use]
    pub const fn spec(self) -> ProviderSpec {
        match self {
            Self::Lrclib => ProviderSpec {
                name: "lrclib",
                get_url: "https://lrclib.net/api/get",
                search_url: "https://lrclib.net/api/search",
                client_header: Some("Lrclib-Client"),
            },
            Self::Lrcmux => ProviderSpec {
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
