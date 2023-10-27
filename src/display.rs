// SPDX-License-Identifier: CC0-1.0

//! Helpers for displaying bytes as hex strings.
//!
//! This module provides a trait for displaying things as hex as well as an implementation for
//! `&[u8]`.
//!
//! For arrays and slices we support padding and precision for length < 512 bytes.
//!
//! # Examples
//!
//! ```
//! use hex_conservative::DisplayHex;
//!
//! // Display as hex.
//! let v = vec![0xde, 0xad, 0xbe, 0xef];
//! assert_eq!(format!("{}", v.as_hex()), "deadbeef");
//!
//! // Get the most significant bytes.
//! let v = vec![0x01, 0x23, 0x45, 0x67];
//! assert_eq!(format!("{0:.4}", v.as_hex()), "0123");
//!
//! // Padding with zeros
//! let v = vec![0xab; 2];
//! assert_eq!(format!("{:0>8}", v.as_hex()), "0000abab");
//!```

use core::borrow::Borrow;
use core::fmt;

use super::Case;
use crate::buf_encoder::{BufEncoder, FixedLenBuf, OutBytes};
#[cfg(feature = "alloc")]
use crate::prelude::*;

/// Extension trait for types that can be displayed as hex.
///
/// Types that have a single, obvious text representation being hex should **not** implement this
/// trait and simply implement `Display` instead.
///
/// This trait should be generally implemented for references only. We would prefer to use GAT but
/// that is beyond our MSRV. As a lint we require the `IsRef` trait which is implemented for all
/// references.
pub trait DisplayHex: Copy + sealed::IsRef {
    /// The type providing [`fmt::Display`] implementation.
    ///
    /// This is usually a wrapper type holding a reference to `Self`.
    type Display: fmt::Display + fmt::Debug + fmt::LowerHex + fmt::UpperHex;

    /// Display `Self` as a continuous sequence of ASCII hex chars.
    fn as_hex(self) -> Self::Display;

    /// Create a lower-hex-encoded string.
    ///
    /// A shorthand for `to_hex_string(Case::Lower)`, so that `Case` doesn't need to be imported.
    ///
    /// This may be faster than `.display_hex().to_string()` because it uses `reserve_suggestion`.
    #[cfg(feature = "alloc")]
    fn to_lower_hex_string(self) -> String { self.to_hex_string(Case::Lower) }

    /// Create an upper-hex-encoded string.
    ///
    /// A shorthand for `to_hex_string(Case::Upper)`, so that `Case` doesn't need to be imported.
    ///
    /// This may be faster than `.display_hex().to_string()` because it uses `reserve_suggestion`.
    #[cfg(feature = "alloc")]
    fn to_upper_hex_string(self) -> String { self.to_hex_string(Case::Upper) }

    /// Create a hex-encoded string.
    ///
    /// This may be faster than `.display_hex().to_string()` because it uses `reserve_suggestion`.
    #[cfg(feature = "alloc")]
    fn to_hex_string(self, case: Case) -> String {
        let mut string = String::new();
        self.append_hex_to_string(case, &mut string);
        string
    }

    /// Appends hex-encoded content to an existing `String`.
    ///
    /// This may be faster than `write!(string, "{:x}", self.as_hex())` because it uses
    /// `hex_reserve_sugggestion`.
    #[cfg(feature = "alloc")]
    fn append_hex_to_string(self, case: Case, string: &mut String) {
        use fmt::Write;

        string.reserve(self.hex_reserve_suggestion());
        match case {
            Case::Lower => write!(string, "{:x}", self.as_hex()),
            Case::Upper => write!(string, "{:X}", self.as_hex()),
        }
        .unwrap_or_else(|_| {
            let name = core::any::type_name::<Self::Display>();
            // We don't expect `std` to ever be buggy, so the bug is most likely in the `Display`
            // impl of `Self::Display`.
            panic!("The implementation of Display for {} returned an error when it shouldn't", name)
        })
    }

    /// Hints how much bytes to reserve when creating a `String`.
    ///
    /// Implementors that know the number of produced bytes upfront should override this.
    /// Defaults to 0.
    ///
    // We prefix the name with `hex_` to avoid potential collision with other methods.
    fn hex_reserve_suggestion(self) -> usize { 0 }
}

mod sealed {
    /// Trait marking a shared reference.
    pub trait IsRef: Copy {}

    impl<T: ?Sized> IsRef for &'_ T {}
}

