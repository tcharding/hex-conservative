// SPDX-License-Identifier: CC0-1.0

//! Test the API surface of `hex-conservative`.
//!
//! The point of these tests is to check the API surface as opposed to test the API functionality.
//!
//! ref: <https://rust-lang.github.io/api-guidelines/about.html>

#![allow(dead_code)]
#![allow(unused_imports)]

use core::borrow::Borrow;
use core::marker::PhantomData;
use core::{fmt, slice};

// These imports test "typical" usage by user code.
use hex_conservative::{
    HexToArrayError, HexToBytesError, InvalidCharError, InvalidLengthError, OddLengthStringError,
    ToArrayError, ToBytesError,
};

/// A struct that includes all public error types.
// These derives are the policy of `rust-bitcoin` not Rust API guidelines.
#[derive(Debug, Clone, PartialEq, Eq)] // All public types implement Debug (C-DEBUG).
struct Errors {
    a: ToArrayError,
    b: ToBytesError,
    c: HexToArrayError,
    d: HexToBytesError,
    e: InvalidCharError,
    f: InvalidLengthError,
    g: OddLengthStringError,
}

#[test]
fn all_types_implement_send_sync() {
    fn assert_send<T: Send>() {}
    fn assert_sync<T: Sync>() {}

    // Error types should implement the Send and Sync traits (C-GOOD-ERR).
    assert_send::<Errors>();
    assert_sync::<Errors>();
}
