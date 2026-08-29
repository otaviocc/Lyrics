// Copyright (c) 2026 Otávio C.
// SPDX-License-Identifier: MIT

//! Library crate for the `lyrics` CLI. Split out from `main.rs` so integration tests can
//! exercise the read/write paths directly. See `tests/read_only_guarantee.rs`.

pub mod cli;
pub mod config;
pub mod ebook;
pub mod http;
pub mod lrc;
pub mod meta;
pub mod provider;
pub mod runner;
pub mod sidecar;
pub mod stats;
