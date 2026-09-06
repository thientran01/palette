import unittest
from karaoke_audit import onset_metrics

class TimingAuditTests(unittest.TestCase):
    def test_audio_clock_and_display_lead_are_not_conflated(self):
        words = [{'text': 'one', 't': 1750}, {'text': 'two', 't': 2750}]
        labels = '1\t1\tone\n2\t2\ttwo\n'
        mapping = {'intercept_ms': 500, 'slope_ms': .0625}
        self.assertEqual(onset_metrics(words, labels, mapping)['bias_ms'], 250)
        self.assertEqual(onset_metrics(words, labels, mapping, 250)['median_ms'], 0)
        self.assertEqual(onset_metrics(words, '# clock: song\n' + labels, mapping)['bias_ms'], 750)

    def test_wrong_identity_and_missing_predictions_fail(self):
        mapping = {'intercept_ms': 0, 'slope_ms': .0625}
        with self.assertRaises(ValueError):
            onset_metrics([{'text': 'two', 't': 1000}], '1\t1\tone', mapping)
        with self.assertRaises(ValueError):
            onset_metrics([], '1\t1\tone', mapping)

    def test_tail_error_is_visible_even_when_median_is_perfect(self):
        words = [{'text': str(i), 't': 1000 * i + (1000 if i == 9 else 0)} for i in range(10)]
        labels = '\n'.join(f'{i}\t{i}\t{i}' for i in range(10))
        result = onset_metrics(words, labels, {'intercept_ms': 0, 'slope_ms': .0625})
        self.assertEqual(result['median_ms'], 0)
        self.assertEqual(result['within_100_pct'], 90)
        self.assertEqual(result['bias_ms'], 100)

if __name__ == '__main__':
    unittest.main()