//! The two scipy routines super timing smooths and reads its histograms with, ported so they
//! agree with scipy to the last index: `scipy.ndimage.gaussian_filter1d` in its default
//! `reflect` mode, and `scipy.signal.find_peaks` with the `distance`, `prominence`, `width` and
//! `wlen` conditions (from `scipy/signal/_peak_finding.py` and `_peak_finding_utils.pyx`, BSD-3).

/// `gaussian_filter1d(x, sigma)` with scipy's defaults: a kernel truncated at four sigma and the
/// input reflected about its edges, `d c b a | a b c d | d c b a`. The sums run in scipy's order,
/// so the result matches it bit for bit: peaks on a flat top are decided by the last bits.
pub fn gaussian_filter1d(x: &[f64], sigma: f64) -> Vec<f64> {
    let radius = (4.0 * sigma + 0.5) as isize;
    let sigma2 = sigma * sigma;
    let mut weights: Vec<f64> = (-radius..=radius)
        .map(|i| (-0.5 / sigma2 * (i * i) as f64).exp())
        .collect();
    let total = numpy_sum(&weights);
    for w in &mut weights {
        *w /= total;
    }
    let n = x.len() as isize;
    let at = |i: isize| -> f64 {
        let mut i = i;
        // Reflection can take more than one bounce when the input is shorter than the kernel.
        loop {
            if i < 0 {
                i = -i - 1;
            } else if i >= n {
                i = 2 * n - i - 1;
            } else {
                return x[i as usize];
            }
        }
    };
    // NI_Correlate1D's symmetric path: the centre tap, then pairs from the outermost inwards.
    let centre = radius as usize;
    (0..n)
        .map(|i| {
            let mut acc = x[i as usize] * weights[centre];
            for j in (1..=radius).rev() {
                acc += (at(i - j) + at(i + j)) * weights[centre - j as usize];
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
    if a.len() < 8 {
        return a.iter().fold(-0.0, |s, v| s + v);
    }
    let mut r = [0.0; 8];
    r.copy_from_slice(&a[..8]);
    let whole = a.len() - a.len() % 8;
    for chunk in a[8..whole].as_chunks::<8>().0 {
        for (acc, v) in r.iter_mut().zip(chunk) {
            *acc += v;
        }
    }
    let mut sum = ((r[0] + r[1]) + (r[2] + r[3])) + ((r[4] + r[5]) + (r[6] + r[7]));
    for v in &a[whole..] {
        sum += v;
    }
    sum
}

/// The peaks `find_peaks(x, distance, prominence=(min_prominence, None), width=(min_width, None),
/// rel_height, wlen)` keeps, with their prominences.
pub fn find_peaks(
    x: &[f64],
    distance: f64,
    min_prominence: f64,
    min_width: f64,
    rel_height: f64,
    wlen: f64,
) -> (Vec<usize>, Vec<f64>) {
    let mut peaks = local_maxima(x);

    let keep = select_by_distance(
        &peaks,
        &peaks.iter().map(|&p| x[p]).collect::<Vec<_>>(),
        distance,
    );
    peaks = filter(&peaks, &keep);

    let wlen = if wlen > 1.0 { wlen.ceil() as isize } else { -1 };
    let (prominences, left_bases, right_bases) = prominences(x, &peaks, wlen);
    let keep: Vec<bool> = prominences.iter().map(|&p| min_prominence <= p).collect();
    let (peaks, prominences, left_bases, right_bases) = (
        filter(&peaks, &keep),
        filter(&prominences, &keep),
        filter(&left_bases, &keep),
        filter(&right_bases, &keep),
    );

    let widths = widths(
        x,
        &peaks,
        rel_height,
        &prominences,
        &left_bases,
        &right_bases,
    );
    let keep: Vec<bool> = widths.iter().map(|&w| min_width <= w).collect();
    (filter(&peaks, &keep), filter(&prominences, &keep))
}

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
                peaks.push((i + ahead - 1) / 2);
                i = ahead;
            }
        }
        i += 1;
    }
    peaks
}

/// `_select_by_peak_distance`: from the highest peak down, drop the peaks closer than
/// `distance` to one kept. Ties in height go to the later peak, as a stable sort gives them.
fn select_by_distance(peaks: &[usize], priority: &[f64], distance: f64) -> Vec<bool> {
    let distance = distance.ceil() as usize;
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
        let mut k = j + 1;
        while k < peaks.len() && peaks[k] - peaks[j] < distance {
            keep[k] = false;
            k += 1;
        }
    }
    keep
}

/// `_peak_prominences`: each peak's height above the higher of the lowest points on either side
/// before the signal rises above it, searched within `wlen` samples around it.
fn prominences(x: &[f64], peaks: &[usize], wlen: isize) -> (Vec<f64>, Vec<usize>, Vec<usize>) {
    let (mut prom, mut lefts, mut rights) = (Vec::new(), Vec::new(), Vec::new());
    for &peak in peaks {
        let (mut i_min, mut i_max) = (0_isize, x.len() as isize - 1);
        if wlen >= 2 {
            i_min = i_min.max(peak as isize - wlen / 2);
            i_max = i_max.min(peak as isize + wlen / 2);
        }
        let mut i = peak as isize;
        let (mut left, mut left_min) = (peak, x[peak]);
        while i_min <= i && x[i as usize] <= x[peak] {
            if x[i as usize] < left_min {
                left_min = x[i as usize];
                left = i as usize;
            }
            i -= 1;
        }
        let mut i = peak as isize;
        let (mut right, mut right_min) = (peak, x[peak]);
        while i <= i_max && x[i as usize] <= x[peak] {
            if x[i as usize] < right_min {
                right_min = x[i as usize];
                right = i as usize;
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
            let mut left_ip = i as f64;
            if x[i] < height {
                left_ip += (height - x[i]) / (x[i + 1] - x[i]);
            }
            let mut i = peak;
            while i < i_max && height < x[i] {
                i += 1;
            }
            let mut right_ip = i as f64;
            if x[i] < height {
                right_ip -= (height - x[i]) / (x[i - 1] - x[i]);
            }
            right_ip - left_ip
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Values from scipy 1.18.1: `gaussian_filter1d([0,0,0,1,0,0,0,0], 1)`.
    #[test]
    fn the_filter_matches_scipy() {
        let got = gaussian_filter1d(&[0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0], 1.0);
        let want = [
            0.004565692244646007,
            0.05399112742070441,
            0.24197144565660073,
            0.39894346935609776,
            0.24197144565660073,
            0.05399112742070441,
            0.0044318616200312655,
            0.00013383062461474175,
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
