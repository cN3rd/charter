//! Reading GGUF files and decoding ggml's block formats to float32.
//!
//! GGUF (ggml's file format, `ggml/docs/gguf.md`): a header of typed key-value metadata and
//! tensor descriptions, then the aligned tensor data. The decoders follow ggml's
//! `dequantize_row_*` in `ggml-quants.c`, with the same float32 operations in the same order, so
//! they give the same bits; `fixtures/ggml.json` holds gguf-py's decoding of each type to test
//! against.

use anyhow::{Context, Result, anyhow, bail, ensure};
use half::{bf16, f16};
use std::collections::HashMap;

/// A metadata value of a GGUF file, its integers widened to `i64` and its floats to `f64`.
#[derive(Clone, Debug, PartialEq)]
pub(crate) enum Value {
    /// Any of GGUF's integer types.
    Int(i64),
    /// Either of GGUF's float types.
    Float(f64),
    /// A boolean.
    Bool(bool),
    /// A UTF-8 string.
    Str(String),
    /// An array of values of one type.
    Array(Vec<Self>),
}

impl Value {
    /// The string this is, if it is one.
    pub(crate) fn as_str(&self) -> Option<&str> {
        match self {
            Self::Str(s) => Some(s),
            Self::Int(_) | Self::Float(_) | Self::Bool(_) | Self::Array(_) => None,
        }
    }

    /// The integer this is, if it is one.
    pub(crate) const fn as_int(&self) -> Option<i64> {
        match self {
            Self::Int(i) => Some(*i),
            Self::Float(_) | Self::Bool(_) | Self::Str(_) | Self::Array(_) => None,
        }
    }
}

/// A tensor's entry in a GGUF file's header.
#[derive(Clone, Debug)]
pub(crate) struct TensorInfo {
    /// Dimensions fastest first, as ggml's `ne`.
    pub(crate) dims: Vec<usize>,
    /// The ggml type of the tensor's data.
    pub(crate) kind: u32,
    /// Where the tensor's data starts in the data section.
    offset: usize,
}

/// A GGUF file's metadata and tensor entries, over the data the entries point into.
pub(crate) struct Gguf<'a> {
    /// The file's key-value metadata.
    pub(crate) metadata: HashMap<String, Value>,
    /// Each tensor's entry, by name.
    pub(crate) tensors: HashMap<String, TensorInfo>,
    /// The aligned data section after the header.
    data: &'a [u8],
}

/// A cursor over a GGUF file's bytes.
struct Reader<'a> {
    /// The whole file.
    bytes: &'a [u8],
    /// Where the next read starts.
    at: usize,
}

#[expect(
    clippy::little_endian_bytes,
    reason = "GGUF stores its numbers little-endian by specification"
)]
impl<'a> Reader<'a> {
    /// The next `n` bytes.
    fn take(&mut self, n: usize) -> Result<&'a [u8]> {
        let end = self
            .at
            .checked_add(n)
            .filter(|&e| e <= self.bytes.len())
            .ok_or_else(|| anyhow!("the GGUF file ends early"))?;
        let s = self.bytes.get(self.at..end).unwrap_or_default();
        self.at = end;
        Ok(s)
    }

    /// The next `N` bytes, as an array.
    fn array<const N: usize>(&mut self) -> Result<[u8; N]> {
        Ok(self.take(N)?.try_into().unwrap_or([0; N]))
    }

    /// The next `u32`.
    fn u32(&mut self) -> Result<u32> {
        Ok(u32::from_le_bytes(self.array()?))
    }

    /// The next `u64`.
    fn u64(&mut self) -> Result<u64> {
        Ok(u64::from_le_bytes(self.array()?))
    }

    /// The next `u64`, a length or a count, as a `usize`.
    #[expect(
        clippy::map_err_ignore,
        reason = "the conversion's error says no more than the message does"
    )]
    fn len(&mut self) -> Result<usize> {
        usize::try_from(self.u64()?).map_err(|_| anyhow!("a GGUF length does not fit in memory"))
    }

    /// The next string: a length, then that many bytes of UTF-8.
    fn string(&mut self) -> Result<String> {
        let n = self.len()?;
        String::from_utf8(self.take(n)?.to_vec()).context("a GGUF string is not UTF-8")
    }

    /// The next value, of GGUF value type `kind`.
    #[expect(
        clippy::map_err_ignore,
        reason = "the conversion's error says no more than the message does"
    )]
    fn value(&mut self, kind: u32) -> Result<Value> {
        Ok(match kind {
            0 => Value::Int(i64::from(self.array::<1>()?[0])),
            1 => Value::Int(i64::from(i8::from_le_bytes(self.array()?))),
            2 => Value::Int(i64::from(u16::from_le_bytes(self.array()?))),
            3 => Value::Int(i64::from(i16::from_le_bytes(self.array()?))),
            4 => Value::Int(i64::from(self.u32()?)),
            5 => Value::Int(i64::from(i32::from_le_bytes(self.array()?))),
            6 => Value::Float(f64::from(f32::from_le_bytes(self.array()?))),
            7 => Value::Bool(self.array::<1>()?[0] != 0),
            8 => Value::Str(self.string()?),
            9 => {
                let item = self.u32()?;
                // Arrays nested to any depth would be read to it, past the end of the stack.
                ensure!(item != 9, "a GGUF array of arrays is not supported");
                let n = self.len()?;
                Value::Array(
                    std::iter::repeat_with(|| self.value(item))
                        .take(n)
                        .collect::<Result<_>>()?,
                )
            }
            10 => Value::Int(
                i64::try_from(self.u64()?).map_err(|_| anyhow!("a GGUF integer is too large"))?,
            ),
            11 => Value::Int(i64::from_le_bytes(self.array()?)),
            12 => Value::Float(f64::from_le_bytes(self.array()?)),
            other => bail!("unknown GGUF value type {other}"),
        })
    }
}

