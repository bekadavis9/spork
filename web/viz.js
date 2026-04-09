// Pure helper functions shared between index.html and viz.test.js.
// No browser APIs — safe to import in Node.

export const F_LOW  = 70;
export const F_HIGH = 8500;

/** Log-spaced [lo, hi] Hz for band i of n. */
export function bandFreqs(i, n) {
  const lo = F_HIGH * Math.pow(F_LOW / F_HIGH, (n - i)     / n);
  const hi = F_HIGH * Math.pow(F_LOW / F_HIGH, (n - 1 - i) / n);
  return [lo, hi];
}

/** HSL colour string for band i of n. */
export function bandColor(i, n) {
  const hue = 20 + 200 * i / Math.max(n - 1, 1);
  return `hsl(${hue.toFixed(0)},75%,58%)`;
}

/** Human-readable frequency label. */
export function fmtHz(f) {
  if (f < 1000) return `${Math.round(f / 10) * 10} Hz`;
  if (f < 2000) return `${(Math.round(f / 100) / 10).toFixed(1)} kHz`;
  const r = Math.round(f / 500) * 500;
  return `${r % 1000 === 0 ? r / 1000 : (r / 1000).toFixed(1)} kHz`;
}

/**
 * Map a [lo, hi] Hz range to [loIdx, hiIdx] FFT bin indices.
 * @param {number} lo      - low frequency (Hz)
 * @param {number} hi      - high frequency (Hz)
 * @param {number} nyquist - sampleRate / 2
 * @param {number} binCount - analyser.frequencyBinCount
 */
export function bandBinRange(lo, hi, nyquist, binCount) {
  return [
    Math.max(0,             Math.floor(lo / nyquist * binCount)),
    Math.min(binCount - 1,  Math.ceil (hi / nyquist * binCount)),
  ];
}

/**
 * Average magnitude of freqData[loIdx..hiIdx] (inclusive).
 * @param {Uint8Array} freqData
 */
export function bandEnergy(freqData, loIdx, hiIdx) {
  let sum = 0;
  for (let k = loIdx; k <= hiIdx; k++) sum += freqData[k];
  return sum / Math.max(1, hiIdx - loIdx + 1);
}
