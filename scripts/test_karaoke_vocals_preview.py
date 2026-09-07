import unittest
from karaoke_vocals_preview import windows, weights

class VocalChunkTests(unittest.TestCase):
    def test_overlap_reconstructs_signal_without_gaps_or_gain_seams(self):
        for count in [1, 8, 9, 10, 11, 17, 18, 19, 25, 26, 27]:
            signal = [i * .17 - 2 for i in range(count)]
            result = [0.] * count
            total = [0.] * count
            for start, end, lo, hi in windows(count, 10, 8, 2):
                self.assertTrue(0 <= lo <= start < end <= hi <= count)
                for i, weight in enumerate(weights(end-start, 2, start > 0, end < count)):
                    result[start+i] += signal[start+i] * weight
                    total[start+i] += weight
            self.assertTrue(all(x > 0 for x in total))
            for original, output, weight in zip(signal, result, total):
                self.assertAlmostEqual(original, output / weight)

    def test_invalid_chunk_settings_fail(self):
        with self.assertRaises(ValueError):
            list(windows(10, 10, 10))

if __name__ == '__main__':
    unittest.main()