impl<'a> Gguf<'a> {
    /// The header of the GGUF file `bytes`, over its data.
    pub(crate) fn parse(bytes: &'a [u8]) -> Result<Self> {
        let mut r = Reader { bytes, at: 0 };
        ensure!(r.take(4)? == b"GGUF", "not a GGUF file");
        let version = r.u32()?;
        ensure!(
            version == 3,
            "GGUF version {version} is not supported, only 3"
        );
        let (tensor_count, kv_count) = (r.len()?, r.len()?);
        let mut metadata = HashMap::new();
        for _ in 0..kv_count {
            let key = r.string()?;
            let kind = r.u32()?;
            metadata.insert(key, r.value(kind)?);
        }
        let mut tensors = HashMap::new();
        for _ in 0..tensor_count {
            let name = r.string()?;
            let n = usize::try_from(r.u32()?)?;
            let dims = std::iter::repeat_with(|| r.len())
                .take(n)
                .collect::<Result<Vec<_>>>()?;
            let kind = r.u32()?;
            let offset = r.len()?;
            tensors.insert(name, TensorInfo { dims, kind, offset });
        }
        // A negative alignment puts the data past the end of any file, as the check below finds.
        let alignment = usize::try_from(
            metadata
                .get("general.alignment")
                .and_then(Value::as_int)
                .unwrap_or(32),
        )
        .unwrap_or(usize::MAX);
        ensure!(alignment > 0, "the GGUF alignment is 0");
        let start = r.at.div_ceil(alignment).saturating_mul(alignment);
        ensure!(start <= bytes.len(), "the GGUF file ends before its data");
        Ok(Self {
            metadata,
            tensors,
            data: bytes.get(start..).unwrap_or_default(),
        })
    }

    /// The tensor `name` decoded to float32, and its shape slowest first, as torch has it.
    pub(crate) fn read(&self, name: &str) -> Result<(Vec<f32>, Vec<usize>)> {
        let info = self
            .tensors
            .get(name)
            .with_context(|| format!("the GGUF file has no {name}"))?;
        let count = product(&info.dims).with_context(|| format!("{name} is too large"))?;
        let size = byte_size(info.kind, count)?;
        let bytes = self
            .data
            .get(info.offset..info.offset.saturating_add(size))
            .with_context(|| format!("{name} runs past the end of the file"))?;
        let values = decode(info.kind, bytes, count)?;
        let shape = match self.metadata.get(&format!("timingator.shape.{name}")) {
            Some(Value::Array(dims)) => dims
                .iter()
                .map(|d| usize::try_from(d.as_int()?).ok())
                .collect::<Option<Vec<_>>>()
                .context("a shape is not integers")?,
            _ => info.dims.iter().rev().copied().collect(),
        };
        ensure!(
            product(&shape) == Some(count),
            "{name}'s shape {shape:?} does not hold {count} values"
        );
        Ok((values, shape))
    }
}

