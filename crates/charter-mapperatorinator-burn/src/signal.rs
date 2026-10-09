//! The two scipy routines super timing smooths and reads its histograms with, ported so they
//! agree with scipy to the last index: `scipy.ndimage.gaussian_filter1d` in its default
//! `reflect` mode, and `scipy.signal.find_peaks` with the `distance`, `prominence`, `width` and
//! `wlen` conditions (from `scipy/signal/_peak_finding.py` and `_peak_finding_utils.pyx`, BSD-3).

/// `gaussian_filter1d(x, sigma)` with scipy's defaults: a kernel truncated at four sigma and the
/// input reflected about its edges, `d c b a | a b c d | d c b a`. The sums run in scipy's order,
/// so the result matches it bit for bit: peaks on a flat top are decided by the last bits.
#[expect(
    clippy::arithmetic_side_effects,
    reason = "kernel offsets and sample positions are small, stepped as NI_Correlate1D steps them"
)]
#[expect(
    clippy::indexing_slicing,
    reason = "reflection keeps positions inside the input, and taps inside the kernel"
)]
pub(crate) fn gaussian_filter1d(x: &[f64], sigma: f64) -> Vec<f64> {
    let radius = truncated(4.0 * sigma + 0.5);
    let sigma2 = sigma * sigma;
    let mut weights: Vec<f64> = (-radius..=radius)
        .map(|i| (-0.5 / sigma2 * offset_float(i * i)).exp())
        .collect();
    let total = numpy_sum(&weights);
    for w in &mut weights {
        *w /= total;
    }
    let n = signed(x.len());
    let at = |mut i: isize| -> f64 {
        // Reflection can take more than one bounce when the input is shorter than the kernel.
        loop {
            if i < 0 {
                i = -i - 1;
            } else if i >= n {
                i = 2 * n - i - 1;
            } else {
                return x[unsigned(i)];
            }
        }
    };
    // NI_Correlate1D's symmetric path: the centre tap, then pairs from the outermost inwards.
    let centre = unsigned(radius);
    (0..n)
        .map(|i| {
            let mut acc = x[unsigned(i)] * weights[centre];
            for j in (1..=radius).rev() {
                acc += (at(i - j) + at(i + j)) * weights[centre - unsigned(j)];
            }
            acc
        })
        .collect()
}

/// `numpy.sum` of up to 128 values: eight running sums, combined pairwise, then the rest.
fn numpy_sum(a: &[f64]) -> f64 {
    assert!(
        a.len() <= 128,
        "numpy sums longer arrays by recursive halving"
    );
    let Some((first, rest)) = a.split_first_chunk::<8>() else {
        return a.iter().fold(-0.0, |s, v| s + v);
    };
    let mut r = *first;
    let (chunks, tail) = rest.as_chunks::<8>();
    for chunk in chunks {
        for (acc, v) in r.iter_mut().zip(chunk) {
            *acc += v;
        }
    }
    let mut sum = ((r[0] + r[1]) + (r[2] + r[3])) + ((r[4] + r[5]) + (r[6] + r[7]));
    for v in tail {
        sum += v;
    }
    sum
}

/// The peaks `find_peaks(x, distance, prominence=(min_prominence, None), width=(min_width, None),
/// rel_height, wlen)` keeps, with their prominences.
#[expect(clippy::indexing_slicing, reason = "local maxima are positions in x")]
pub(crate) fn find_peaks(
    x: &[f64],
    distance: f64,
    min_prominence: f64,
    min_width: f64,
    rel_height: f64,
    wlen: f64,
) -> (Vec<usize>, Vec<f64>) {
    let mut peaks = local_maxima(x);

    let by_distance = select_by_distance(
        &peaks,
        &peaks.iter().map(|&p| x[p]).collect::<Vec<_>>(),
        distance,
    );
    peaks = filter(&peaks, &by_distance);

    let wlen = if wlen > 1.0 {
        truncated(wlen.ceil())
    } else {
        -1
    };
    let (prominences, left_bases, right_bases) = prominences(x, &peaks, wlen);
    let by_prominence: Vec<bool> = prominences.iter().map(|&p| min_prominence <= p).collect();
    let (peaks, prominences, left_bases, right_bases) = (
        filter(&peaks, &by_prominence),
        filter(&prominences, &by_prominence),
        filter(&left_bases, &by_prominence),
        filter(&right_bases, &by_prominence),
    );

    let widths = widths(
        x,
        &peaks,
        rel_height,
        &prominences,
        &left_bases,
        &right_bases,
    );
    let by_width: Vec<bool> = widths.iter().map(|&w| min_width <= w).collect();
    (filter(&peaks, &by_width), filter(&prominences, &by_width))
}

/// The `values` whose `keep` is true.
fn filter<T: Copy>(values: &[T], keep: &[bool]) -> Vec<T> {
    values
        .iter()
        .zip(keep)
        .filter(|(_, k)| **k)
        .map(|(v, _)| *v)
        .collect()
}

