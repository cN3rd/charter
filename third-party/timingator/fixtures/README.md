# Fixtures

`signal.json`: synthetic beat histograms, their `gaussian_filter1d(hist, 10) / 20 * 50` and the
peaks and prominences `find_peaks(x, distance=50, prominence=0.1, rel_height=1, width=2, wlen=50)`
gives, from scipy 1.18.1; timingator's `rust/parity/make_signal_fixture.py` regenerates them.
