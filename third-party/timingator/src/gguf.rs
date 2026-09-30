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

#[derive(Clone, Debug, PartialEq)]
pub enum Value {
    Int(i64),
    Float(f64),
    Bool(bool),
    Str(String),
    Array(Vec<Value>),
}

impl Value {
    pub fn as_str(&self) -> Option<&str> {
        match self {
            Self::Str(s) => Some(s),
            _ => None,
        }
    }

    pub fn as_int(&self) -> Option<i64> {
        match self {
            Self::Int(i) => Some(*i),
            _ => None,
        }
    }
}

#[derive(Clone, Debug)]
pub struct TensorInfo {
    /// Dimensions fastest first, as ggml's `ne`.
    pub dims: Vec<usize>,
    pub kind: u32,
    offset: usize,
}

pub struct Gguf<'a> {
    pub metadata: HashMap<String, Value>,
    pub tensors: HashMap<String, TensorInfo>,
    data: &'a [u8],
}

struct Reader<'a> {
    bytes: &'a [u8],
    at: usize,
}

impl<'a> Reader<'a> {
    fn take(&mut self, n: usize) -> Result<&'a [u8]> {
        let end = self
            .at
            .checked_add(n)
            .filter(|&e| e <= self.bytes.len())
            .ok_or_else(|| anyhow!("the GGUF file ends early"))?;
        let s = &self.bytes[self.at..end];
        self.at = end;
        Ok(s)
    }

    fn array<const N: usize>(&mut self) -> Result<[u8; N]> {
        Ok(self.take(N)?.try_into().unwrap_or([0; N]))
    }

    fn u32(&mut self) -> Result<u32> {
        Ok(u32::from_le_bytes(self.array()?))
    }

    fn u64(&mut self) -> Result<u64> {
        Ok(u64::from_le_bytes(self.array()?))
    }

    fn len(&mut self) -> Result<usize> {
        usize::try_from(self.u64()?).map_err(|_| anyhow!("a GGUF length does not fit in memory"))
    }

    fn string(&mut self) -> Result<String> {
        let n = self.len()?;
        String::from_utf8(self.take(n)?.to_vec()).context("a GGUF string is not UTF-8")
    }

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
                let n = self.len()?;
                Value::Array((0..n).map(|_| self.value(item)).collect::<Result<_>>()?)
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
    pub fn parse(bytes: &'a [u8]) -> Result<Self> {
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
            let n = r.u32()? as usize;
            let dims = (0..n).map(|_| r.len()).collect::<Result<Vec<_>>>()?;
            let kind = r.u32()?;
            let offset = r.len()?;
            tensors.insert(name, TensorInfo { dims, kind, offset });
        }
        let alignment = metadata
            .get("general.alignment")
            .and_then(Value::as_int)
            .unwrap_or(32) as usize;
        let start = r.at.div_ceil(alignment) * alignment;
        ensure!(start <= bytes.len(), "the GGUF file ends before its data");
        Ok(Self {
            metadata,
            tensors,
            data: &bytes[start..],
        })
    }

    /// The tensor `name` decoded to float32, and its shape slowest first, as PyTorch has it.
    pub fn read(&self, name: &str) -> Result<(Vec<f32>, Vec<usize>)> {
        let info = self
            .tensors
            .get(name)
            .with_context(|| format!("the GGUF file has no {name}"))?;
        let count: usize = info.dims.iter().product();
        let size = byte_size(info.kind, count)?;
        let bytes = self
            .data
            .get(info.offset..info.offset + size)
            .with_context(|| format!("{name} runs past the end of the file"))?;
        let values = decode(info.kind, bytes, count)?;
        let shape = match self.metadata.get(&format!("timingator.shape.{name}")) {
            Some(Value::Array(dims)) => dims
                .iter()
                .map(|d| d.as_int().map(|d| d as usize))
                .collect::<Option<Vec<_>>>()
                .context("a shape is not integers")?,
            _ => info.dims.iter().rev().copied().collect(),
        };
        ensure!(
            shape.iter().product::<usize>() == count,
            "{name}'s shape {shape:?} does not hold {count} values"
        );
        Ok((values, shape))
    }
}

/// Weights per block and bytes per block of each ggml type this reads.
fn block(kind: u32) -> Result<(usize, usize)> {
    Ok(match kind {
        0 => (1, 4),      // F32
        1 | 30 => (1, 2), // F16, BF16
        2 => (32, 18),    // Q4_0
        3 => (32, 20),    // Q4_1
        8 => (32, 34),    // Q8_0
        12 => (256, 144), // Q4_K
        13 => (256, 176), // Q5_K
        14 => (256, 210), // Q6_K
        20 => (32, 18),   // IQ4_NL
        39 => (32, 17),   // MXFP4
        other => bail!("ggml type {other} is not supported"),
    })
}

fn byte_size(kind: u32, count: usize) -> Result<usize> {
    let (weights, bytes) = block(kind)?;
    ensure!(
        count.is_multiple_of(weights),
        "{count} values do not fill blocks of {weights}"
    );
    Ok(count / weights * bytes)
}