impl<'a> DisplayHex for &'a [u8] {
    type Display = DisplayByteSlice<'a>;

    #[inline]
    fn as_hex(self) -> Self::Display { DisplayByteSlice { bytes: self } }

    #[inline]
    fn hex_reserve_suggestion(self) -> usize {
        // Since the string wouldn't fit into address space if this overflows (actually even for
        // smaller amounts) it's better to panic right away. It should also give the optimizer
        // better opportunities.
        self.len().checked_mul(2).expect("the string wouldn't fit into address space")
    }
}

#[cfg(feature = "alloc")]
impl<'a> DisplayHex for &'a alloc::vec::Vec<u8> {
    type Display = DisplayByteSlice<'a>;

    #[inline]
    fn as_hex(self) -> Self::Display { DisplayByteSlice { bytes: self } }

    #[inline]
    fn hex_reserve_suggestion(self) -> usize {
        // Since the string wouldn't fit into address space if this overflows (actually even for
        // smaller amounts) it's better to panic right away. It should also give the optimizer
        // better opportunities.
        self.len().checked_mul(2).expect("the string wouldn't fit into address space")
    }
}

/// Displays byte slice as hex.
///
/// Created by [`<&[u8] as DisplayHex>::as_hex`](DisplayHex::as_hex).
pub struct DisplayByteSlice<'a> {
    // pub because we want to keep lengths in sync
    pub(crate) bytes: &'a [u8],
}

impl<'a> DisplayByteSlice<'a> {
    fn display(&self, f: &mut fmt::Formatter, case: Case) -> fmt::Result {
        use fmt::Write;
        // There are at least two optimizations left:
        //
        // * Reusing the buffer (encoder) which may decrease the number of virtual calls
        // * Not recursing, avoiding another 1024B allocation and zeroing
        //
        // This would complicate the code so I was too lazy to do them but feel free to send a PR!

        let mut buf = [0u8; 1024];
        let mut encoder = BufEncoder::new(&mut buf);

        let pad_right = if let Some(width) = f.width() {
            let string_len = match f.precision() {
                Some(max) if self.bytes.len() * 2 > (max + 1) / 2 => max,
                Some(_) | None => self.bytes.len() * 2,
            };

            if string_len < width {
                let (left, right) = match f.align().unwrap_or(fmt::Alignment::Left) {
                    fmt::Alignment::Left => (0, width - string_len),
                    fmt::Alignment::Right => (width - string_len, 0),
                    fmt::Alignment::Center =>
                        ((width - string_len) / 2, (width - string_len + 1) / 2),
                };
                // Avoid division by zero and optimize for common case.
                if left > 0 {
                    let c = f.fill();
                    let chunk_len = encoder.put_filler(c, left);
                    let padding = encoder.as_str();
                    for _ in 0..(left / chunk_len) {
                        f.write_str(padding)?;
                    }
                    f.write_str(&padding[..((left % chunk_len) * c.len_utf8())])?;
                    encoder.clear();
                }
                right
            } else {
                0
            }
        } else {
            0
        };

        match f.precision() {
            Some(max) if self.bytes.len() > (max + 1) / 2 => {
                write!(f, "{}", self.bytes[..(max / 2)].as_hex())?;
                if max % 2 == 1 && self.bytes.len() > max / 2 + 1 {
                    f.write_char(
                        super::byte_to_hex(self.bytes[max / 2 + 1], case.table())[1].into(),
                    )?;
                }
            }
            Some(_) | None => {
                let mut chunks = self.bytes.chunks_exact(512);
                for chunk in &mut chunks {
                    encoder.put_bytes(chunk, case);
                    f.write_str(encoder.as_str())?;
                    encoder.clear();
                }
                encoder.put_bytes(chunks.remainder(), case);
                f.write_str(encoder.as_str())?;
            }
        }

        // Avoid division by zero and optimize for common case.
        if pad_right > 0 {
            encoder.clear();
            let c = f.fill();
            let chunk_len = encoder.put_filler(c, pad_right);
            let padding = encoder.as_str();
            for _ in 0..(pad_right / chunk_len) {
                f.write_str(padding)?;
            }
            f.write_str(&padding[..((pad_right % chunk_len) * c.len_utf8())])?;
        }
        Ok(())
    }
}

impl<'a> fmt::Display for DisplayByteSlice<'a> {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result { fmt::LowerHex::fmt(self, f) }
}