/// `_local_maxima_1d`: every sample higher than its neighbours, a flat top counting once at its
/// middle, rounded down.
#[expect(
    clippy::indexing_slicing,
    clippy::arithmetic_side_effects,
    reason = "positions stay within 0..len, as in scipy's _local_maxima_1d"
)]
#[expect(
    clippy::float_cmp,
    reason = "a flat top is samples exactly equal, as scipy compares them"
)]
fn local_maxima(x: &[f64]) -> Vec<usize> {
    let mut peaks = Vec::new();
    if x.len() < 3 {
        return peaks;
    }
    let i_max = x.len() - 1;
    let mut i = 1;
    while i < i_max {
        if x[i - 1] < x[i] {
            let mut ahead = i + 1;
            while ahead < i_max && x[ahead] == x[i] {
                ahead += 1;
            }
            if x[ahead] < x[i] {
                peaks.push((i + ahead - 1).div_euclid(2));
                i = ahead;
            }
        }
        i += 1;
    }
    peaks
}

/// `_select_by_peak_distance`: from the highest peak down, drop the peaks closer than
/// `distance` to one kept. Ties in height go to the later peak, as a stable sort gives them.
#[expect(
    clippy::indexing_slicing,
    clippy::arithmetic_side_effects,
    reason = "j and k stay within the peaks, which ascend, as in scipy's _select_by_peak_distance"
)]
fn select_by_distance(peaks: &[usize], priority: &[f64], distance: f64) -> Vec<bool> {
    let distance = whole(distance.ceil());
    let mut keep = vec![true; peaks.len()];
    let mut order: Vec<usize> = (0..peaks.len()).collect();
    order.sort_by(|&a, &b| priority[a].total_cmp(&priority[b]));
    for &j in order.iter().rev() {
        if !keep[j] {
            continue;
        }
        let mut k = j;
        while k > 0 && peaks[j] - peaks[k - 1] < distance {
            keep[k - 1] = false;
            k -= 1;
        }
        k = j + 1;
        while k < peaks.len() && peaks[k] - peaks[j] < distance {
            keep[k] = false;
            k += 1;
        }
    }
    keep
}

/// `_peak_prominences`: each peak's height above the higher of the lowest points on either side
/// before the signal rises above it, searched within `wlen` samples around it.
#[expect(
    clippy::indexing_slicing,
    clippy::arithmetic_side_effects,
    reason = "the searches stop at the window's ends inside the signal, as in _peak_prominences"
)]
fn prominences(x: &[f64], peaks: &[usize], wlen: isize) -> (Vec<f64>, Vec<usize>, Vec<usize>) {
    let (mut prom, mut lefts, mut rights) = (Vec::new(), Vec::new(), Vec::new());
    for &peak in peaks {
        let (mut i_min, mut i_max) = (0_isize, signed(x.len()) - 1);
        if wlen >= 2 {
            i_min = i_min.max(signed(peak) - wlen.div_euclid(2));
            i_max = i_max.min(signed(peak) + wlen.div_euclid(2));
        }
        let mut i = signed(peak);
        let (mut left, mut left_min) = (peak, x[peak]);
        while i_min <= i && x[unsigned(i)] <= x[peak] {
            if x[unsigned(i)] < left_min {
                left_min = x[unsigned(i)];
                left = unsigned(i);
            }
            i -= 1;
        }
        i = signed(peak);
        let (mut right, mut right_min) = (peak, x[peak]);
        while i <= i_max && x[unsigned(i)] <= x[peak] {
            if x[unsigned(i)] < right_min {
                right_min = x[unsigned(i)];
                right = unsigned(i);
            }
            i += 1;
        }
        prom.push(x[peak] - left_min.max(right_min));
        lefts.push(left);
        rights.push(right);
    }
    (prom, lefts, rights)
}

/// `_peak_widths`: each peak's width at `rel_height` of its prominence below it, interpolated
/// between samples, within its bases.
#[expect(
    clippy::indexing_slicing,
    clippy::arithmetic_side_effects,
    reason = "the searches stay between each peak's bases, as in scipy's _peak_widths"
)]
fn widths(
    x: &[f64],
    peaks: &[usize],
    rel_height: f64,
    prominences: &[f64],
    left_bases: &[usize],
    right_bases: &[usize],
) -> Vec<f64> {
    peaks
        .iter()
        .enumerate()
        .map(|(p, &peak)| {
            let (i_min, i_max) = (left_bases[p], right_bases[p]);
            let height = x[peak] - prominences[p] * rel_height;
            let mut i = peak;
            while i_min < i && height < x[i] {
                i -= 1;
            }
            let mut left_ip = index_float(i);
            if x[i] < height {
                left_ip += (height - x[i]) / (x[i + 1] - x[i]);
            }
            i = peak;
            while i < i_max && height < x[i] {
                i += 1;
            }
            let mut right_ip = index_float(i);
            if x[i] < height {
                right_ip -= (height - x[i]) / (x[i - 1] - x[i]);
            }
            right_ip - left_ip
        })
        .collect()
}