/// The number of values `dims` hold, unless it is past any size.
fn product(dims: &[usize]) -> Option<usize> {
    dims.iter()
        .try_fold(1_usize, |count, &dim| count.checked_mul(dim))
}

/// Weights per block and bytes per block of each ggml type this reads.
fn block(kind: u32) -> Result<(usize, usize)> {
    Ok(match kind {
        0 => (1, 4),        // F32
        1 | 30 => (1, 2),   // F16, BF16
        2 | 20 => (32, 18), // Q4_0, IQ4_NL
        3 => (32, 20),      // Q4_1
        8 => (32, 34),      // Q8_0
        12 => (256, 144),   // Q4_K
        13 => (256, 176),   // Q5_K
        14 => (256, 210),   // Q6_K
        39 => (32, 17),     // MXFP4
        other => bail!("ggml type {other} is not supported"),
    })
}

/// The bytes `count` weights of ggml type `kind` take.
fn byte_size(kind: u32, count: usize) -> Result<usize> {
    let (weights, bytes) = block(kind)?;
    ensure!(
        count.is_multiple_of(weights),
        "{count} values do not fill blocks of {weights}"
    );
    Ok(count.div_euclid(weights).saturating_mul(bytes))
}

/// The 16 weights an `IQ4_NL` nibble picks between.
const IQ4NL: [i8; 16] = [
    -127, -104, -83, -65, -49, -35, -22, -10, 1, 13, 25, 38, 53, 69, 89, 113,
];
/// E2M1 values doubled; the E8M0 scale is halved to match, as ggml does.
const MXFP4: [i8; 16] = [0, 1, 2, 3, 4, 6, 8, 12, 0, -1, -2, -3, -4, -6, -8, -12];

/// The float16 at the start of `b`, as float32.
#[expect(
    clippy::indexing_slicing,
    clippy::missing_asserts_for_indexing,
    reason = "every block holds its two-byte float16 scales where the decoders read them"
)]
fn half(b: &[u8]) -> f32 {
    f16::from_le_bytes([b[0], b[1]]).to_f32()
}

/// `ggml_e8m0_to_fp32_half`: 2^(e - 128).
fn e8m0_half(e: u8) -> f32 {
    let e = u32::from(e);
    f32::from_bits(if e < 2 {
        0x0020_0000 << e
    } else {
        e.saturating_sub(1) << 23
    })
}

/// Element `i` of 32 in a block of nibbles: low halves hold 0..16, high halves 16..32.
#[expect(
    clippy::indexing_slicing,
    clippy::arithmetic_side_effects,
    reason = "i is below 32 and a block's nibbles fill 16 bytes"
)]
const fn nibble(qs: &[u8], i: usize) -> u8 {
    if i < 16 { qs[i] & 0xF } else { qs[i - 16] >> 4 }
}

/// Sub-block `j`'s 6-bit scale and min in a K-quant's 12 scale bytes (`get_scale_min_k4`).
#[expect(
    clippy::indexing_slicing,
    clippy::arithmetic_side_effects,
    reason = "j is one of 8 sub-blocks, which keeps every index within the 12 scale bytes"
)]
const fn scale_min(s: &[u8], j: usize) -> (u8, u8) {
    if j < 4 {
        (s[j] & 63, s[j + 4] & 63)
    } else {
        (
            (s[j + 4] & 0xF) | ((s[j - 4] >> 6) << 4),
            (s[j + 4] >> 4) | ((s[j] >> 6) << 4),
        )
    }
}

/// `count` values of ggml type `kind` decoded from `bytes` to float32.
#[expect(
    clippy::little_endian_bytes,
    reason = "ggml stores float32 tensors little-endian"
)]
pub(crate) fn decode(kind: u32, bytes: &[u8], count: usize) -> Result<Vec<f32>> {
    let (weights, size) = block(kind)?;
    ensure!(
        bytes.len() == count.div_euclid(weights).saturating_mul(size),
        "{} bytes do not hold {count} values of ggml type {kind}",
        bytes.len()
    );
    let mut out = Vec::with_capacity(count);
    match kind {
        0 => out.extend(
            bytes
                .as_chunks::<4>()
                .0
                .iter()
                .map(|&b| f32::from_le_bytes(b)),
        ),
        1 => out.extend(
            bytes
                .as_chunks::<2>()
                .0
                .iter()
                .map(|&b| f16::from_le_bytes(b).to_f32()),
        ),
        30 => out.extend(
            bytes
                .as_chunks::<2>()
                .0
                .iter()
                .map(|&b| bf16::from_le_bytes(b).to_f32()),
        ),
        _ => {
            for b in bytes.chunks_exact(size) {
                decode_block(kind, b, &mut out)?;
            }
        }
    }
    Ok(out)
}