impl<'a> fmt::Debug for DisplayByteSlice<'a> {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result { fmt::LowerHex::fmt(self, f) }
}

impl<'a> fmt::LowerHex for DisplayByteSlice<'a> {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result { self.display(f, Case::Lower) }
}

impl<'a> fmt::UpperHex for DisplayByteSlice<'a> {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result { self.display(f, Case::Upper) }
}

/// Displays byte array as hex.
///
/// Created by [`<&[u8; LEN] as DisplayHex>::as_hex`](DisplayHex::as_hex).
// See `buf_encoder::impl_encode!` for `DisplayHex` implementation.
pub struct DisplayArray<A: Clone + IntoIterator, B: FixedLenBuf>
where
    A::Item: Borrow<u8>,
{
    array: A,
    _buffer_marker: core::marker::PhantomData<B>,
}

impl<A: Clone + IntoIterator, B: FixedLenBuf> DisplayArray<A, B>
where
    A::Item: Borrow<u8>,
{
    /// Creates the wrapper.
    #[inline]
    pub fn new(array: A) -> Self { DisplayArray { array, _buffer_marker: Default::default() } }

    fn display(&self, f: &mut fmt::Formatter, case: Case) -> fmt::Result {
        let mut buf = B::uninit();
        let mut encoder = BufEncoder::new(&mut buf);
        encoder.put_bytes(self.array.clone(), case);
        f.pad_integral(true, "0x", encoder.as_str())
    }
}

impl<A: Clone + IntoIterator, B: FixedLenBuf> fmt::Display for DisplayArray<A, B>
where
    A::Item: Borrow<u8>,
{
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result { fmt::LowerHex::fmt(self, f) }
}

impl<A: Clone + IntoIterator, B: FixedLenBuf> fmt::Debug for DisplayArray<A, B>
where
    A::Item: Borrow<u8>,
{
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result { fmt::LowerHex::fmt(self, f) }
}

impl<A: Clone + IntoIterator, B: FixedLenBuf> fmt::LowerHex for DisplayArray<A, B>
where
    A::Item: Borrow<u8>,
{
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result { self.display(f, Case::Lower) }
}

impl<A: Clone + IntoIterator, B: FixedLenBuf> fmt::UpperHex for DisplayArray<A, B>
where
    A::Item: Borrow<u8>,
{
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result { self.display(f, Case::Upper) }
}

/// Format known-length array as hex.
///
/// This supports all formatting options of formatter and may be faster than calling `as_hex()` on
/// an arbitrary `&[u8]`. Note that the implementation intentionally keeps leading zeros even when
/// not requested. This is designed to display values such as hashes and keys and removing leading
/// zeros would be confusing.
///
/// Note that the bytes parameter is `IntoIterator` this means that if you would like to do some
/// manipulation to the byte array before formatting then you can. For example `bytes.iter().rev()`
/// to print the array backwards.
///
/// ## Parameters
///
/// * `$formatter` - a [`fmt::Formatter`].
/// * `$len` known length of `$bytes`, must be a const expression.
/// * `$bytes` - bytes to be encoded, most likely a reference to an array.
/// * `$case` - value of type [`Case`] determining whether to format as lower or upper case.
///
/// ## Panics
///
/// This macro panics if `$len` is not equal to `$bytes.len()`. It also fails to compile if `$len`
/// is more than half of `usize::MAX`.
#[macro_export]
macro_rules! fmt_hex_exact {
    ($formatter:expr, $len:expr, $bytes:expr, $case:expr) => {{
        // statically check $len
        #[allow(deprecated)]
        const _: () = [()][($len > usize::MAX / 2) as usize];
        assert_eq!($bytes.len(), $len);
        let mut buf = [0u8; $len * 2];
        let buf = $crate::buf_encoder::AsOutBytes::as_mut_out_bytes(&mut buf);
        $crate::display::fmt_hex_exact_fn($formatter, buf, $bytes, $case)
    }};
}
pub use fmt_hex_exact;

// Implementation detail of `write_hex_exact` macro to de-duplicate the code
#[doc(hidden)]
#[inline]
pub fn fmt_hex_exact_fn<I>(
    f: &mut fmt::Formatter,
    buf: &mut OutBytes,
    bytes: I,
    case: Case,
) -> fmt::Result
where
    I: IntoIterator,
    I::Item: Borrow<u8>,
{
    let mut encoder = BufEncoder::new(buf);
    encoder.put_bytes(bytes, case);
    f.pad_integral(true, "0x", encoder.as_str())
}