#[expect(
    clippy::as_conversions,
    clippy::cast_possible_wrap,
    reason = "signal lengths and positions are far below isize::MAX"
)]
/// A signal position as a signed one, for searches that may step off its start.
const fn signed(position: usize) -> isize {
    position as isize
}

#[expect(
    clippy::as_conversions,
    clippy::cast_sign_loss,
    reason = "positions are checked or reflected into the signal before they index it"
)]
/// A signed signal position, inside the signal, as an index.
const fn unsigned(position: isize) -> usize {
    position as usize
}

#[expect(
    clippy::as_conversions,
    clippy::cast_possible_truncation,
    reason = "kernel radii and peak windows are small, and scipy truncates them toward zero too"
)]
/// `value` truncated toward zero, as scipy's `int` and `np.intp` do.
const fn truncated(value: f64) -> isize {
    value as isize
}

#[expect(
    clippy::as_conversions,
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    reason = "a peak distance is a small whole number of samples once ceiled"
)]
/// A whole, non-negative `value` as a count.
const fn whole(value: f64) -> usize {
    value as usize
}

#[expect(
    clippy::as_conversions,
    clippy::cast_precision_loss,
    reason = "squared kernel offsets are far below 2^52, where f64 is exact"
)]
/// A kernel offset as a float.
const fn offset_float(offset: isize) -> f64 {
    offset as f64
}

#[expect(
    clippy::as_conversions,
    clippy::cast_precision_loss,
    reason = "signal positions are far below 2^52, where f64 is exact"
)]
/// A signal position as a float.
const fn index_float(index: usize) -> f64 {
    index as f64
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Values from scipy 1.18.1: `gaussian_filter1d([0,0,0,1,0,0,0,0], 1)`.
    #[test]
    fn the_filter_matches_scipy() {
        let got = gaussian_filter1d(&[0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0], 1.0);
        let want = [
            0.004_565_692_244_646_007,
            0.053_991_127_420_704_41,
            0.241_971_445_656_600_73,
            0.398_943_469_356_097_76,
            0.241_971_445_656_600_73,
            0.053_991_127_420_704_41,
            0.004_431_861_620_031_265_5,
            0.000_133_830_624_614_741_75,
        ];
        for (g, w) in got.iter().zip(want) {
            assert_eq!(*g, w, "{got:?}");
        }
    }

    #[test]
    fn a_short_input_reflects_more_than_once() {
        let got = gaussian_filter1d(&[1.0, 1.0], 3.0);
        assert!(got.iter().all(|v| (v - 1.0).abs() < 1e-12));
    }

    /// `find_peaks(x, distance=3, prominence=0.5, width=1, rel_height=1, wlen=6)` in scipy 1.18.1
    /// gives peaks [2, 9] with prominences [1, 3]: the window caps the first peak's search to the
    /// right before the signal drops to 0.
    #[test]
    fn peaks_match_scipy() {
        let x = [
            0.0, 1.0, 2.0, 1.0, 1.5, 1.0, 0.0, 1.0, 3.0, 3.0, 3.0, 0.0, 0.2, 0.0,
        ];
        let (peaks, prom) = find_peaks(&x, 3.0, 0.5, 1.0, 1.0, 6.0);
        assert_eq!(peaks, vec![2, 9]);
        assert_eq!(prom, vec![1.0, 3.0]);
    }

    #[derive(serde::Deserialize)]
    struct Case {
        hist: Vec<f64>,
        smoothed: Vec<f64>,
        peaks: Vec<usize>,
        prominences: Vec<f64>,
    }

    /// Synthetic beat histograms through super timing's smoothing and peak picking, against
    /// scipy's results in `fixtures/signal.json`.
    #[test]
    fn super_timings_smoothing_and_peaks_match_scipy() {
        let cases: Vec<Case> =
            serde_json::from_str(include_str!("../fixtures/signal.json")).unwrap();
        for case in cases {
            let smoothed: Vec<f64> = gaussian_filter1d(&case.hist, 10.0)
                .iter()
                .map(|v| v / 20.0 * 50.0)
                .collect();
            let worst = smoothed
                .iter()
                .zip(&case.smoothed)
                .map(|(a, b)| (a - b).abs())
                .fold(0.0, f64::max);
            assert!(worst == 0.0, "{worst}");
            let (peaks, prom) = find_peaks(&case.smoothed, 50.0, 0.1, 2.0, 1.0, 50.0);
            assert_eq!(peaks, case.peaks);
            assert_eq!(prom, case.prominences);
        }
    }
}