/// The weights of `bytes`, one block of ggml type `kind`, appended to `out` as ggml's
/// `dequantize_row_*` computes them.
#[expect(
    clippy::indexing_slicing,
    clippy::missing_asserts_for_indexing,
    reason = "each block is the type's whole size, which every index and range stays within"
)]
#[expect(
    clippy::arithmetic_side_effects,
    clippy::integer_division,
    reason = "index arithmetic over a block's weights, as ggml's dequantize_row_* loops do it"
)]
fn decode_block(kind: u32, bytes: &[u8], out: &mut Vec<f32>) -> Result<()> {
    match kind {
        2 => {
            let d = half(bytes);
            out.extend((0..32).map(|i| d * f32::from(nibble(&bytes[2..], i).cast_signed() - 8)));
        }
        3 => {
            let (d, m) = (half(bytes), half(&bytes[2..]));
            out.extend((0..32).map(|i| d * f32::from(nibble(&bytes[4..], i)) + m));
        }
        8 => {
            let d = half(bytes);
            out.extend(bytes[2..34].iter().map(|&q| d * f32::from(q.cast_signed())));
        }
        20 => {
            let d = half(bytes);
            out.extend((0..32).map(|i| d * f32::from(IQ4NL[usize::from(nibble(&bytes[2..], i))])));
        }
        39 => {
            let d = e8m0_half(bytes[0]);
            out.extend((0..32).map(|i| d * f32::from(MXFP4[usize::from(nibble(&bytes[1..], i))])));
        }
        12 | 13 => {
            let (d, dmin) = (half(bytes), half(&bytes[2..]));
            let scales = &bytes[4..16];
            let (qh, qs) = if kind == 13 {
                (&bytes[16..48], &bytes[48..176])
            } else {
                (&[][..], &bytes[16..144])
            };
            for j in 0..8 {
                let (sc, m) = scale_min(scales, j);
                let (d1, m1) = (d * f32::from(sc), dmin * f32::from(m));
                for i in 0..32 {
                    let byte = qs[(j / 2) * 32 + i];
                    let mut q = if j % 2 == 0 { byte & 0xF } else { byte >> 4 };
                    if kind == 13 {
                        q |= ((qh[i] >> j) & 1) << 4;
                    }
                    out.push(d1 * f32::from(q) - m1);
                }
            }
        }
        14 => {
            let (ql, qh, sc) = (&bytes[..128], &bytes[128..192], &bytes[192..208]);
            let d = half(&bytes[208..]);
            for p in 0..256 {
                let (h, r) = (p / 128, p % 128);
                let low = (ql[64 * h + r % 64] >> (4 * (r / 64))) & 0xF;
                let high = (qh[32 * h + r % 32] >> (2 * (r / 32))) & 3;
                let q = (low | (high << 4)).cast_signed() - 32;
                out.push(d * f32::from(sc[p / 16].cast_signed()) * f32::from(q));
            }
        }
        other => bail!("ggml type {other} is not supported"),
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(serde::Deserialize)]
    struct Case {
        kind: u32,
        bytes: String,
        values: Vec<f32>,
    }

    fn hex(s: &str) -> Vec<u8> {
        s.as_bytes()
            .chunks(2)
            .map(|pair| u8::from_str_radix(std::str::from_utf8(pair).unwrap(), 16).unwrap())
            .collect()
    }

    /// Every type decodes to the bits gguf-py decodes it to (`quant/make_ggml_fixture.py`).
    #[test]
    fn every_type_decodes_as_gguf_py_does() {
        let cases: Vec<Case> = serde_json::from_str(include_str!("../fixtures/ggml.json")).unwrap();
        assert_eq!(cases.len(), 10);
        for case in cases {
            let got = decode(case.kind, &hex(&case.bytes), case.values.len()).unwrap();
            assert_eq!(got.len(), case.values.len());
            for (i, (a, b)) in got.iter().zip(&case.values).enumerate() {
                assert_eq!(
                    a.to_bits(),
                    b.to_bits(),
                    "type {} value {i}: {a} vs {b}",
                    case.kind
                );
            }
        }
    }

    #[test]
    #[expect(
        clippy::little_endian_bytes,
        reason = "a GGUF file is little-endian by specification"
    )]
    fn a_minimal_file_parses() {
        let mut f = b"GGUF".to_vec();
        f.extend(3_u32.to_le_bytes());
        f.extend(1_u64.to_le_bytes());
        f.extend(1_u64.to_le_bytes());
        let text = |file: &mut Vec<u8>, s: &str| {
            file.extend(u64::try_from(s.len()).unwrap().to_le_bytes());
            file.extend(s.as_bytes());
        };
        text(&mut f, "general.name");
        f.extend(8_u32.to_le_bytes());
        text(&mut f, "x");
        text(&mut f, "w");
        f.extend(2_u32.to_le_bytes());
        f.extend(2_u64.to_le_bytes());
        f.extend(1_u64.to_le_bytes());
        f.extend(0_u32.to_le_bytes());
        f.extend(0_u64.to_le_bytes());
        f.resize(f.len().div_ceil(32) * 32, 0);
        f.extend(1.5_f32.to_le_bytes());
        f.extend((-2.0_f32).to_le_bytes());
        let g = Gguf::parse(&f).unwrap();
        assert_eq!(g.metadata["general.name"], Value::Str("x".into()));
        assert_eq!(g.read("w").unwrap(), (vec![1.5, -2.0], vec![1, 2]));
        assert!(Gguf::parse(&f[..30]).is_err());
        assert!(Gguf::parse(b"GGUF\x02\0\0\0").is_err());
    }

    /// The start of a version 3 GGUF file of `tensors` tensors and `values` metadata values.
    #[expect(
        clippy::little_endian_bytes,
        reason = "a GGUF file is little-endian by specification"
    )]
    fn header(tensors: u64, values: u64) -> Vec<u8> {
        let mut f = b"GGUF".to_vec();
        f.extend(3_u32.to_le_bytes());
        f.extend(tensors.to_le_bytes());
        f.extend(values.to_le_bytes());
        f
    }

    /// `s` as a GGUF string, its length first.
    #[expect(
        clippy::little_endian_bytes,
        reason = "a GGUF file is little-endian by specification"
    )]
    fn text(file: &mut Vec<u8>, s: &str) {
        file.extend(u64::try_from(s.len()).unwrap().to_le_bytes());
        file.extend(s.as_bytes());
    }

    #[test]
    #[expect(
        clippy::little_endian_bytes,
        reason = "a GGUF file is little-endian by specification"
    )]
    fn an_array_of_arrays_is_refused_rather_than_read_to_any_depth() {
        let mut f = header(0, 1);
        text(&mut f, "a");
        for kind in [9_u32, 9] {
            f.extend(kind.to_le_bytes());
        }
        f.extend(1_u64.to_le_bytes());
        f.extend(8_u32.to_le_bytes());
        f.extend(1_u64.to_le_bytes());
        text(&mut f, "x");
        f.resize(f.len().div_ceil(32) * 32, 0);
        assert!(Gguf::parse(&f).is_err());
    }

    #[test]
    #[expect(
        clippy::little_endian_bytes,
        reason = "a GGUF file is little-endian by specification"
    )]
    fn an_alignment_of_zero_is_refused() {
        let mut f = header(0, 1);
        text(&mut f, "general.alignment");
        f.extend(4_u32.to_le_bytes());
        f.extend(0_u32.to_le_bytes());
        assert!(Gguf::parse(&f).is_err());
    }

    #[test]
    #[expect(
        clippy::little_endian_bytes,
        reason = "a GGUF file is little-endian by specification"
    )]
    fn dimensions_past_any_size_are_refused() {
        let huge = 1_u64 << 32;
        let mut f = header(1, 0);
        text(&mut f, "w");
        f.extend(3_u32.to_le_bytes());
        for _ in 0..3 {
            f.extend(huge.to_le_bytes());
        }
        f.extend(0_u32.to_le_bytes());
        f.extend(0_u64.to_le_bytes());
        f.resize(f.len().div_ceil(32) * 32, 0);
        Gguf::parse(&f).unwrap().read("w").unwrap_err();
    }
}
