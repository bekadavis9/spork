import { test, describe } from 'node:test';
import assert from 'node:assert/strict';
import { F_LOW, F_HIGH, bandFreqs, bandColor, fmtHz, bandBinRange, bandEnergy } from './viz.js';

// ── bandFreqs ──────────────────────────────────────────────────────────────

describe('bandFreqs', () => {
  test('band 0 starts at F_LOW', () => {
    const [lo] = bandFreqs(0, 4);
    assert.ok(Math.abs(lo - F_LOW) < 0.01, `expected ~${F_LOW}, got ${lo}`);
  });

  test('last band ends at F_HIGH', () => {
    const [, hi] = bandFreqs(3, 4);
    assert.ok(Math.abs(hi - F_HIGH) < 0.01, `expected ~${F_HIGH}, got ${hi}`);
  });

  test('adjacent bands are contiguous (hi of i === lo of i+1)', () => {
    for (let i = 0; i < 11; i++) {
      const [, hi] = bandFreqs(i, 12);
      const [lo]   = bandFreqs(i + 1, 12);
      assert.ok(Math.abs(hi - lo) < 0.001, `gap between band ${i} and ${i+1}`);
    }
  });

  test('each band has lo < hi', () => {
    for (let i = 0; i < 24; i++) {
      const [lo, hi] = bandFreqs(i, 24);
      assert.ok(lo < hi, `band ${i}: lo=${lo} >= hi=${hi}`);
    }
  });

  test('frequencies increase with band index', () => {
    const mids = Array.from({ length: 8 }, (_, i) => {
      const [lo, hi] = bandFreqs(i, 8);
      return (lo + hi) / 2;
    });
    for (let i = 1; i < mids.length; i++) {
      assert.ok(mids[i] > mids[i - 1], `band ${i} mid not > band ${i-1} mid`);
    }
  });

  test('n=1 covers entire range', () => {
    const [lo, hi] = bandFreqs(0, 1);
    assert.ok(Math.abs(lo - F_LOW)  < 0.01);
    assert.ok(Math.abs(hi - F_HIGH) < 0.01);
  });
});

// ── bandBinRange ───────────────────────────────────────────────────────────

describe('bandBinRange', () => {
  const NYQUIST    = 22050; // 44100 Hz sample rate
  const BIN_COUNT  = 1024;

  test('loIdx <= hiIdx for every CI band at 44100 Hz', () => {
    for (let n of [4, 12, 24]) {
      for (let i = 0; i < n; i++) {
        const [lo, hi] = bandFreqs(i, n);
        const [loIdx, hiIdx] = bandBinRange(lo, hi, NYQUIST, BIN_COUNT);
        assert.ok(loIdx <= hiIdx, `n=${n} band ${i}: loIdx=${loIdx} > hiIdx=${hiIdx}`);
      }
    }
  });

  test('clamps lo to 0 when frequency is 0', () => {
    const [loIdx] = bandBinRange(0, 500, NYQUIST, BIN_COUNT);
    assert.equal(loIdx, 0);
  });

  test('clamps hi to binCount-1 when frequency exceeds nyquist', () => {
    const [, hiIdx] = bandBinRange(10000, 50000, NYQUIST, BIN_COUNT);
    assert.equal(hiIdx, BIN_COUNT - 1);
  });

  test('1 kHz maps to expected bin at 44100 Hz', () => {
    const [loIdx] = bandBinRange(1000, 2000, NYQUIST, BIN_COUNT);
    const expected = Math.floor(1000 / NYQUIST * BIN_COUNT);
    assert.equal(loIdx, expected);
  });

  test('higher frequency band gets higher bin indices', () => {
    const [lo1, hi1] = bandBinRange(500,  1000, NYQUIST, BIN_COUNT);
    const [lo2, hi2] = bandBinRange(2000, 4000, NYQUIST, BIN_COUNT);
    assert.ok(lo2 > lo1 && hi2 > hi1);
  });
});

// ── bandEnergy ─────────────────────────────────────────────────────────────

describe('bandEnergy', () => {
  test('all-zero data returns 0', () => {
    const data = new Uint8Array(1024);
    assert.equal(bandEnergy(data, 10, 20), 0);
  });

  test('uniform data returns that value', () => {
    const data = new Uint8Array(1024).fill(128);
    assert.equal(bandEnergy(data, 10, 20), 128);
  });

  test('single bin returns its exact value', () => {
    const data = new Uint8Array(1024);
    data[42] = 200;
    assert.equal(bandEnergy(data, 42, 42), 200);
  });

  test('averages two bins correctly', () => {
    const data = new Uint8Array(1024);
    data[10] = 100;
    data[11] = 200;
    assert.equal(bandEnergy(data, 10, 11), 150);
  });

  test('only counts bins in [loIdx, hiIdx] inclusive', () => {
    const data = new Uint8Array(1024).fill(255);
    data[0]  = 0; // before range
    data[5]  = 0; // inside range
    data[11] = 0; // after range
    // range [1,10]: 9 bins of 255, 1 bin of 0 → avg = 255*9/10 = 229.5
    assert.equal(bandEnergy(data, 1, 10), 255 * 9 / 10);
  });

  test('above-threshold energy (>20) from a loud band', () => {
    const data = new Uint8Array(1024).fill(100);
    assert.ok(bandEnergy(data, 0, 100) > 20);
  });

  test('below-threshold energy from silence', () => {
    const data = new Uint8Array(1024); // all zeros
    assert.ok(bandEnergy(data, 0, 100) <= 20);
  });
});

// ── bandColor ──────────────────────────────────────────────────────────────

describe('bandColor', () => {
  test('returns an hsl() string', () => {
    assert.match(bandColor(0, 4), /^hsl\(\d+,75%,58%\)$/);
  });

  test('first and last band have different hues', () => {
    assert.notEqual(bandColor(0, 4), bandColor(3, 4));
  });

  test('hue increases with band index', () => {
    const hue = (i, n) => parseInt(bandColor(i, n).match(/\d+/)[0]);
    for (let i = 1; i < 8; i++) assert.ok(hue(i, 8) > hue(i - 1, 8));
  });

  test('n=1 returns fixed colour (hue=20)', () => {
    assert.equal(bandColor(0, 1), 'hsl(20,75%,58%)');
  });

  test('n=2 last band has hue 220 (20 + 200)', () => {
    assert.equal(bandColor(1, 2), 'hsl(220,75%,58%)');
  });
});

// ── fmtHz ──────────────────────────────────────────────────────────────────

describe('fmtHz', () => {
  test('rounds to nearest 10 Hz below 1 kHz', () => {
    assert.equal(fmtHz(70),   '70 Hz');
    assert.equal(fmtHz(155),  '160 Hz');
    assert.equal(fmtHz(999),  '1000 Hz'); // rounds up to 1000
  });

  test('formats 1–2 kHz as x.x kHz', () => {
    assert.equal(fmtHz(1000), '1.0 kHz');
    assert.equal(fmtHz(1500), '1.5 kHz');
    assert.equal(fmtHz(1900), '1.9 kHz');
  });

  test('above 2 kHz rounds to nearest 500 Hz', () => {
    assert.equal(fmtHz(2000), '2 kHz');
    assert.equal(fmtHz(2500), '2.5 kHz');
    assert.equal(fmtHz(3000), '3 kHz');
    assert.equal(fmtHz(8500), '8.5 kHz');
  });

  test('F_LOW and F_HIGH format without error', () => {
    assert.ok(fmtHz(F_LOW).includes('Hz'));
    assert.ok(fmtHz(F_HIGH).includes('kHz'));
  });
});