#[cfg(test)]
mod tests {
    #[cfg(feature = "alloc")]
    use super::*;

    #[cfg(feature = "alloc")]
    mod alloc {
        use super::*;

        fn check_encoding(bytes: &[u8]) {
            use core::fmt::Write;

            let s1 = bytes.to_lower_hex_string();
            let mut s2 = String::with_capacity(bytes.len() * 2);
            for b in bytes {
                write!(s2, "{:02x}", b).unwrap();
            }
            assert_eq!(s1, s2);
        }

        #[test]
        fn empty() { check_encoding(b""); }

        #[test]
        fn single() { check_encoding(b"*"); }

        #[test]
        fn two() { check_encoding(b"*x"); }

        #[test]
        fn just_below_boundary() { check_encoding(&[42; 512]); }

        #[test]
        fn just_above_boundary() { check_encoding(&[42; 513]); }

        #[test]
        fn just_above_double_boundary() { check_encoding(&[42; 1025]); }

        #[test]
        fn fmt_exact_macro() {
            use crate::alloc::string::ToString;

            struct Dummy([u8; 32]);

            impl fmt::Display for Dummy {
                fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
                    fmt_hex_exact!(f, 32, &self.0, Case::Lower)
                }
            }

            assert_eq!(Dummy([42; 32]).to_string(), "2a".repeat(32));
        }

        #[test]
        fn display_short_with_padding() {
            let v = vec![0xbe, 0xef];
            assert_eq!(format!("Hello {:<8}!", v.as_hex()), "Hello beef    !");
            assert_eq!(format!("Hello {:-<8}!", v.as_hex()), "Hello beef----!");
            assert_eq!(format!("Hello {:^8}!", v.as_hex()), "Hello   beef  !");
            assert_eq!(format!("Hello {:>8}!", v.as_hex()), "Hello     beef!");
        }

        #[test]
        fn display_long() {
            // Note this string is shorter than the one above.
            let v = vec![0xab; 512];
            let mut want = "0".repeat(2000 - 1024);
            want.extend(core::iter::repeat("ab").take(512));
            let got = format!("{:0>2000}", v.as_hex());
            assert_eq!(got, want)
        }

        // Precision and padding act the same as for strings in the stdlib (because we use `Formatter::pad`).

        #[test]
        fn precision_truncates() {
            // Precision gets the most significant bytes.
            let v = vec![0x12, 0x34, 0x56, 0x78];
            // Remember the integer is number of hex chars not number of bytes.
            assert_eq!(format!("{0:.4}", v.as_hex()), "1234");
        }

        #[test]
        fn precision_with_padding_truncates() {
            // Precision gets the most significant bytes.
            let v = vec![0x12, 0x34, 0x56, 0x78];
            assert_eq!(format!("{0:10.4}", v.as_hex()), "1234      ");
        }

        #[test]
        fn precision_with_padding_pads_right() {
            let v = vec![0x12, 0x34, 0x56, 0x78];
            assert_eq!(format!("{0:10.20}", v.as_hex()), "12345678  ");
        }

        #[test]
        fn precision_with_padding_pads_left() {
            let v = vec![0x12, 0x34, 0x56, 0x78];
            assert_eq!(format!("{0:>10.20}", v.as_hex()), "  12345678");
        }

        #[test]
        fn precision_with_padding_pads_center() {
            let v = vec![0x12, 0x34, 0x56, 0x78];
            assert_eq!(format!("{0:^10.20}", v.as_hex()), " 12345678 ");
        }

        #[test]
        fn precision_with_padding_pads_center_odd() {
            let v = vec![0x12, 0x34, 0x56, 0x78];
            assert_eq!(format!("{0:^11.20}", v.as_hex()), " 12345678  ");
        }

        #[test]
        fn precision_does_not_extend() {
            let v = vec![0x12, 0x34, 0x56, 0x78];
            assert_eq!(format!("{0:.16}", v.as_hex()), "12345678");
        }

        #[test]
        fn padding_extends() {
            let v = vec![0xab; 2];
            assert_eq!(format!("{:0>8}", v.as_hex()), "0000abab");
        }

        #[test]
        fn padding_does_not_truncate() {
            let v = vec![0x12, 0x34, 0x56, 0x78];
            assert_eq!(format!("{:0>4}", v.as_hex()), "12345678");
        }
    }
}