const IQ4NL: [i8; 16] = [
    -127, -104, -83, -65, -49, -35, -22, -10, 1, 13, 25, 38, 53, 69, 89, 113,
];
/// E2M1 values doubled; the E8M0 scale is halved to match, as ggml does.
const MXFP4: [i8; 16] = [0, 1, 2, 3, 4, 6, 8, 12, 0, -1, -2, -3, -4, -6, -8, -12];

fn half(b: &[u8]) -> f32 {
    f16::from_le_bytes([b[0], b[1]]).to_f32()
}

/// `ggml_e8m0_to_fp32_half`: 2^(e - 128).
fn e8m0_half(e: u8) -> f32 {
    let e = u32::from(e);
    f32::from_bits(if e < 2 {
        0x0020_0000 << e
    } else {
        (e - 1) << 23
    })
}

/// Element `i` of 32 in a block of nibbles: low halves hold 0..16, high halves 16..32.
fn nibble(qs: &[u8], i: usize) -> u8 {
    if i < 16 { qs[i] & 0xF } else { qs[i - 16] >> 4 }
}

/// Sub-block `j`'s 6-bit scale and min in a K-quant's 12 scale bytes (`get_scale_min_k4`).
fn scale_min(s: &[u8], j: usize) -> (u8, u8) {
    if j < 4 {
        (s[j] & 63, s[j + 4] & 63)
    } else {
        (
            (s[j + 4] & 0xF) | ((s[j - 4] >> 6) << 4),
            (s[j + 4] >> 4) | ((s[j] >> 6) << 4),
        )
    }
}

pub fn decode(kind: u32, bytes: &[u8], count: usize) -> Result<Vec<f32>> {
    let (weights, size) = block(kind)?;
    ensure!(
        bytes.len() == count / weights * size,
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
                decode_block(kind, b, &mut out);
            }
        }
    }
    Ok(out)
}

fn decode_block(kind: u32, b: &[u8], out: &mut Vec<f32>) {
    match kind {
        2 => {
            let d = half(b);
            out.extend((0..32).map(|i| d * f32::from(nibble(&b[2..], i) as i8 - 8)));
        }
        3 => {
            let (d, m) = (half(b), half(&b[2..]));
            out.extend((0..32).map(|i| d * f32::from(nibble(&b[4..], i)) + m));
        }
        8 => {
            let d = half(b);
            out.extend(b[2..34].iter().map(|&q| d * f32::from(q as i8)));
        }
        20 => {
            let d = half(b);
            out.extend((0..32).map(|i| d * f32::from(IQ4NL[usize::from(nibble(&b[2..], i))])));
        }
        39 => {
            let d = e8m0_half(b[0]);
            out.extend((0..32).map(|i| d * f32::from(MXFP4[usize::from(nibble(&b[1..], i))])));
        }
        12 | 13 => {
            let (d, dmin) = (half(b), half(&b[2..]));
            let scales = &b[4..16];
            let (qh, qs) = if kind == 13 {
                (&b[16..48], &b[48..176])
            } else {
                (&[][..], &b[16..144])
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
            let (ql, qh, sc) = (&b[..128], &b[128..192], &b[192..208]);
            let d = half(&b[208..]);
            for p in 0..256 {
                let (h, r) = (p / 128, p % 128);
                let low = (ql[64 * h + r % 64] >> (4 * (r / 64))) & 0xF;
                let high = (qh[32 * h + r % 32] >> (2 * (r / 32))) & 3;
                let q = (low | (high << 4)) as i8 - 32;
                out.push(d * f32::from(sc[p / 16] as i8) * f32::from(q));
            }
        }
        _ => unreachable!("block() admits only the types handled here"),
    }
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
        (0..s.len())
            .step_by(2)
            .map(|i| u8::from_str_radix(&s[i..i + 2], 16).unwrap())
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
    fn a_minimal_file_parses() {
        let mut f = b"GGUF".to_vec();
        f.extend(3u32.to_le_bytes());
        f.extend(1u64.to_le_bytes());
        f.extend(1u64.to_le_bytes());
        let text = |f: &mut Vec<u8>, s: &str| {
            f.extend((s.len() as u64).to_le_bytes());
            f.extend(s.as_bytes());
        };
        text(&mut f, "general.name");
        f.extend(8u32.to_le_bytes());
        text(&mut f, "x");
        text(&mut f, "w");
        f.extend(2u32.to_le_bytes());
        f.extend(2u64.to_le_bytes());
        f.extend(1u64.to_le_bytes());
        f.extend(0u32.to_le_bytes());
        f.extend(0u64.to_le_bytes());
        f.resize(f.len().div_ceil(32) * 32, 0);
        f.extend(1.5f32.to_le_bytes());
        f.extend((-2.0f32).to_le_bytes());
        let g = Gguf::parse(&f).unwrap();
        assert_eq!(g.metadata["general.name"], Value::Str("x".into()));
        assert_eq!(g.read("w").unwrap(), (vec![1.5, -2.0], vec![1, 2]));
        assert!(Gguf::parse(&f[..30]).is_err());
        assert!(Gguf::parse(b"GGUF\x02\0\0\0").is_err());
    }
}
